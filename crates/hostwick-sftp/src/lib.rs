//! Remote filesystem port and its adapters.
//!
//! [`RemoteFs`] is the port the transfer engine and the file browser use.
//! [`SftpFs`] is the SFTP adapter; [`LocalFs`] serves the local pane and makes
//! the transfer engine testable without a server. SCP or S3 adapters can be
//! added later behind the same trait.

mod local;
mod sftp;

use std::pin::Pin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

pub use local::{LocalFs, path_to_slash};
pub use sftp::{SftpFs, SftpOptions};

pub type BoxRead = Pin<Box<dyn AsyncRead + Send>>;
pub type BoxWrite = Pin<Box<dyn AsyncWrite + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
    /// Sub-second part of `modified`, when the filesystem provides it (local
    /// disks do; SFTP v3 only has whole seconds). Used to notice edits made
    /// within the same second.
    #[serde(default)]
    pub modified_nanos: Option<u32>,
    /// Unix mode bits (`0o755` etc.), when the server reports them.
    pub permissions: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("{path}: {message}")]
    Remote { path: String, message: String },
    #[error("request timed out: {0}")]
    Timeout(String),
    #[error("SSH: {0}")]
    Ssh(#[from] hostwick_ssh::SshError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl FsError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
            || matches!(self, Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
    }
}

/// Filesystem operations, all with POSIX-style `/` paths.
#[async_trait]
pub trait RemoteFs: Send + Sync {
    /// Directory listing, sorted directories-first then by name. Excludes `.`
    /// and `..`.
    async fn list_dir(&self, path: &str) -> Result<Vec<Entry>, FsError>;
    /// `Ok(None)` if the path does not exist.
    async fn stat(&self, path: &str) -> Result<Option<Entry>, FsError>;
    async fn mkdir(&self, path: &str) -> Result<(), FsError>;
    async fn remove_file(&self, path: &str) -> Result<(), FsError>;
    async fn remove_dir(&self, path: &str) -> Result<(), FsError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError>;
    /// Open for reading, positioned at `offset`.
    async fn open_read(&self, path: &str, offset: u64) -> Result<BoxRead, FsError>;
    /// Open for writing at `offset`; `offset == 0` truncates. Callers must
    /// `shutdown()` the writer to flush and confirm every write.
    async fn open_write(&self, path: &str, offset: u64) -> Result<BoxWrite, FsError>;
    async fn set_modified(&self, path: &str, unix_secs: u64) -> Result<(), FsError>;
    /// Absolute form of `path` (`.` resolves to the login directory).
    async fn canonicalize(&self, path: &str) -> Result<String, FsError>;

    /// `mkdir -p`.
    async fn mkdir_all(&self, path: &str) -> Result<(), FsError> {
        let mut cur = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            if cur.is_empty() && !path.starts_with('/') {
                cur.push_str(part);
            } else {
                cur.push('/');
                cur.push_str(part);
            }
            match self.stat(&cur).await? {
                Some(e) if e.is_dir() => {}
                Some(_) => {
                    return Err(FsError::Remote {
                        path: cur,
                        message: "exists and is not a directory".into(),
                    });
                }
                None => self.mkdir(&cur).await?,
            }
        }
        Ok(())
    }

    /// Read exactly `len` bytes at `offset` (or fewer at EOF).
    async fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, FsError> {
        use tokio::io::AsyncReadExt;
        let mut r = self.open_read(path, offset).await?;
        let mut buf = vec![0u8; len];
        let mut filled = 0;
        while filled < len {
            let n = r.read(&mut buf[filled..]).await?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        Ok(buf)
    }
}

/// Join POSIX paths without doubling slashes.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Parent of a POSIX path (`/a/b` → `/a`, `/a` → `/`).
pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(i) => trimmed[..i].to_owned(),
        None => ".".to_owned(),
    }
}

pub(crate) fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_dir()
            .cmp(&a.is_dir())
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
}

/// Natural sort: `file2` < `file10`, case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(join("/a", "b"), "/a/b");
        assert_eq!(join("/a/", "b"), "/a/b");
        assert_eq!(join("", "b"), "b");
        assert_eq!(parent("/a/b"), "/a");
        assert_eq!(parent("/a/b/"), "/a");
        assert_eq!(parent("/a"), "/");
        assert_eq!(parent("a"), ".");
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["file10", "File2", "file1", "a", "file02x"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a", "file1", "File2", "file02x", "file10"]);
    }
}
