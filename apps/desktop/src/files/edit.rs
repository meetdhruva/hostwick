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
//! Uploads write into the existing remote file (truncate and write), so its
//! owner and permissions stay as they were — important for config files
//! edited as root.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use hostwick_sftp::{Entry, HashAlgo, LocalFs, RemoteFs};
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

/// The per-tab temporary folder, created on first use.
pub fn new_temp_root() -> PathBuf {
    let n = uuid::Uuid::new_v4().simple().to_string();
    std::env::temp_dir().join(format!("hostwick-edit-{}-{}", std::process::id(), &n[..12]))
}

/// Remove edit folders that earlier runs left behind (a crash or a killed
/// app skips the clean-up on tab close). Only folders untouched for a day
/// go, so a second running instance never loses its files.
pub fn sweep_stale() {
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let day = Duration::from_secs(24 * 3600);
    let mine = format!("hostwick-edit-{}-", std::process::id());
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("hostwick-edit-") || name.starts_with(&mine) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
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
) {
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
        match upload(fs.as_ref(), &local_path, &target).await {
            Ok(s) => {
                stamp = s;
                last_hash = h;
                last_meta = local_meta(&local_path);
                if target != remote_path {
                    remote_path = target.clone();
                    send(EditEvent::Retarget(id, target));
                }
                status(EditStatus::Uploaded(now_hms()));
            }
            Err(e) => status(EditStatus::Error(e)),
        }
    }
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn local_meta(p: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
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
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    let tmp = local.with_extension("hostwick-download");
    let mut r = fs.open_read(remote, 0).await.map_err(|e| e.to_string())?;
    let mut w = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| e.to_string())?;
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

/// Write the local file into the remote one in place (keeps its owner and
/// permissions), then record what the server now has.
async fn upload(fs: &dyn RemoteFs, local: &Path, remote: &str) -> Result<Stamp, String> {
    let data = tokio::fs::read(local).await.map_err(|e| e.to_string())?;
    let mut w = fs
        .open_write_sized(remote, 0, data.len() as u64)
        .await
        .map_err(|e| e.to_string())?;
    w.write_all(&data).await.map_err(|e| e.to_string())?;
    w.shutdown().await.map_err(|e| e.to_string())?;
    let after = fs
        .stat(remote)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| t!("edit-missing-after-upload", path = remote).to_string())?;
    if after.size != data.len() as u64 {
        return Err(t!(
            "edit-size-mismatch",
            path = remote,
            server = after.size,
            uploaded = data.len()
        )
        .into());
    }
    Ok(Stamp::from(&after))
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
}
