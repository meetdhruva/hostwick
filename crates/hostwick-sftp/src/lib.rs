//! Remote filesystem port and its adapters.
//!
//! [`RemoteFs`] is the port the transfer engine and the file browser use.
//! [`SftpFs`] is the SFTP adapter (optionally elevated through `sudo`),
//! [`ScpFs`] the fallback for servers without SFTP, and [`LocalFs`] serves the
//! local pane and makes the transfer engine testable without a server.
//! [`open_remote_fs`] picks the right one for a host profile.

mod factory;
mod local;
mod ls;
mod names;
mod remote_cmd;
mod scp;
mod sftp;
pub mod shell;
mod sudo;

use std::pin::Pin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

pub use factory::{open_remote_fs, open_remote_fs_with};
pub use local::{LocalFs, path_to_slash};
pub use ls::{LsFlavor, decode_ls_escapes, parse_ls_line, parse_ls_output};
pub use names::{Listing, check_name, filter_listing, join_checked, unsafe_name_reason};
pub use scp::ScpFs;
pub use sftp::{SftpFs, SftpOptions};
pub use sudo::{SudoError, parse_sftp_subsystem};

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
    /// Numeric owner, when the server reports it.
    #[serde(default)]
    pub uid: Option<u32>,
    #[serde(default)]
    pub gid: Option<u32>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
}

/// Checksum algorithms for [`RemoteFs::checksum`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgo {
    Sha256,
    Md5,
}

impl HashAlgo {
    /// Length of the lowercase hex digest.
    pub fn hex_len(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Md5 => 32,
        }
    }
}

/// What [`RemoteFs::disk_usage`] found under a path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskUsage {
    /// Sum of regular file sizes (apparent size, like WinSCP's "Calculate").
    pub bytes: u64,
    pub files: u64,
    /// Directories below the path (the path itself not counted).
    pub dirs: u64,
    /// False when the walk stopped at [`DISK_USAGE_CAP`] entries.
    pub complete: bool,
}

/// Entries the default [`RemoteFs::disk_usage`] walk visits before giving up.
pub const DISK_USAGE_CAP: u64 = 2_000_000;

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
    /// The operation is not available with this protocol or server (e.g.
    /// resuming over SCP, owners on Windows).
    #[error("not supported: {0}")]
    Unsupported(String),
    /// A name from the server that is not usable as one path component
    /// here (`..`, a path separator, a drive prefix, …).
    #[error("{name}: unsafe file name ({reason})")]
    UnsafeName { name: String, reason: &'static str },
    #[error(transparent)]
    Sudo(#[from] SudoError),
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

    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported(_))
            || matches!(self, Self::Io(e) if e.kind() == std::io::ErrorKind::Unsupported)
    }
}

/// Filesystem operations, all with POSIX-style `/` paths.
#[async_trait]
pub trait RemoteFs: Send + Sync {
    /// Directory listing, sorted directories-first then by name. Excludes `.`
    /// and `..`.
    async fn list_dir(&self, path: &str) -> Result<Vec<Entry>, FsError>;
    /// [`RemoteFs::list_dir`] without entries whose names are unsafe to use
    /// as a path component here (see [`unsafe_name_reason`]), which are
    /// returned separately so the caller can tell the person.
    async fn list_dir_checked(&self, path: &str) -> Result<Listing, FsError> {
        Ok(filter_listing(path, self.list_dir(path).await?))
    }
    /// `Ok(None)` if the path does not exist. Follows symbolic links.
    async fn stat(&self, path: &str) -> Result<Option<Entry>, FsError>;
    /// Like [`RemoteFs::stat`] but does not follow a final symbolic link
    /// (`lstat`). The default looks the name up in its folder's listing.
    async fn lstat(&self, path: &str) -> Result<Option<Entry>, FsError> {
        let trimmed = path.trim_end_matches('/');
        let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
        let dir = parent(trimmed);
        Ok(self
            .list_dir(&dir)
            .await?
            .into_iter()
            .find(|e| e.name == name))
    }
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

    /// Short description for the UI, e.g. `SFTP`, `SFTP (sudo root)`, `SCP`.
    fn protocol(&self) -> String {
        "SFTP".into()
    }

    /// [`RemoteFs::open_write`] when the final size is known in advance.
    /// Protocols that must announce the size up front (SCP) need it; others
    /// ignore it.
    async fn open_write_sized(
        &self,
        path: &str,
        offset: u64,
        _size: u64,
    ) -> Result<BoxWrite, FsError> {
        self.open_write(path, offset).await
    }

    /// `chmod`: the lower 12 bits of `mode` (`0o7777`).
    async fn set_permissions(&self, path: &str, _mode: u32) -> Result<(), FsError> {
        Err(FsError::Unsupported(format!(
            "{path}: changing permissions"
        )))
    }

    /// `chown` by number. `None` keeps the current value.
    async fn set_owner(
        &self,
        path: &str,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> Result<(), FsError> {
        Err(FsError::Unsupported(format!("{path}: changing the owner")))
    }

    /// Create a symbolic link at `link` pointing to `target` (`ln -s target link`).
    async fn symlink(&self, _target: &str, link: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported(format!("{link}: symbolic links")))
    }

    /// Create a hard link at `link` to the existing file `target`.
    async fn hard_link(&self, _target: &str, link: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported(format!("{link}: hard links")))
    }

    /// Rename `from` to `to`, replacing `to` if it exists. Atomic where the
    /// protocol allows; the default never loses data either way (see
    /// [`swap_replace`]).
    async fn rename_replace(&self, from: &str, to: &str) -> Result<(), FsError> {
        swap_replace(self, from, to).await
    }

    /// Target of a symbolic link, as stored (may be relative).
    async fn read_link(&self, path: &str) -> Result<String, FsError> {
        Err(FsError::Unsupported(format!("{path}: reading links")))
    }

    /// User and group names for numeric ids, when the system can tell.
    async fn lookup_ids(
        &self,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> (Option<String>, Option<String>) {
        (None, None)
    }

    /// Lowercase hex digest of the file's contents. The default streams the
    /// file through the hasher; adapters may compute it on the server.
    async fn checksum(&self, path: &str, algo: HashAlgo) -> Result<String, FsError> {
        stream_checksum(self, path, algo).await
    }

    /// Total size and counts under `path` (recursive, symlinks not
    /// followed). The default walks the tree with [`RemoteFs::list_dir`] and
    /// stops after [`DISK_USAGE_CAP`] entries; abort the task to cancel.
    async fn disk_usage(&self, path: &str) -> Result<DiskUsage, FsError> {
        walk_disk_usage(self, path).await
    }

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

/// Replace `to` with `from` using only plain renames: `to` is first moved
/// aside, then `from` takes its place, then the old copy is removed. If the
/// second step fails the old file is put back. At every moment the data
/// exists under one of the three names; `to` is missing only between the two
/// renames.
pub async fn swap_replace<F: RemoteFs + ?Sized>(
    fs: &F,
    from: &str,
    to: &str,
) -> Result<(), FsError> {
    if fs.stat(to).await?.is_none() {
        return fs.rename(from, to).await;
    }
    let aside = format!("{to}.hostwick-old-{:08x}", rand::random::<u32>());
    fs.rename(to, &aside).await?;
    if let Err(e) = fs.rename(from, to).await {
        if let Err(back) = fs.rename(&aside, to).await {
            tracing::error!(error = %back, %aside, %to, "could not restore the original file");
        }
        return Err(e);
    }
    if let Err(e) = fs.remove_file(&aside).await {
        tracing::warn!(error = %e, %aside, "could not remove the replaced file");
    }
    Ok(())
}

/// [`RemoteFs::checksum`] by reading the whole file.
pub async fn stream_checksum<F: RemoteFs + ?Sized>(
    fs: &F,
    path: &str,
    algo: HashAlgo,
) -> Result<String, FsError> {
    use md5::Digest as _;
    use tokio::io::AsyncReadExt;
    let mut r = fs.open_read(path, 0).await?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut sha = sha2::Sha256::new();
    let mut md5 = md5::Md5::new();
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        match algo {
            HashAlgo::Sha256 => sha.update(&buf[..n]),
            HashAlgo::Md5 => md5.update(&buf[..n]),
        }
    }
    Ok(match algo {
        HashAlgo::Sha256 => hex::encode(sha.finalize()),
        HashAlgo::Md5 => hex::encode(md5.finalize()),
    })
}

/// [`RemoteFs::disk_usage`] by walking the tree.
pub async fn walk_disk_usage<F: RemoteFs + ?Sized>(
    fs: &F,
    path: &str,
) -> Result<DiskUsage, FsError> {
    let mut u = DiskUsage {
        complete: true,
        ..Default::default()
    };
    match fs.stat(path).await? {
        None => return Err(FsError::NotFound(path.to_owned())),
        Some(e) if !e.is_dir() => {
            u.files = 1;
            u.bytes = e.size;
            return Ok(u);
        }
        Some(_) => {}
    }
    let mut stack = vec![path.to_owned()];
    let mut seen = 0u64;
    while let Some(dir) = stack.pop() {
        let entries = match fs.list_dir(&dir).await {
            Ok(e) => e,
            // An unreadable subfolder should not hide everything else.
            Err(FsError::PermissionDenied(_)) if dir != path => continue,
            Err(e) => return Err(e),
        };
        for e in entries {
            seen += 1;
            if seen > DISK_USAGE_CAP {
                u.complete = false;
                return Ok(u);
            }
            match e.kind {
                EntryKind::Dir => {
                    u.dirs += 1;
                    stack.push(e.path);
                }
                EntryKind::File => {
                    u.files += 1;
                    u.bytes += e.size;
                }
                EntryKind::Symlink | EntryKind::Other => u.files += 1,
            }
        }
    }
    Ok(u)
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
