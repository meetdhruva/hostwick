use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::io::AsyncSeekExt;

use crate::{BoxRead, BoxWrite, Entry, EntryKind, FsError, RemoteFs, join, sort_entries};

/// The local disk behind the [`RemoteFs`] port.
///
/// [`LocalFs::system`] takes native paths as-is (`C:/Users/me/x`, `/home/me/x`);
/// [`LocalFs::new`] confines `/`-separated paths under a root directory.
#[derive(Debug, Clone)]
pub struct LocalFs {
    root: Option<PathBuf>,
}

impl LocalFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    /// Unrooted: paths are native absolute paths.
    pub fn system() -> Self {
        Self { root: None }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        match &self.root {
            None => PathBuf::from(path),
            Some(root) => {
                let rel = path.trim_start_matches('/');
                if rel.is_empty() || rel == "." {
                    root.clone()
                } else {
                    root.join(rel)
                }
            }
        }
    }
}

/// Native path as a `/`-separated string, without Windows' `\\?\` prefix.
pub fn path_to_slash(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

fn entry_from(name: String, path: String, md: &std::fs::Metadata) -> Entry {
    let ft = md.file_type();
    let kind = if ft.is_dir() {
        EntryKind::Dir
    } else if ft.is_file() {
        EntryKind::File
    } else if ft.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        Some(md.permissions().mode() & 0o7777)
    };
    // Windows has only a read-only flag: report it as 0o444 / 0o644.
    #[cfg(not(unix))]
    let permissions = Some(
        if md.permissions().readonly() {
            0o444
        } else {
            0o644
        } | if ft.is_dir() { 0o111 } else { 0 },
    );
    #[cfg(unix)]
    let (uid, gid) = {
        use std::os::unix::fs::MetadataExt;
        (Some(md.uid()), Some(md.gid()))
    };
    #[cfg(not(unix))]
    let (uid, gid) = (None, None);
    let since_epoch = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    Entry {
        name,
        path,
        kind,
        size: md.len(),
        modified: since_epoch.map(|d| d.as_secs()),
        modified_nanos: since_epoch.map(|d| d.subsec_nanos()),
        permissions,
        owner: None,
        group: None,
        uid,
        gid,
    }
}

fn blocking_err(e: tokio::task::JoinError) -> FsError {
    FsError::Io(std::io::Error::other(e))
}

/// Name for an id from a `name:x:id:…` file (`/etc/passwd`, `/etc/group`).
#[cfg(unix)]
fn id_name(file: &str, id: u32) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|l| {
        let mut f = l.split(':');
        let name = f.next()?;
        let _ = f.next();
        (f.next()?.parse::<u32>().ok()? == id).then(|| name.to_owned())
    })
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[async_trait]
impl RemoteFs for LocalFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<Entry>, FsError> {
        let mut rd = tokio::fs::read_dir(self.resolve(path)).await?;
        let mut out = Vec::new();
        while let Some(e) = rd.next_entry().await? {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Ok(md) = e.metadata().await {
                out.push(entry_from(name.clone(), join(path, &name), &md));
            }
        }
        sort_entries(&mut out);
        Ok(out)
    }

    async fn stat(&self, path: &str) -> Result<Option<Entry>, FsError> {
        let p = self.resolve(path);
        match tokio::fs::metadata(&p).await {
            Ok(md) => Ok(Some(entry_from(base_name(&p), path.to_owned(), &md))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        Ok(tokio::fs::create_dir(self.resolve(path)).await?)
    }

    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        Ok(tokio::fs::remove_file(self.resolve(path)).await?)
    }

    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        Ok(tokio::fs::remove_dir(self.resolve(path)).await?)
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        Ok(tokio::fs::rename(self.resolve(from), self.resolve(to)).await?)
    }

    async fn open_read(&self, path: &str, offset: u64) -> Result<BoxRead, FsError> {
        let mut f = tokio::fs::File::open(self.resolve(path)).await?;
        if offset > 0 {
            f.seek(SeekFrom::Start(offset)).await?;
        }
        Ok(Box::pin(f))
    }

    async fn open_write(&self, path: &str, offset: u64) -> Result<BoxWrite, FsError> {
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(offset == 0)
            .open(self.resolve(path))
            .await?;
        if offset > 0 {
            f.seek(SeekFrom::Start(offset)).await?;
        }
        Ok(Box::pin(f))
    }

    async fn set_modified(&self, path: &str, unix_secs: u64) -> Result<(), FsError> {
        let p = self.resolve(path);
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(unix_secs);
        tokio::task::spawn_blocking(move || {
            std::fs::File::options()
                .write(true)
                .open(p)?
                .set_modified(t)
        })
        .await
        .map_err(|e| FsError::Io(std::io::Error::other(e)))??;
        Ok(())
    }

    fn protocol(&self) -> String {
        "local".into()
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> Result<(), FsError> {
        let p = self.resolve(path);
        tokio::task::spawn_blocking(move || {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode & 0o7777))
            }
            #[cfg(not(unix))]
            {
                // Best effort: no write bit for anyone → read-only.
                let mut perms = std::fs::metadata(&p)?.permissions();
                perms.set_readonly(mode & 0o222 == 0);
                std::fs::set_permissions(&p, perms)
            }
        })
        .await
        .map_err(blocking_err)??;
        Ok(())
    }

    async fn set_owner(
        &self,
        path: &str,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<(), FsError> {
        #[cfg(unix)]
        {
            let p = self.resolve(path);
            tokio::task::spawn_blocking(move || std::os::unix::fs::lchown(p, uid, gid))
                .await
                .map_err(blocking_err)??;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (uid, gid);
            Err(FsError::Unsupported(format!(
                "{path}: owners cannot be changed on this system"
            )))
        }
    }

    async fn symlink(&self, target: &str, link: &str) -> Result<(), FsError> {
        let l = self.resolve(link);
        let target = target.to_owned();
        tokio::task::spawn_blocking(move || {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target, &l)
            }
            #[cfg(windows)]
            {
                let resolved = l
                    .parent()
                    .map(|d| d.join(&target))
                    .unwrap_or_else(|| PathBuf::from(&target));
                if resolved.is_dir() {
                    std::os::windows::fs::symlink_dir(&target, &l)
                } else {
                    std::os::windows::fs::symlink_file(&target, &l)
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = (target, l);
                Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
            }
        })
        .await
        .map_err(blocking_err)??;
        Ok(())
    }

    async fn read_link(&self, path: &str) -> Result<String, FsError> {
        let t = tokio::fs::read_link(self.resolve(path)).await?;
        Ok(t.to_string_lossy().into_owned())
    }

    async fn lookup_ids(
        &self,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> (Option<String>, Option<String>) {
        #[cfg(unix)]
        {
            tokio::task::spawn_blocking(move || {
                (
                    uid.and_then(|u| id_name("/etc/passwd", u)),
                    gid.and_then(|g| id_name("/etc/group", g)),
                )
            })
            .await
            .unwrap_or((None, None))
        }
        #[cfg(not(unix))]
        {
            let _ = (uid, gid);
            (None, None)
        }
    }

    async fn canonicalize(&self, path: &str) -> Result<String, FsError> {
        let p = tokio::fs::canonicalize(self.resolve(path)).await?;
        match &self.root {
            None => Ok(path_to_slash(&p)),
            Some(root) => {
                let root = tokio::fs::canonicalize(root).await?;
                let rel = p.strip_prefix(&root).unwrap_or(&p);
                Ok(format!("/{}", path_to_slash(rel).trim_start_matches('/')))
            }
        }
    }
}
