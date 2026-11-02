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
    #[cfg(not(unix))]
    let permissions = None;
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
    }
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
