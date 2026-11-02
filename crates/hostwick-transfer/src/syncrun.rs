//! Running syncs: one-shot (WinSCP "Synchronize") and continuous
//! (WinSCP "Keep remote directory up to date").

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use hostwick_sftp::{RemoteFs, join, parent};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::sync::{FileInfo, SyncAction, SyncMode, plan_sync, walk};
use crate::{ExistingPolicy, Job, TransferEngine, TransferError, TransferState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOptions {
    pub mode: SyncMode,
    /// Glob patterns (`*`, `?`). Without `/` they match any path component
    /// (`*.tmp`, `.git`, `node_modules`); with `/` the whole relative path.
    /// Excluded files are neither copied nor deleted.
    pub excludes: Vec<String>,
    /// Plan only; change nothing.
    pub dry_run: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            mode: SyncMode::Update,
            excludes: Vec::new(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub copied: Vec<String>,
    pub deleted: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub bytes: u64,
}

impl SyncReport {
    pub fn is_noop(&self) -> bool {
        self.copied.is_empty() && self.deleted.is_empty() && self.failed.is_empty()
    }
}

/// `*` / `?` glob, iterative with backtracking.
fn glob(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn is_excluded(rel_path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|pat| {
        if pat.contains('/') {
            glob(pat.trim_start_matches('/'), rel_path)
        } else {
            rel_path.split('/').any(|component| glob(pat, component))
        }
    })
}

async fn listing(
    fs: &dyn RemoteFs,
    root: &str,
    excludes: &[String],
) -> Result<Vec<FileInfo>, TransferError> {
    let mut files = walk(fs, root).await?;
    files.retain(|f| !is_excluded(&f.rel_path, excludes));
    Ok(files)
}

/// Make `dst_root` match `src_root` (one direction), copying through the
/// engine (pipelined, resumable, retried) and deleting extras in mirror mode.
pub async fn sync_once(
    engine: &TransferEngine,
    src: Arc<dyn RemoteFs>,
    src_root: &str,
    dst: Arc<dyn RemoteFs>,
    dst_root: &str,
    opts: &SyncOptions,
) -> Result<SyncReport, TransferError> {
    sync_inner(engine, src, src_root, dst, dst_root, opts, &HashSet::new()).await
}

/// `force`: source paths known to have changed (from the watcher), copied
/// even when size and timestamp look equal — e.g. a same-size edit made
/// within the timestamp granularity of the previous copy.
async fn sync_inner(
    engine: &TransferEngine,
    src: Arc<dyn RemoteFs>,
    src_root: &str,
    dst: Arc<dyn RemoteFs>,
    dst_root: &str,
    opts: &SyncOptions,
    force: &HashSet<String>,
) -> Result<SyncReport, TransferError> {
    let src_files = listing(src.as_ref(), src_root, &opts.excludes).await?;
    if dst.stat(dst_root).await?.is_none() {
        if opts.dry_run {
            let copied = src_files.into_iter().map(|f| f.rel_path).collect();
            return Ok(SyncReport {
                copied,
                ..Default::default()
            });
        }
        dst.mkdir_all(dst_root).await?;
    }
    let dst_files = listing(dst.as_ref(), dst_root, &opts.excludes).await?;
    let mut plan = plan_sync(&src_files, &dst_files, opts.mode);
    if !force.is_empty() {
        let planned: HashSet<String> = plan
            .iter()
            .filter_map(|a| match a {
                SyncAction::Copy(p) => Some(p.clone()),
                SyncAction::Delete(_) => None,
            })
            .collect();
        for f in &src_files {
            if force.contains(&f.rel_path) && !planned.contains(&f.rel_path) {
                plan.push(SyncAction::Copy(f.rel_path.clone()));
            }
        }
    }

    let mut report = SyncReport::default();
    if opts.dry_run {
        for a in plan {
            match a {
                SyncAction::Copy(p) => report.copied.push(p),
                SyncAction::Delete(p) => report.deleted.push(p),
            }
        }
        return Ok(report);
    }

    let mut made_dirs = HashSet::new();
    let mut jobs = Vec::new();
    for action in plan {
        match action {
            SyncAction::Copy(rel) => {
                let dst_path = join(dst_root, &rel);
                let dir = parent(&dst_path);
                if made_dirs.insert(dir.clone())
                    && let Err(e) = dst.mkdir_all(&dir).await
                {
                    report.failed.push((rel, e.to_string()));
                    continue;
                }
                let id = engine.enqueue(Job {
                    src: src.clone(),
                    src_path: join(src_root, &rel),
                    dst: dst.clone(),
                    dst_path,
                    policy: ExistingPolicy::Overwrite,
                });
                jobs.push((rel, id));
            }
            SyncAction::Delete(rel) => match dst.remove_file(&join(dst_root, &rel)).await {
                Ok(()) => report.deleted.push(rel),
                Err(e) => report.failed.push((rel, e.to_string())),
            },
        }
    }
    if opts.mode == SyncMode::Mirror {
        prune_dirs(
            src.as_ref(),
            src_root,
            dst.as_ref(),
            dst_root,
            &opts.excludes,
            &mut report,
        )
        .await;
    }
    for (rel, id) in jobs {
        match engine.wait(id).await {
            Some(TransferState::Done { bytes, .. }) => {
                report.bytes += bytes;
                report.copied.push(rel);
            }
            Some(TransferState::Skipped) => {}
            Some(TransferState::Failed { error }) => report.failed.push((rel, error)),
            other => report.failed.push((rel, format!("{other:?}"))),
        }
    }
    Ok(report)
}

/// Relative paths of all directories under `root`.
async fn dirs(fs: &dyn RemoteFs, root: &str) -> Result<Vec<String>, TransferError> {
    let mut out = Vec::new();
    let mut stack = vec![String::new()];
    while let Some(rel) = stack.pop() {
        let abs = if rel.is_empty() {
            root.to_owned()
        } else {
            join(root, &rel)
        };
        for e in fs.list_dir(&abs).await? {
            if e.is_dir() {
                let child = if rel.is_empty() {
                    e.name
                } else {
                    format!("{rel}/{}", e.name)
                };
                out.push(child.clone());
                stack.push(child);
            }
        }
    }
    Ok(out)
}

/// Mirror mode: remove destination directories absent from the source,
/// deepest first. Only empty ones go (`remove_dir` refuses otherwise), so
/// excluded files inside them are never lost.
async fn prune_dirs(
    src: &dyn RemoteFs,
    src_root: &str,
    dst: &dyn RemoteFs,
    dst_root: &str,
    excludes: &[String],
    report: &mut SyncReport,
) {
    let (Ok(src_dirs), Ok(mut dst_dirs)) = (dirs(src, src_root).await, dirs(dst, dst_root).await)
    else {
        return;
    };
    let keep: HashSet<String> = src_dirs.into_iter().collect();
    dst_dirs.retain(|d| !keep.contains(d) && !is_excluded(d, excludes));
    dst_dirs.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    for d in dst_dirs {
        if dst.remove_dir(&join(dst_root, &d)).await.is_ok() {
            report.deleted.push(format!("{d}/"));
        }
    }
}

/// Paths that are new or differ (size or mtime) between two snapshots.
fn changed_since(prev: Option<&[FileInfo]>, now: &[FileInfo]) -> HashSet<String> {
    let Some(prev) = prev else {
        return HashSet::new();
    };
    let before: std::collections::HashMap<&str, &FileInfo> =
        prev.iter().map(|f| (f.rel_path.as_str(), f)).collect();
    now.iter()
        .filter(|f| before.get(f.rel_path.as_str()).is_none_or(|b| *b != *f))
        .map(|f| f.rel_path.clone())
        .collect()
}

/// Keep `dst_root` up to date with `src_root` until `cancel` fires: sync once,
/// then poll the source every `interval` and sync when it changes.
///
/// Polling (rather than OS file-change notifications) behaves the same on
/// every OS and on network drives, and costs one directory walk per tick when
/// nothing changes. Errors (e.g. a dropped connection) are reported and the
/// loop keeps going, so a transient outage never stops the watch.
// Mirrors `sync_once`'s explicit arguments plus the watch controls.
#[allow(clippy::too_many_arguments)]
pub async fn watch(
    engine: &TransferEngine,
    src: Arc<dyn RemoteFs>,
    src_root: &str,
    dst: Arc<dyn RemoteFs>,
    dst_root: &str,
    opts: &SyncOptions,
    interval: Duration,
    cancel: CancellationToken,
    mut on_sync: impl FnMut(Result<&SyncReport, &TransferError>),
) {
    let mut last: Option<Vec<FileInfo>> = None;
    loop {
        match listing(src.as_ref(), src_root, &opts.excludes).await {
            Ok(snapshot) if last.as_ref() != Some(&snapshot) => {
                // Let in-progress writes settle before copying.
                if last.is_some() {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(300)) => {}
                        _ = cancel.cancelled() => return,
                    }
                }
                let changed = changed_since(last.as_deref(), &snapshot);
                match sync_inner(
                    engine,
                    src.clone(),
                    src_root,
                    dst.clone(),
                    dst_root,
                    opts,
                    &changed,
                )
                .await
                {
                    Ok(report) => {
                        on_sync(Ok(&report));
                        // Re-snapshot after syncing so files changed during the
                        // sync are picked up on the next tick.
                        last = if report.failed.is_empty() {
                            Some(snapshot)
                        } else {
                            None
                        };
                    }
                    Err(e) => {
                        on_sync(Err(&e));
                        last = None;
                    }
                }
            }
            Ok(_) => {}
            Err(e) => on_sync(Err(&e)),
        }
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = cancel.cancelled() => return,
        }
    }
}
