//! "Edit" in an external editor (WinSCP's most-loved feature).
//!
//! The file is downloaded into a per-tab temporary folder under its own
//! name, opened in the configured editor (or the system's default app), and
//! watched: every second its *content hash* is compared with the last one
//! (timestamps are not trusted — editors, file systems and time zones all
//! disagree about them), and each save is uploaded. Editors that save by
//! writing a new file and renaming it over the old one are handled because
//! the path, not an open handle, is read each time.
//!
//! Before uploading, the remote file's size and timestamp are compared with
//! what was downloaded (or last uploaded); if someone else changed it, the
//! person decides: overwrite, take theirs, or save the edit as a copy.
//!
//! Uploads never truncate the remote file first: the new content goes into
//! a temporary file next to it, its size is checked, it gets the original's
//! permissions and owner, and it is renamed over the original (atomically
//! where the protocol allows). A dropped connection leaves the original
//! untouched. Only when that is impossible (the folder is not writable, the
//! owner cannot be kept, or the path is a symbolic link) is the file
//! rewritten in place, with its size checked afterwards, and the status says
//! so.
//!
//! The local copies live in a private (`0700`) folder under the app's data
//! directory, never the shared temp folder. Each files tab holds a lock on
//! its folder; leftovers are swept only when no running instance holds
//! their lock, and a folder holding edits that never reached the server is
//! kept (and the person told where) instead of being deleted.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use hostwick_sftp::{Entry, EntryKind, FsError, HashAlgo, LocalFs, RemoteFs};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::ops::copy_name;

const POLL: Duration = Duration::from_secs(1);
/// Above this size the file is re-hashed only when its size or timestamp
/// changed (hashing a huge file every second would be wasteful).
const ALWAYS_HASH_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditStatus {
    Downloading,
    Watching,
    Uploading,
    /// Local time of the last upload.
    Uploaded(String),
    /// Uploaded by rewriting the remote file in place (its folder is not
    /// writable or its owner could not be kept); local time.
    UploadedInPlace(String),
    /// Waiting for the person to settle a conflict.
    Conflict,
    Error(String),
}

impl EditStatus {
    pub fn describe(&self) -> String {
        match self {
            Self::Downloading => t!("edit-status-downloading").into(),
            Self::Watching => t!("edit-status-watching").into(),
            Self::Uploading => t!("edit-status-uploading").into(),
            Self::Uploaded(time) => t!("edit-status-uploaded", time = time).into(),
            Self::UploadedInPlace(time) => t!("edit-status-uploaded-in-place", time = time).into(),
            Self::Conflict => t!("edit-status-conflict").into(),
            Self::Error(e) => t!("edit-status-error", error = e).into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Upload ours anyway.
    Overwrite,
    /// Replace the local copy with the server's (the editor reloads it).
    DownloadTheirs,
    /// Upload ours next to the original under a new name, and keep
    /// editing that copy.
    SaveCopy,
    /// Do nothing now; ask again on the next save.
    Later,
}

pub enum EditEvent {
    Status(u64, EditStatus),
    /// Downloaded: open it in the editor.
    Ready(u64, PathBuf),
    /// Now uploading to another remote path (after "save as copy").
    Retarget(u64, String),
    Conflict {
        name: String,
        reply: oneshot::Sender<ConflictChoice>,
    },
}

/// One file being edited.
pub struct EditSession {
    pub id: u64,
    pub name: String,
    pub remote_path: String,
    pub local_path: PathBuf,
    pub status: EditStatus,
    pub cancel: CancellationToken,
    /// Size and time of the local copy when it last matched the server.
    pub synced: SyncedMeta,
}

/// Local size and modification time when the copy last matched the server
/// (`None` until downloaded).
pub type SyncedMeta = Arc<Mutex<Option<LocalMeta>>>;
pub type LocalMeta = (u64, Option<SystemTime>);

impl EditSession {
    /// Whether the local copy may hold changes the server does not have.
    pub fn has_unsaved_changes(&self) -> bool {
        match self.status {
            EditStatus::Uploading | EditStatus::Conflict | EditStatus::Error(_) => true,
            EditStatus::Downloading => false,
            EditStatus::Watching | EditStatus::Uploaded(_) | EditStatus::UploadedInPlace(_) => {
                let synced = self.synced.lock().ok().and_then(|g| *g);
                synced.is_some() && local_meta(&self.local_path) != synced
            }
        }
    }
}

/// What the server had when we last synchronised with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<u64>,
    /// Where the server reports it (not SFTP v3), catches same-second edits.
    nanos: Option<u32>,
}

impl From<&Entry> for Stamp {
    fn from(e: &Entry) -> Self {
        Self {
            size: e.size,
            modified: e.modified,
            nanos: e.modified_nanos,
        }
    }
}

const EDIT_PREFIX: &str = "hostwick-edit-";
const LOCK_FILE: &str = ".hostwick-lock";
/// Marks a folder kept because it holds edits that were never uploaded.
pub const KEPT_FILE: &str = "NOT-UPLOADED.txt";

/// Where edit folders live: `<data dir>/edit`, private to the user.
pub fn edit_base(data_dir: &Path) -> PathBuf {
    data_dir.join("edit")
}

/// A files tab's folder of edit copies. Created (`0700`) and locked on
/// first use; the lock tells other instances it is in use.
pub struct EditRoot {
    pub path: PathBuf,
    lock: Option<std::fs::File>,
}

impl EditRoot {
    pub fn new(base: &Path) -> Self {
        let n = uuid::Uuid::new_v4().simple().to_string();
        Self {
            path: base.join(format!("{EDIT_PREFIX}{}-{}", std::process::id(), &n[..12])),
            lock: None,
        }
    }

    /// Create the folder and take its lock (once).
    pub fn ensure(&mut self) -> std::io::Result<()> {
        if self.lock.is_some() {
            return Ok(());
        }
        create_private_dir(&self.path)?;
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(self.path.join(LOCK_FILE))?;
        // Ours alone; a failure here only weakens the sweep's protection.
        if let Err(e) = f.try_lock() {
            tracing::warn!(error = %e, "could not lock the edit folder");
        }
        use std::io::Write as _;
        let _ = writeln!(&f, "{}", std::process::id());
        self.lock = Some(f);
        Ok(())
    }

    /// Delete the folder, or keep it (marked, unlocked) when `keep`.
    /// Returns the kept folder.
    pub fn finish(&mut self, keep: bool) -> Option<PathBuf> {
        self.lock = None;
        if !self.path.exists() {
            return None;
        }
        if keep {
            let _ = std::fs::write(
                self.path.join(KEPT_FILE),
                "Hostwick kept these edited files because they were not uploaded to the \
                 server when their tab closed. Copy what you need, then delete this folder.\n",
            );
            let _ = std::fs::remove_file(self.path.join(LOCK_FILE));
            return Some(self.path.clone());
        }
        if let Err(e) = std::fs::remove_dir_all(&self.path) {
            tracing::warn!(error = %e, dir = %self.path.display(), "could not remove edit folder");
        }
        None
    }
}

/// `mkdir -p` with `0700` on Unix for every folder it creates.
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        b.mode(0o700);
    }
    b.create(dir)
}

/// Remove edit folders that earlier runs left behind (a crash or a killed
/// app skips the clean-up on tab close): only folders no running instance
/// holds the lock of, and never one kept for unsaved edits. Folders from
/// older versions (no lock file, in the shared temp folder) go after a day.
pub fn sweep_stale(base: &Path) {
    for dir in [base.to_path_buf(), std::env::temp_dir()] {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with(EDIT_PREFIX) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if is_stale(&e.path()) {
                tracing::info!(dir = %e.path().display(), "removing a stale edit folder");
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

fn is_stale(dir: &Path) -> bool {
    if dir.join(KEPT_FILE).exists() {
        return false;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join(LOCK_FILE))
    {
        // The lock is free: its owner has exited.
        Ok(f) => f.try_lock().is_ok(),
        Err(_) => {
            // No lock file (older version, or never used): age decides.
            let day = Duration::from_secs(24 * 3600);
            std::fs::metadata(dir)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day)
        }
    }
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

async fn hash(path: &Path) -> Option<String> {
    let p = hostwick_sftp::path_to_slash(path);
    LocalFs::system().checksum(&p, HashAlgo::Sha256).await.ok()
}

/// Download `remote_path` into `local_path`, then watch and upload until
/// `cancel`. `open` = false only downloads and opens (no upload).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    id: u64,
    fs: Arc<dyn RemoteFs>,
    remote_path: String,
    local_path: PathBuf,
    watch: bool,
    events: UnboundedSender<EditEvent>,
    cancel: CancellationToken,
    synced: SyncedMeta,
) {
    let mark_synced = |m: Option<LocalMeta>| {
        if let Ok(mut g) = synced.lock() {
            *g = m;
        }
    };
    let send = |ev: EditEvent| {
        let _ = events.unbounded_send(ev);
    };
    let status = |s: EditStatus| {
        let _ = events.unbounded_send(EditEvent::Status(id, s));
    };

    let mut stamp = match download(fs.as_ref(), &remote_path, &local_path).await {
        Ok(s) => s,
        Err(e) => {
            status(EditStatus::Error(e));
            return;
        }
    };
    // Fingerprint before the editor can touch the file.
    let mut last_hash = if watch { hash(&local_path).await } else { None };
    let mut last_meta = local_meta(&local_path);
    mark_synced(last_meta);
    send(EditEvent::Ready(id, local_path.clone()));
    if !watch {
        return;
    }
    status(EditStatus::Watching);

    let mut remote_path = remote_path;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = cancel.cancelled() => return,
        }
        let meta = local_meta(&local_path);
        let Some((size, _)) = meta else {
            // Mid-save (rename over) or deleted: look again next tick.
            continue;
        };
        if size > ALWAYS_HASH_LIMIT && meta == last_meta {
            continue;
        }
        let h = hash(&local_path).await;
        if h.is_none() || h == last_hash {
            if h.is_some() && last_meta != meta {
                // Touched but unchanged: still in step with the server.
                mark_synced(meta);
            }
            last_meta = meta;
            continue;
        }
        // Let a multi-step save finish, then hash again.
        tokio::time::sleep(Duration::from_millis(250)).await;
        let h = hash(&local_path).await;
        if h.is_none() || h == last_hash {
            continue;
        }

        status(EditStatus::Uploading);
        let current = match fs.stat(&remote_path).await {
            Ok(e) => e,
            Err(e) => {
                status(EditStatus::Error(e.to_string()));
                continue;
            }
        };
        let mut target = remote_path.clone();
        if let Some(cur) = &current
            && Stamp::from(cur) != stamp
        {
            status(EditStatus::Conflict);
            let (reply, rx) = oneshot::channel();
            send(EditEvent::Conflict {
                name: file_name(&remote_path),
                reply,
            });
            let choice = tokio::select! {
                c = rx => c.unwrap_or(ConflictChoice::Later),
                _ = cancel.cancelled() => return,
            };
            match choice {
                ConflictChoice::Overwrite => {}
                ConflictChoice::Later => {
                    status(EditStatus::Watching);
                    // Ask again on the next change.
                    last_hash = h;
                    continue;
                }
                ConflictChoice::DownloadTheirs => {
                    match download(fs.as_ref(), &remote_path, &local_path).await {
                        Ok(s) => {
                            stamp = s;
                            last_hash = hash(&local_path).await;
                            last_meta = local_meta(&local_path);
                            mark_synced(last_meta);
                            status(EditStatus::Watching);
                        }
                        Err(e) => status(EditStatus::Error(e)),
                    }
                    continue;
                }
                ConflictChoice::SaveCopy => match free_copy_path(fs.as_ref(), &remote_path).await {
                    Ok(p) => target = p,
                    Err(e) => {
                        status(EditStatus::Error(e));
                        continue;
                    }
                },
            }
        }
        let before = local_meta(&local_path);
        match upload(fs.as_ref(), &local_path, &target).await {
            Ok((s, how)) => {
                stamp = s;
                last_hash = h;
                last_meta = local_meta(&local_path);
                // Saved again during the upload: the next tick uploads it.
                if last_meta == before {
                    mark_synced(last_meta);
                }
                if target != remote_path {
                    remote_path = target.clone();
                    send(EditEvent::Retarget(id, target));
                }
                status(match how {
                    Upload::Replaced => EditStatus::Uploaded(now_hms()),
                    Upload::InPlace => EditStatus::UploadedInPlace(now_hms()),
                });
            }
            Err(e) => status(EditStatus::Error(e)),
        }
    }
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn local_meta(p: &Path) -> Option<LocalMeta> {
    let md = std::fs::metadata(p).ok()?;
    Some((md.len(), md.modified().ok()))
}

/// Download into a sibling temporary file and rename it into place, so an
/// editor never sees a half-written file.
async fn download(fs: &dyn RemoteFs, remote: &str, local: &Path) -> Result<Stamp, String> {
    let meta = fs
        .stat(remote)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| t!("edit-gone", path = remote).to_string())?;
    if meta.is_dir() {
        return Err(t!("edit-is-folder", path = remote).into());
    }
    if let Some(dir) = local.parent() {
        create_private_dir(dir).map_err(|e| e.to_string())?;
    }
    let tmp = local.with_extension("hostwick-download");
    let mut r = fs.open_read(remote, 0).await.map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(&tmp).await;
    // Private (0600), and never through an existing file or link.
    let mut o = tokio::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut w = o.open(&tmp).await.map_err(|e| e.to_string())?;
    tokio::io::copy(&mut r, &mut w)
        .await
        .map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    drop(w);
    tokio::fs::rename(&tmp, local)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Stamp::from(&meta))
}

/// How an upload reached the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Upload {
    /// Temporary file renamed over the original.
    Replaced,
    /// The original rewritten in place.
    InPlace,
}

/// Why the safe replace could not be used.
enum NoReplace {
    /// Rewrite in place instead (folder not writable, owner or mode cannot
    /// be kept, symbolic link).
    InPlace(String),
    /// Give up; the original is untouched.
    Failed(String),
}

/// Upload the local file to `remote`: replace it safely when possible,
/// otherwise rewrite it in place; then record what the server has.
async fn upload(fs: &dyn RemoteFs, local: &Path, remote: &str) -> Result<(Stamp, Upload), String> {
    let data = tokio::fs::read(local).await.map_err(|e| e.to_string())?;
    match replace_via_temp(fs, remote, &data).await {
        Ok(stamp) => Ok((stamp, Upload::Replaced)),
        Err(NoReplace::Failed(e)) => Err(e),
        Err(NoReplace::InPlace(why)) => {
            tracing::info!(path = remote, %why, "uploading in place");
            let stamp = upload_in_place(fs, remote, &data).await?;
            Ok((stamp, Upload::InPlace))
        }
    }
}

async fn replace_via_temp(
    fs: &dyn RemoteFs,
    remote: &str,
    data: &[u8],
) -> Result<Stamp, NoReplace> {
    let failed = |e: FsError| NoReplace::Failed(e.to_string());
    // Replacing a link would turn it into a plain file.
    if let Ok(Some(l)) = fs.lstat(remote).await
        && l.kind == EntryKind::Symlink
    {
        return Err(NoReplace::InPlace("it is a symbolic link".into()));
    }
    let original = fs.stat(remote).await.map_err(failed)?;
    let dir = hostwick_sftp::parent(remote);
    let tmp = hostwick_sftp::join(
        &dir,
        &format!(".{}.hostwick-{:08x}.tmp", file_name(remote), rand_u32()),
    );
    let mut w = match fs.open_write_sized(&tmp, 0, data.len() as u64).await {
        Ok(w) => w,
        Err(e) => {
            return Err(NoReplace::InPlace(format!(
                "cannot create a file next to it: {e}"
            )));
        }
    };
    let written = async {
        w.write_all(data).await?;
        w.shutdown().await
    }
    .await;
    let tmp_ref = tmp.as_str();
    let discard = |e: String| async move {
        let _ = fs.remove_file(tmp_ref).await;
        e
    };
    if let Err(e) = written {
        return Err(NoReplace::Failed(discard(e.to_string()).await));
    }
    match fs.stat(&tmp).await {
        Ok(Some(e)) if e.size == data.len() as u64 => {}
        Ok(other) => {
            let got = other.map_or(0, |e| e.size);
            return Err(NoReplace::Failed(
                discard(format!(
                    "{tmp}: {got} bytes on the server, {} sent",
                    data.len()
                ))
                .await,
            ));
        }
        Err(e) => return Err(NoReplace::Failed(discard(e.to_string()).await)),
    }
    if let Some(o) = &original {
        if let Some(mode) = o.permissions
            && let Err(e) = fs.set_permissions(&tmp, mode & 0o7777).await
            && !e.is_unsupported()
        {
            let _ = fs.remove_file(&tmp).await;
            return Err(NoReplace::InPlace(format!(
                "cannot copy its permissions: {e}"
            )));
        }
        let made = fs.stat(&tmp).await.ok().flatten();
        let differs = made.is_some_and(|m| {
            (o.uid.is_some() && m.uid != o.uid) || (o.gid.is_some() && m.gid != o.gid)
        });
        if differs && let Err(e) = fs.set_owner(&tmp, o.uid, o.gid).await {
            let _ = fs.remove_file(&tmp).await;
            return Err(NoReplace::InPlace(format!("cannot keep its owner: {e}")));
        }
    }
    if let Err(e) = fs.rename_replace(&tmp, remote).await {
        let _ = fs.remove_file(&tmp).await;
        return Err(failed(e));
    }
    verify(fs, remote, data.len())
        .await
        .map_err(NoReplace::Failed)
}

/// Rewrite the remote file in place (keeps its owner and permissions where
/// the folder cannot take a new file), then check its size.
async fn upload_in_place(fs: &dyn RemoteFs, remote: &str, data: &[u8]) -> Result<Stamp, String> {
    let mut w = fs
        .open_write_sized(remote, 0, data.len() as u64)
        .await
        .map_err(|e| e.to_string())?;
    w.write_all(data).await.map_err(|e| e.to_string())?;
    w.shutdown().await.map_err(|e| e.to_string())?;
    verify(fs, remote, data.len()).await
}

async fn verify(fs: &dyn RemoteFs, remote: &str, len: usize) -> Result<Stamp, String> {
    let after = fs
        .stat(remote)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| t!("edit-missing-after-upload", path = remote).to_string())?;
    if after.size != len as u64 {
        return Err(t!(
            "edit-size-mismatch",
            path = remote,
            server = after.size,
            uploaded = len
        )
        .into());
    }
    Ok(Stamp::from(&after))
}

fn rand_u32() -> u32 {
    let b = uuid::Uuid::new_v4();
    let b = b.as_bytes();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

async fn free_copy_path(fs: &dyn RemoteFs, remote: &str) -> Result<String, String> {
    let dir = hostwick_sftp::parent(remote);
    let name = file_name(remote);
    for n in 1..100 {
        let candidate = hostwick_sftp::join(&dir, &copy_name(&name, n));
        if fs
            .stat(&candidate)
            .await
            .map_err(|e| e.to_string())?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    Err(t!("edit-no-copy-name").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;
    use futures::channel::mpsc::UnboundedReceiver;

    /// The next event `want` accepts (others are skipped), within 10 s.
    async fn next_matching(
        rx: &mut UnboundedReceiver<EditEvent>,
        want: fn(&EditEvent) -> bool,
    ) -> Result<EditEvent, ()> {
        let fut = async {
            loop {
                match rx.next().await {
                    Some(ev) if want(&ev) => return Ok(ev),
                    Some(_) => {}
                    None => return Err(()),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), fut)
            .await
            .unwrap_or(Err(()))
    }

    /// Download, edit (atomic rename like many editors), upload, conflict.
    #[tokio::test]
    async fn watch_uploads_saves_and_reports_conflicts() {
        let server = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let work = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(server.path().join("conf.txt"), b"v1\n").unwrap_or_else(|e| panic!("{e}"));
        let fs: Arc<dyn RemoteFs> = Arc::new(LocalFs::new(server.path()));
        let local = work.path().join("1").join("conf.txt");
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(
            1,
            fs.clone(),
            "/conf.txt".into(),
            local.clone(),
            true,
            tx,
            cancel.clone(),
            Arc::default(),
        ));
        assert!(matches!(
            next_matching(&mut rx, |e| matches!(e, EditEvent::Ready(..))).await,
            Ok(EditEvent::Ready(1, _))
        ));
        assert_eq!(std::fs::read(&local).unwrap_or_default(), b"v1\n");

        // Save through a temp file + rename, the way vim and VS Code may.
        let tmp = local.with_extension("swp");
        std::fs::write(&tmp, b"v2 from editor\n").unwrap_or_else(|e| panic!("{e}"));
        std::fs::rename(&tmp, &local).unwrap_or_else(|e| panic!("{e}"));
        let up = next_matching(&mut rx, |e| {
            matches!(e, EditEvent::Status(_, EditStatus::Uploaded(_)))
        })
        .await;
        assert!(up.is_ok());
        assert_eq!(
            std::fs::read(server.path().join("conf.txt")).unwrap_or_default(),
            b"v2 from editor\n"
        );

        // Someone else changes the server copy; our next save conflicts.
        std::fs::write(server.path().join("conf.txt"), b"theirs, much longer\n")
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&local, b"v3\n").unwrap_or_else(|e| panic!("{e}"));
        let ev = next_matching(&mut rx, |e| matches!(e, EditEvent::Conflict { .. }))
            .await
            .unwrap_or_else(|_| panic!("no conflict"));
        let EditEvent::Conflict { reply, .. } = ev else {
            unreachable!()
        };
        let _ = reply.send(ConflictChoice::SaveCopy);
        let re = next_matching(&mut rx, |e| matches!(e, EditEvent::Retarget(..))).await;
        assert!(matches!(re, Ok(EditEvent::Retarget(1, ref p)) if p == "/conf (my copy).txt"));
        assert_eq!(
            std::fs::read(server.path().join("conf (my copy).txt")).unwrap_or_default(),
            b"v3\n"
        );
        assert_eq!(
            std::fs::read(server.path().join("conf.txt")).unwrap_or_default(),
            b"theirs, much longer\n"
        );
        cancel.cancel();
        let _ = task.await;
    }

    /// A server where writing new files fails part-way (a dropped
    /// connection), or where new files cannot be created at all.
    struct Flaky {
        inner: LocalFs,
        /// Fail writes after this many bytes.
        cut_after: Option<usize>,
        /// Refuse to create files whose name contains this.
        refuse: Option<&'static str>,
    }

    struct Cut(usize, Option<hostwick_sftp::BoxWrite>);

    impl tokio::io::AsyncWrite for Cut {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.0 == 0 {
                return std::task::Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()));
            }
            let n = buf.len().min(self.0);
            let Some(w) = self.1.as_mut() else {
                return std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
            };
            let r = w.as_mut().poll_write(cx, &buf[..n]);
            if let std::task::Poll::Ready(Ok(k)) = r {
                self.0 -= k;
            }
            r
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.1.as_mut() {
                Some(w) => w.as_mut().poll_flush(cx),
                None => std::task::Poll::Ready(Ok(())),
            }
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.1.as_mut() {
                Some(w) => w.as_mut().poll_shutdown(cx),
                None => std::task::Poll::Ready(Ok(())),
            }
        }
    }

    #[async_trait::async_trait]
    impl RemoteFs for Flaky {
        async fn list_dir(&self, p: &str) -> Result<Vec<Entry>, FsError> {
            self.inner.list_dir(p).await
        }
        async fn stat(&self, p: &str) -> Result<Option<Entry>, FsError> {
            self.inner.stat(p).await
        }
        async fn lstat(&self, p: &str) -> Result<Option<Entry>, FsError> {
            self.inner.lstat(p).await
        }
        async fn mkdir(&self, p: &str) -> Result<(), FsError> {
            self.inner.mkdir(p).await
        }
        async fn remove_file(&self, p: &str) -> Result<(), FsError> {
            self.inner.remove_file(p).await
        }
        async fn remove_dir(&self, p: &str) -> Result<(), FsError> {
            self.inner.remove_dir(p).await
        }
        async fn rename(&self, a: &str, b: &str) -> Result<(), FsError> {
            self.inner.rename(a, b).await
        }
        async fn open_read(&self, p: &str, o: u64) -> Result<hostwick_sftp::BoxRead, FsError> {
            self.inner.open_read(p, o).await
        }
        async fn open_write(&self, p: &str, o: u64) -> Result<hostwick_sftp::BoxWrite, FsError> {
            if self.refuse.is_some_and(|r| p.contains(r)) {
                return Err(FsError::PermissionDenied(p.to_owned()));
            }
            let w = self.inner.open_write(p, o).await?;
            Ok(match self.cut_after {
                Some(n) => Box::pin(Cut(n, Some(w))),
                None => w,
            })
        }
        async fn set_modified(&self, p: &str, t: u64) -> Result<(), FsError> {
            self.inner.set_modified(p, t).await
        }
        async fn canonicalize(&self, p: &str) -> Result<String, FsError> {
            self.inner.canonicalize(p).await
        }
        async fn set_permissions(&self, p: &str, m: u32) -> Result<(), FsError> {
            self.inner.set_permissions(p, m).await
        }
        async fn set_owner(&self, p: &str, u: Option<u32>, g: Option<u32>) -> Result<(), FsError> {
            self.inner.set_owner(p, u, g).await
        }
        async fn rename_replace(&self, a: &str, b: &str) -> Result<(), FsError> {
            self.inner.rename_replace(a, b).await
        }
    }

    fn flaky(dir: &Path, cut_after: Option<usize>, refuse: Option<&'static str>) -> Flaky {
        Flaky {
            inner: LocalFs::new(dir),
            cut_after,
            refuse,
        }
    }

    /// A connection that drops mid-upload leaves the server's file as it
    /// was (it used to be truncated first).
    #[tokio::test(flavor = "current_thread")]
    async fn dropped_upload_never_damages_the_original() {
        let server = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let work = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(server.path().join("nginx.conf"), b"original config\n")
            .unwrap_or_else(|e| panic!("{e}"));
        let local = work.path().join("nginx.conf");
        std::fs::write(&local, vec![b'x'; 100_000]).unwrap_or_else(|e| panic!("{e}"));
        let fs = flaky(server.path(), Some(10_000), None);
        assert!(upload(&fs, &local, "/nginx.conf").await.is_err());
        assert_eq!(
            std::fs::read(server.path().join("nginx.conf")).unwrap_or_default(),
            b"original config\n"
        );
        // No temporary file left behind.
        let left: Vec<_> = std::fs::read_dir(server.path())
            .unwrap_or_else(|e| panic!("{e}"))
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    /// The new content replaces the file and keeps its permissions.
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn replace_keeps_mode_and_falls_back_in_place() {
        use std::os::unix::fs::PermissionsExt as _;
        let server = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let work = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let remote = server.path().join("run.sh");
        std::fs::write(&remote, b"old").unwrap_or_else(|e| panic!("{e}"));
        std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o751))
            .unwrap_or_else(|e| panic!("{e}"));
        let local = work.path().join("run.sh");
        std::fs::write(&local, b"#!/bin/sh\necho new\n").unwrap_or_else(|e| panic!("{e}"));

        let fs = flaky(server.path(), None, None);
        let (_, how) = upload(&fs, &local, "/run.sh")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(how, Upload::Replaced);
        assert_eq!(
            std::fs::read(&remote).unwrap_or_default(),
            b"#!/bin/sh\necho new\n"
        );
        let mode = std::fs::metadata(&remote).map(|m| m.permissions().mode() & 0o7777);
        assert_eq!(mode.ok(), Some(0o751));

        // The folder takes no new files: in place, size checked.
        std::fs::write(&local, b"third").unwrap_or_else(|e| panic!("{e}"));
        let fs = flaky(server.path(), None, Some(".hostwick-"));
        let (_, how) = upload(&fs, &local, "/run.sh")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(how, Upload::InPlace);
        assert_eq!(std::fs::read(&remote).unwrap_or_default(), b"third");

        // A symbolic link stays a link.
        std::os::unix::fs::symlink("run.sh", server.path().join("link.sh"))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&local, b"via link").unwrap_or_else(|e| panic!("{e}"));
        let fs = flaky(server.path(), None, None);
        let (_, how) = upload(&fs, &local, "/link.sh")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(how, Upload::InPlace);
        assert!(
            std::fs::symlink_metadata(server.path().join("link.sh"))
                .is_ok_and(|m| m.file_type().is_symlink())
        );
        assert_eq!(std::fs::read(&remote).unwrap_or_default(), b"via link");
    }

    /// Edit folders are private, locked while in use, swept only when their
    /// owner is gone, and kept (never swept) when edits were not uploaded.
    #[test]
    fn edit_folders_are_private_locked_and_kept() {
        let base = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut root = EditRoot::new(base.path());
        root.ensure().unwrap_or_else(|e| panic!("{e}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&root.path).map(|m| m.permissions().mode() & 0o777);
            assert_eq!(mode.ok(), Some(0o700));
        }
        std::fs::write(root.path.join("edit.txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        // In use (our lock): a sweep leaves it alone.
        sweep_stale(base.path());
        assert!(root.path.join("edit.txt").exists());

        // Owner gone (lock released, as when a process exits): swept.
        let mut gone = EditRoot::new(base.path());
        gone.ensure().unwrap_or_else(|e| panic!("{e}"));
        gone.lock = None;
        sweep_stale(base.path());
        assert!(!gone.path.exists());
        assert!(root.path.exists());

        // Kept for unsaved edits: never swept.
        let kept = root.finish(true).unwrap_or_else(|| panic!("kept"));
        sweep_stale(base.path());
        assert!(kept.join("edit.txt").exists() && kept.join(KEPT_FILE).exists());

        let mut clean = EditRoot::new(base.path());
        clean.ensure().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(clean.finish(false), None);
        assert!(!clean.path.exists());
    }

    #[test]
    fn unsaved_changes_are_detected() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("f");
        std::fs::write(&path, b"a").unwrap_or_else(|e| panic!("{e}"));
        let mut s = EditSession {
            id: 1,
            name: "f".into(),
            remote_path: "/f".into(),
            local_path: path.clone(),
            status: EditStatus::Watching,
            cancel: CancellationToken::new(),
            synced: Arc::new(Mutex::new(local_meta(&path))),
        };
        assert!(!s.has_unsaved_changes());
        std::fs::write(&path, b"ab").unwrap_or_else(|e| panic!("{e}"));
        assert!(s.has_unsaved_changes());
        s.synced = Arc::new(Mutex::new(local_meta(&path)));
        assert!(!s.has_unsaved_changes());
        s.status = EditStatus::Error("connection lost".into());
        assert!(s.has_unsaved_changes());
    }
}
