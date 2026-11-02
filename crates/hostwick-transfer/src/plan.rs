//! Reviewable synchronization plans (WinSCP's "Synchronize" checklist).
//!
//! [`build_plan`] compares a local and a remote tree and lists every action
//! with its reason and size, for one direction or both. Nothing changes
//! until [`execute_plan`] runs the actions the person kept checked; copies go
//! through the [`TransferEngine`] (so they show up in the transfer queue,
//! resume and retry), deletions run directly.
//!
//! Two-way ("Both") follows WinSCP: the newer file wins in each direction and
//! nothing is ever deleted. When the timestamps are equal within the
//! filesystem granularity but the sizes differ, the plan cannot know which
//! side is right and reports a conflict instead of guessing. Names that
//! differ only in letter case (`Readme` / `README`) are also a conflict in
//! two-way mode: on Windows and macOS they are the same file, and copying
//! one would overwrite the other.
//!
//! Safety rules: a one-way plan refuses to run when its source folder does
//! not exist (a typo would otherwise plan to delete everything at the
//! destination); entries whose names are unsafe as local path components are
//! left out and listed in [`SyncPlan::skipped`]; every path is joined with
//! [`join_checked`]; and deletions run before copies, so a file that is
//! deleted under one spelling and copied under another (case-only renames)
//! ends up present.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use hostwick_sftp::{EntryKind, RemoteFs, join_checked, parent};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::sync::FileInfo;
use crate::syncrun::is_excluded;
use crate::{ExistingPolicy, Job, TransferEngine, TransferError, TransferId, TransferState};

/// Timestamps closer than this are "the same" (FAT and some servers keep
/// 2-second granularity).
pub const MTIME_SLACK: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    /// Make the remote side like the local one (WinSCP "Remote").
    LocalToRemote,
    /// Make the local side like the remote one (WinSCP "Local").
    RemoteToLocal,
    /// Newer wins each way; never deletes (WinSCP "Both").
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
    /// Both sides changed (or cannot be told apart): left alone unless the
    /// person picks a side with [`PlanItem::resolve`].
    Conflict,
    /// Listed for the record; nothing to do (e.g. destination is newer).
    Skip,
}

impl ActionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
            Self::DeleteLocal => "Delete local",
            Self::DeleteRemote => "Delete remote",
            Self::Conflict => "Conflict",
            Self::Skip => "Skip",
        }
    }

    /// Whether running it changes anything.
    pub fn is_action(self) -> bool {
        !matches!(self, Self::Conflict | Self::Skip)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Only on the source side.
    New,
    /// The source copy is newer.
    Newer,
    /// Same timestamp, different size (one-way: the source wins).
    SizeDiffers,
    /// Mirror: differs and the source wins even though it is older.
    Differs,
    /// Mirror: only on the destination side.
    NotInSource,
    /// Two-way: same timestamp, different size.
    BothChanged,
    /// Two-way: one side has no timestamp and the sizes differ.
    Unknown,
    /// One-way, not mirroring: the destination copy is newer.
    DestinationNewer,
    /// Folder missing on the other side (created, possibly empty).
    NewFolder,
    /// Two-way: another file's name differs from this one only in letter
    /// case; on a case-insensitive disk one would overwrite the other.
    CaseConflict,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Newer => "newer",
            Self::SizeDiffers => "size differs",
            Self::Differs => "differs",
            Self::NotInSource => "not in source",
            Self::BothChanged => "changed on both sides (same time, different size)",
            Self::Unknown => "cannot compare (no timestamp)",
            Self::DestinationNewer => "destination is newer",
            Self::NewFolder => "new folder",
            Self::CaseConflict => "another file differs only in letter case",
        }
    }
}

/// One line of the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// Path relative to both roots, `/`-separated.
    pub rel_path: String,
    pub is_dir: bool,
    pub kind: ActionKind,
    pub reason: Reason,
    pub local: Option<FileInfo>,
    pub remote: Option<FileInfo>,
}

impl PlanItem {
    /// Bytes this action transfers.
    pub fn bytes(&self) -> u64 {
        match self.kind {
            ActionKind::Upload => self.local.as_ref().map_or(0, |f| f.size),
            ActionKind::Download => self.remote.as_ref().map_or(0, |f| f.size),
            _ => 0,
        }
    }

    /// Whether this item starts checked in a review list.
    pub fn default_selected(&self) -> bool {
        self.kind.is_action()
    }

    /// Settle a conflict (or override a skip) by choosing a side. Only
    /// copies are allowed, and only where the source exists.
    pub fn resolve(&mut self, kind: ActionKind) -> bool {
        let ok = match kind {
            ActionKind::Upload => self.local.is_some(),
            ActionKind::Download => self.remote.is_some(),
            _ => false,
        };
        if ok && !self.is_dir {
            self.kind = kind;
        }
        ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOptions {
    pub direction: SyncDirection,
    /// One-way only: delete destination files missing from the source and
    /// replace any differing file (mirror). Ignored for [`SyncDirection::Both`].
    pub delete: bool,
    /// Same patterns as [`crate::SyncOptions::excludes`].
    pub excludes: Vec<String>,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            direction: SyncDirection::LocalToRemote,
            delete: false,
            excludes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub local_root: String,
    pub remote_root: String,
    pub options: PlanOptions,
    pub items: Vec<PlanItem>,
    /// Entries left out because their names are unsafe as path components
    /// (`side: path (reason)`); nothing is done with them.
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// Counts for a summary line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTotals {
    pub uploads: usize,
    pub downloads: usize,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub delete_local: usize,
    pub delete_remote: usize,
    pub conflicts: usize,
    pub skipped: usize,
}

impl SyncPlan {
    /// Totals over the items for which `selected` is true.
    pub fn totals(&self, selected: impl Fn(usize, &PlanItem) -> bool) -> PlanTotals {
        let mut t = PlanTotals::default();
        for (i, it) in self.items.iter().enumerate() {
            match it.kind {
                ActionKind::Conflict => t.conflicts += 1,
                ActionKind::Skip => t.skipped += 1,
                _ if !selected(i, it) => {}
                ActionKind::Upload => {
                    t.uploads += 1;
                    t.upload_bytes += it.bytes();
                }
                ActionKind::Download => {
                    t.downloads += 1;
                    t.download_bytes += it.bytes();
                }
                ActionKind::DeleteLocal => t.delete_local += 1,
                ActionKind::DeleteRemote => t.delete_remote += 1,
            }
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        !self
            .items
            .iter()
            .any(|i| i.kind.is_action() || i.kind == ActionKind::Conflict)
    }
}

/// Both trees, as returned by [`walk_tree`].
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub files: Vec<FileInfo>,
    /// Relative paths of every directory below the root.
    pub dirs: Vec<String>,
    /// Relative paths left out because of unsafe names, with the reason.
    pub skipped: Vec<(String, &'static str)>,
}

/// Files (regular, without our `.filepart`s) and directories under `root`.
/// A missing root is an empty tree.
pub async fn walk_tree(fs: &dyn RemoteFs, root: &str) -> Result<Tree, TransferError> {
    let mut tree = Tree::default();
    match fs.stat(root).await? {
        None => return Ok(tree),
        Some(e) if !e.is_dir() => {
            return Err(TransferError::Fs(hostwick_sftp::FsError::Remote {
                path: root.to_owned(),
                message: "not a folder".into(),
            }));
        }
        Some(_) => {}
    }
    let mut stack = vec![String::new()];
    while let Some(rel_dir) = stack.pop() {
        let abs = if rel_dir.is_empty() {
            root.to_owned()
        } else {
            join_checked(root, &rel_dir)?
        };
        let listing = fs.list_dir_checked(&abs).await?;
        for (name, reason) in listing.skipped {
            let rel = if rel_dir.is_empty() {
                name
            } else {
                format!("{rel_dir}/{name}")
            };
            tree.skipped.push((rel, reason));
        }
        for e in listing.entries {
            let rel = if rel_dir.is_empty() {
                e.name.clone()
            } else {
                format!("{rel_dir}/{}", e.name)
            };
            match e.kind {
                EntryKind::Dir => {
                    tree.dirs.push(rel.clone());
                    stack.push(rel);
                }
                EntryKind::File if !e.name.ends_with(crate::PART_SUFFIX) => {
                    tree.files.push(FileInfo {
                        rel_path: rel,
                        size: e.size,
                        modified: e.modified,
                        modified_nanos: e.modified_nanos,
                    });
                }
                _ => {}
            }
        }
    }
    tree.files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    tree.dirs.sort();
    Ok(tree)
}

/// List both sides and plan. A one-way plan fails with
/// [`TransferError::SourceMissing`] when its source folder does not exist;
/// a missing destination (or either side of a two-way plan) is simply empty
/// and gets created.
pub async fn build_plan(
    local: &dyn RemoteFs,
    local_root: &str,
    remote: &dyn RemoteFs,
    remote_root: &str,
    options: PlanOptions,
) -> Result<SyncPlan, TransferError> {
    let source = match options.direction {
        SyncDirection::LocalToRemote => Some((local, local_root)),
        SyncDirection::RemoteToLocal => Some((remote, remote_root)),
        SyncDirection::Both => None,
    };
    if let Some((fs, root)) = source
        && fs.stat(root).await?.is_none()
    {
        return Err(TransferError::SourceMissing(root.to_owned()));
    }
    let (l, r) = tokio::try_join!(walk_tree(local, local_root), walk_tree(remote, remote_root))?;
    let items = plan_trees(&l, &r, &options);
    let skipped = l
        .skipped
        .iter()
        .map(|(p, why)| format!("local: {p} ({why})"))
        .chain(
            r.skipped
                .iter()
                .map(|(p, why)| format!("remote: {p} ({why})")),
        )
        .collect();
    Ok(SyncPlan {
        local_root: local_root.to_owned(),
        remote_root: remote_root.to_owned(),
        options,
        items,
        skipped,
    })
}

fn filter(tree: &Tree, excludes: &[String]) -> Tree {
    Tree {
        files: tree
            .files
            .iter()
            .filter(|f| !is_excluded(&f.rel_path, excludes))
            .cloned()
            .collect(),
        dirs: tree
            .dirs
            .iter()
            .filter(|d| !is_excluded(d, excludes))
            .cloned()
            .collect(),
        skipped: Vec::new(),
    }
}

/// The pure planning step (no I/O), sorted by path.
pub fn plan_trees(local: &Tree, remote: &Tree, o: &PlanOptions) -> Vec<PlanItem> {
    let (local, remote) = (filter(local, &o.excludes), filter(remote, &o.excludes));
    let l: HashMap<&str, &FileInfo> = local
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), f))
        .collect();
    let r: HashMap<&str, &FileInfo> = remote
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), f))
        .collect();
    let paths: BTreeSet<&str> = l.keys().chain(r.keys()).copied().collect();
    let mirror = o.delete && o.direction != SyncDirection::Both;
    // Two-way: paths that collide when letter case is ignored.
    let mut folded: HashMap<String, usize> = HashMap::new();
    if o.direction == SyncDirection::Both {
        for p in &paths {
            *folded.entry(p.to_lowercase()).or_default() += 1;
        }
    }
    let case_clash = |p: &str| folded.get(&p.to_lowercase()).is_some_and(|n| *n > 1);
    let mut out = Vec::new();

    let item = |rel: &str, kind, reason, lf: Option<&&FileInfo>, rf: Option<&&FileInfo>| PlanItem {
        rel_path: rel.to_owned(),
        is_dir: false,
        kind,
        reason,
        local: lf.map(|f| (*f).clone()),
        remote: rf.map(|f| (*f).clone()),
    };

    for p in paths {
        let (lf, rf) = (l.get(p), r.get(p));
        let planned = match (o.direction, lf, rf) {
            (_, None, None) => None,
            (SyncDirection::Both, _, _) if case_clash(p) => {
                Some((ActionKind::Conflict, Reason::CaseConflict))
            }
            // Only on one side.
            (SyncDirection::LocalToRemote | SyncDirection::Both, Some(_), None) => {
                Some((ActionKind::Upload, Reason::New))
            }
            (SyncDirection::RemoteToLocal | SyncDirection::Both, None, Some(_)) => {
                Some((ActionKind::Download, Reason::New))
            }
            (SyncDirection::LocalToRemote, None, Some(_)) => {
                mirror.then_some((ActionKind::DeleteRemote, Reason::NotInSource))
            }
            (SyncDirection::RemoteToLocal, Some(_), None) => {
                mirror.then_some((ActionKind::DeleteLocal, Reason::NotInSource))
            }
            // On both sides.
            (dir, Some(a), Some(b)) => compare(dir, mirror, a, b),
        };
        if let Some((kind, reason)) = planned {
            out.push(item(p, kind, reason, lf, rf));
        }
    }

    // Folders: create missing ones on the destination (empty ones would
    // otherwise be lost); in mirror mode, remove extra ones.
    let ld: HashSet<&str> = local.dirs.iter().map(String::as_str).collect();
    let rd: HashSet<&str> = remote.dirs.iter().map(String::as_str).collect();
    let dir_item = |rel: &str, kind, reason| PlanItem {
        rel_path: rel.to_owned(),
        is_dir: true,
        kind,
        reason,
        local: None,
        remote: None,
    };
    let to_remote = matches!(
        o.direction,
        SyncDirection::LocalToRemote | SyncDirection::Both
    );
    let to_local = matches!(
        o.direction,
        SyncDirection::RemoteToLocal | SyncDirection::Both
    );
    for d in &local.dirs {
        if !rd.contains(d.as_str()) {
            if to_remote {
                out.push(dir_item(d, ActionKind::Upload, Reason::NewFolder));
            } else if mirror {
                out.push(dir_item(d, ActionKind::DeleteLocal, Reason::NotInSource));
            }
        }
    }
    for d in &remote.dirs {
        if !ld.contains(d.as_str()) {
            if to_local {
                out.push(dir_item(d, ActionKind::Download, Reason::NewFolder));
            } else if mirror {
                out.push(dir_item(d, ActionKind::DeleteRemote, Reason::NotInSource));
            }
        }
    }
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path).then(b.is_dir.cmp(&a.is_dir)));
    out
}

fn compare(
    dir: SyncDirection,
    mirror: bool,
    local: &FileInfo,
    remote: &FileInfo,
) -> Option<(ActionKind, Reason)> {
    use std::cmp::Ordering;
    let same_size = local.size == remote.size;
    // Newer side, if the difference is beyond the granularity.
    let newer = match (local.modified, remote.modified) {
        (Some(a), Some(b)) if a.abs_diff(b) > MTIME_SLACK => Some(a.cmp(&b)),
        (Some(_), Some(_)) => Some(Ordering::Equal),
        _ => None,
    };
    match dir {
        SyncDirection::Both => match newer {
            Some(Ordering::Greater) => Some((ActionKind::Upload, Reason::Newer)),
            Some(Ordering::Less) => Some((ActionKind::Download, Reason::Newer)),
            Some(Ordering::Equal) if same_size => None,
            Some(Ordering::Equal) => Some((ActionKind::Conflict, Reason::BothChanged)),
            None if same_size => None,
            None => Some((ActionKind::Conflict, Reason::Unknown)),
        },
        SyncDirection::LocalToRemote | SyncDirection::RemoteToLocal => {
            let (copy, src_newer) = if dir == SyncDirection::LocalToRemote {
                (ActionKind::Upload, Ordering::Greater)
            } else {
                (ActionKind::Download, Ordering::Less)
            };
            match newer {
                Some(o) if o == src_newer => Some((copy, Reason::Newer)),
                Some(Ordering::Equal) | None if same_size => None,
                Some(Ordering::Equal) | None => Some((copy, Reason::SizeDiffers)),
                // Destination is newer.
                Some(_) if mirror => Some((copy, Reason::Differs)),
                Some(_) => Some((ActionKind::Skip, Reason::DestinationNewer)),
            }
        }
    }
}

/// What [`execute_plan`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanReport {
    pub uploaded: Vec<String>,
    pub downloaded: Vec<String>,
    pub deleted_local: Vec<String>,
    pub deleted_remote: Vec<String>,
    pub created_dirs: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub bytes: u64,
    pub cancelled: bool,
}

impl PlanReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let mut push = |n: usize, what: &str| {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        };
        push(self.uploaded.len(), "uploaded");
        push(self.downloaded.len(), "downloaded");
        push(
            self.deleted_local.len() + self.deleted_remote.len(),
            "deleted",
        );
        push(self.created_dirs.len(), "folders created");
        push(self.failed.len(), "failed");
        if parts.is_empty() {
            "nothing to do".into()
        } else {
            parts.join(", ")
        }
    }
}

/// Run the items for which `selected` is true (conflicts and skips never
/// run). Deletions run first (files, then folders deepest first and only
/// when empty, so excluded files are never lost), then folders are created
/// and copies are queued on `engine`. Entries skipped for unsafe names are
/// reported as failures.
pub async fn execute_plan(
    engine: &TransferEngine,
    local: Arc<dyn RemoteFs>,
    remote: Arc<dyn RemoteFs>,
    plan: &SyncPlan,
    selected: impl Fn(usize, &PlanItem) -> bool,
    cancel: &CancellationToken,
) -> PlanReport {
    let mut report = PlanReport::default();
    for s in &plan.skipped {
        report
            .failed
            .push((s.clone(), "skipped: unsafe file name".to_owned()));
    }
    let chosen: Vec<&PlanItem> = plan
        .items
        .iter()
        .enumerate()
        .filter(|(i, it)| it.kind.is_action() && selected(*i, it))
        .map(|(_, it)| it)
        .collect();
    let (lr, rr) = (plan.local_root.as_str(), plan.remote_root.as_str());

    // 1. File deletions, then folder deletions deepest first. Before any
    // copy: on a case-insensitive disk `File.txt` (deleted) and `file.txt`
    // (copied) are the same file, and the copy must win.
    let mut dirs: Vec<&PlanItem> = Vec::new();
    for it in &chosen {
        let (fs, root) = match it.kind {
            ActionKind::DeleteLocal => (&local, lr),
            ActionKind::DeleteRemote => (&remote, rr),
            _ => continue,
        };
        if cancel.is_cancelled() {
            report.cancelled = true;
            return report;
        }
        if it.is_dir {
            dirs.push(it);
            continue;
        }
        let res = match join_checked(root, &it.rel_path) {
            Ok(path) => fs.remove_file(&path).await,
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => record_delete(&mut report, it),
            Err(e) => report.failed.push((it.rel_path.clone(), e.to_string())),
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.rel_path.matches('/').count()));
    for it in dirs {
        let (fs, root) = if it.kind == ActionKind::DeleteLocal {
            (&local, lr)
        } else {
            (&remote, rr)
        };
        let res = match join_checked(root, &it.rel_path) {
            Ok(path) => fs.remove_dir(&path).await,
            Err(e) => Err(e),
        };
        match res {
            Ok(()) => record_delete(&mut report, it),
            Err(e) => report
                .failed
                .push((format!("{}/", it.rel_path), e.to_string())),
        }
    }

    // 2. Folders to create (parents first).
    let mut made: HashSet<(bool, String)> = HashSet::new();
    for it in chosen.iter().filter(|i| i.is_dir) {
        let (fs, root, remote_side) = match it.kind {
            ActionKind::Upload => (&remote, rr, true),
            ActionKind::Download => (&local, lr, false),
            _ => continue,
        };
        let res = match join_checked(root, &it.rel_path) {
            Ok(path) => fs.mkdir_all(&path).await.map(|()| path),
            Err(e) => Err(e),
        };
        match res {
            Ok(path) => {
                made.insert((remote_side, path));
                report.created_dirs.push(it.rel_path.clone());
            }
            Err(e) => report.failed.push((it.rel_path.clone(), e.to_string())),
        }
    }

    // 3. Copies through the engine.
    let mut jobs: Vec<(TransferId, &PlanItem)> = Vec::new();
    for it in chosen.iter().filter(|i| !i.is_dir) {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let (src, src_root, dst, dst_root, remote_side) = match it.kind {
            ActionKind::Upload => (&local, lr, &remote, rr, true),
            ActionKind::Download => (&remote, rr, &local, lr, false),
            _ => continue,
        };
        let paths = (
            join_checked(src_root, &it.rel_path),
            join_checked(dst_root, &it.rel_path),
        );
        let (src_path, dst_path) = match paths {
            (Ok(s), Ok(d)) => (s, d),
            (Err(e), _) | (_, Err(e)) => {
                report.failed.push((it.rel_path.clone(), e.to_string()));
                continue;
            }
        };
        let dir = parent(&dst_path);
        if !made.contains(&(remote_side, dir.clone())) {
            if let Err(e) = dst.mkdir_all(&dir).await {
                report.failed.push((it.rel_path.clone(), e.to_string()));
                continue;
            }
            made.insert((remote_side, dir));
        }
        let id = engine.enqueue(Job {
            src: src.clone(),
            src_path,
            dst: dst.clone(),
            dst_path,
            policy: ExistingPolicy::Overwrite,
        });
        jobs.push((id, it));
    }

    // 4. Wait for the copies (cancelling the rest if asked to).
    for (id, it) in jobs {
        let state = tokio::select! {
            s = engine.wait(id) => s,
            _ = cancel.cancelled() => {
                engine.cancel(id);
                report.cancelled = true;
                engine.wait(id).await
            }
        };
        match state {
            Some(TransferState::Done { bytes, .. }) => {
                report.bytes += bytes;
                if it.kind == ActionKind::Upload {
                    report.uploaded.push(it.rel_path.clone());
                } else {
                    report.downloaded.push(it.rel_path.clone());
                }
            }
            Some(TransferState::Skipped) => {}
            Some(TransferState::Cancelled) => report.cancelled = true,
            Some(TransferState::Failed { error }) => {
                report.failed.push((it.rel_path.clone(), error))
            }
            other => report
                .failed
                .push((it.rel_path.clone(), format!("{other:?}"))),
        }
    }
    report
}

fn record_delete(report: &mut PlanReport, it: &PlanItem) {
    let name = if it.is_dir {
        format!("{}/", it.rel_path)
    } else {
        it.rel_path.clone()
    };
    if it.kind == ActionKind::DeleteLocal {
        report.deleted_local.push(name);
    } else {
        report.deleted_remote.push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str, size: u64, m: u64) -> FileInfo {
        FileInfo {
            rel_path: p.into(),
            size,
            modified: Some(m),
            modified_nanos: None,
        }
    }

    fn tree(files: Vec<FileInfo>, dirs: &[&str]) -> Tree {
        Tree {
            files,
            dirs: dirs.iter().map(|d| (*d).to_owned()).collect(),
            skipped: Vec::new(),
        }
    }

    fn kinds(items: &[PlanItem]) -> Vec<(String, ActionKind, Reason)> {
        items
            .iter()
            .map(|i| {
                let p = if i.is_dir {
                    format!("{}/", i.rel_path)
                } else {
                    i.rel_path.clone()
                };
                (p, i.kind, i.reason)
            })
            .collect()
    }

    fn opts(direction: SyncDirection, delete: bool) -> PlanOptions {
        PlanOptions {
            direction,
            delete,
            excludes: vec![],
        }
    }

    fn sample() -> (Tree, Tree) {
        let local = tree(
            vec![
                f("same", 1, 100),
                f("local-only", 2, 100),
                f("local-newer", 3, 500),
                f("remote-newer", 4, 100),
                f("conflict", 5, 300),
                f("slack", 6, 301),
                f("d/x", 7, 100),
            ],
            &["d", "empty-local"],
        );
        let remote = tree(
            vec![
                f("same", 1, 100),
                f("remote-only", 8, 100),
                f("local-newer", 3, 100),
                f("remote-newer", 9, 500),
                f("conflict", 50, 301),
                f("slack", 6, 300),
            ],
            &["empty-remote"],
        );
        (local, remote)
    }

    #[test]
    fn both_newer_wins_and_never_deletes() {
        let (l, r) = sample();
        let plan = plan_trees(&l, &r, &opts(SyncDirection::Both, true));
        use ActionKind::*;
        assert_eq!(
            kinds(&plan),
            vec![
                ("conflict".into(), Conflict, Reason::BothChanged),
                ("d/".into(), Upload, Reason::NewFolder),
                ("d/x".into(), Upload, Reason::New),
                ("empty-local/".into(), Upload, Reason::NewFolder),
                ("empty-remote/".into(), Download, Reason::NewFolder),
                ("local-newer".into(), Upload, Reason::Newer),
                ("local-only".into(), Upload, Reason::New),
                ("remote-newer".into(), Download, Reason::Newer),
                ("remote-only".into(), Download, Reason::New),
            ]
        );
        let p = SyncPlan {
            local_root: "/l".into(),
            remote_root: "/r".into(),
            options: opts(SyncDirection::Both, false),
            items: plan,
            skipped: Vec::new(),
        };
        let t = p.totals(|_, i| i.default_selected());
        assert_eq!((t.uploads, t.downloads, t.conflicts), (5, 3, 1));
        assert_eq!((t.upload_bytes, t.download_bytes), (2 + 3 + 7, 8 + 9));
        assert!(!p.items[0].default_selected());
    }

    #[test]
    fn one_way_update_and_mirror() {
        let (l, r) = sample();
        use ActionKind::*;
        let up = plan_trees(&l, &r, &opts(SyncDirection::LocalToRemote, false));
        assert_eq!(
            kinds(&up),
            vec![
                ("conflict".into(), Upload, Reason::SizeDiffers),
                ("d/".into(), Upload, Reason::NewFolder),
                ("d/x".into(), Upload, Reason::New),
                ("empty-local/".into(), Upload, Reason::NewFolder),
                ("local-newer".into(), Upload, Reason::Newer),
                ("local-only".into(), Upload, Reason::New),
                ("remote-newer".into(), Skip, Reason::DestinationNewer),
            ]
        );
        let mirror = plan_trees(&l, &r, &opts(SyncDirection::LocalToRemote, true));
        assert_eq!(
            kinds(&mirror),
            vec![
                ("conflict".into(), Upload, Reason::SizeDiffers),
                ("d/".into(), Upload, Reason::NewFolder),
                ("d/x".into(), Upload, Reason::New),
                ("empty-local/".into(), Upload, Reason::NewFolder),
                ("empty-remote/".into(), DeleteRemote, Reason::NotInSource),
                ("local-newer".into(), Upload, Reason::Newer),
                ("local-only".into(), Upload, Reason::New),
                ("remote-newer".into(), Upload, Reason::Differs),
                ("remote-only".into(), DeleteRemote, Reason::NotInSource),
            ]
        );
        let down = plan_trees(&l, &r, &opts(SyncDirection::RemoteToLocal, true));
        assert_eq!(
            kinds(&down),
            vec![
                ("conflict".into(), Download, Reason::SizeDiffers),
                ("d/".into(), DeleteLocal, Reason::NotInSource),
                ("d/x".into(), DeleteLocal, Reason::NotInSource),
                ("empty-local/".into(), DeleteLocal, Reason::NotInSource),
                ("empty-remote/".into(), Download, Reason::NewFolder),
                ("local-newer".into(), Download, Reason::Differs),
                ("local-only".into(), DeleteLocal, Reason::NotInSource),
                ("remote-newer".into(), Download, Reason::Newer),
                ("remote-only".into(), Download, Reason::New),
            ]
        );
    }

    /// Two-way: names that differ only in case never overwrite each other.
    #[test]
    fn case_variants_are_conflicts_in_two_way() {
        let l = tree(
            vec![f("docs/Readme.md", 1, 100), f("a.txt", 1, 100)],
            &["docs"],
        );
        let r = tree(
            vec![f("docs/README.md", 2, 200), f("A.TXT", 3, 300)],
            &["docs"],
        );
        let plan = plan_trees(&l, &r, &opts(SyncDirection::Both, false));
        assert!(
            plan.iter()
                .all(|i| i.kind == ActionKind::Conflict && i.reason == Reason::CaseConflict),
            "{:?}",
            kinds(&plan)
        );
        assert_eq!(plan.len(), 4);
        // One-way plans are unchanged.
        let up = plan_trees(&l, &r, &opts(SyncDirection::LocalToRemote, false));
        assert!(up.iter().any(|i| i.kind == ActionKind::Upload));
    }

    #[test]
    fn excludes_and_missing_timestamps() {
        let l = tree(
            vec![
                f("keep", 1, 1),
                f(".git/HEAD", 1, 1),
                FileInfo {
                    rel_path: "nots".into(),
                    size: 1,
                    modified: None,
                    modified_nanos: None,
                },
            ],
            &[".git"],
        );
        let r = tree(vec![f("nots", 2, 1)], &[]);
        let o = PlanOptions {
            direction: SyncDirection::Both,
            delete: false,
            excludes: vec![".git".into()],
        };
        let plan = plan_trees(&l, &r, &o);
        assert_eq!(
            kinds(&plan),
            vec![
                ("keep".into(), ActionKind::Upload, Reason::New),
                ("nots".into(), ActionKind::Conflict, Reason::Unknown),
            ]
        );
        let mut c = plan[1].clone();
        assert!(c.resolve(ActionKind::Download));
        assert_eq!(c.kind, ActionKind::Download);
        assert!(!c.resolve(ActionKind::DeleteLocal));
    }
}
