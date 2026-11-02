use std::io::SeekFrom;

use async_trait::async_trait;
use hostwick_ssh::Connection;
use russh_sftp::client::{Config, SftpSession, error::Error as SftpError};
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use tokio::io::AsyncSeekExt;

use crate::{BoxRead, BoxWrite, Entry, EntryKind, FsError, RemoteFs, join, sort_entries};

/// Throughput knobs. The defaults keep 64 requests in flight per open file,
/// like OpenSSH's `sftp` client, which hides round-trip latency on WAN links.
#[derive(Debug, Clone)]
pub struct SftpOptions {
    pub max_concurrent_reads: usize,
    pub max_concurrent_writes: usize,
    /// Upper bound per request; servers may lower it via `limits@openssh.com`.
    pub max_packet_len: u32,
    pub max_write_packet_len: u32,
    pub request_timeout_secs: u64,
}

impl Default for SftpOptions {
    fn default() -> Self {
        Self {
            max_concurrent_reads: 64,
            max_concurrent_writes: 64,
            max_packet_len: 256 * 1024,
            max_write_packet_len: 64 * 1024,
            request_timeout_secs: 30,
        }
    }
}

/// SFTP over a channel of an existing [`Connection`] (no second login).
pub struct SftpFs {
    session: SftpSession,
}

impl SftpFs {
    pub async fn open(conn: &Connection, opts: &SftpOptions) -> Result<Self, FsError> {
        let stream = conn.open_subsystem("sftp").await?;
        let cfg = Config {
            max_packet_len: opts.max_packet_len,
            max_concurrent_reads: opts.max_concurrent_reads,
            max_concurrent_writes: opts.max_concurrent_writes,
            max_write_packet_len: opts.max_write_packet_len,
            request_timeout_secs: opts.request_timeout_secs,
        };
        let session = SftpSession::new_with_config(stream, cfg)
            .await
            .map_err(|e| map_err("<init>", e))?;
        Ok(Self { session })
    }

    pub async fn close(&self) {
        let _ = self.session.close().await;
    }
}

fn map_err(path: &str, e: SftpError) -> FsError {
    match e {
        SftpError::Status(s) => match s.status_code {
            StatusCode::NoSuchFile => FsError::NotFound(path.to_owned()),
            StatusCode::PermissionDenied => FsError::PermissionDenied(path.to_owned()),
            _ => FsError::Remote {
                path: path.to_owned(),
                message: if s.error_message.is_empty() {
                    s.status_code.to_string()
                } else {
                    s.error_message
                },
            },
        },
        SftpError::Timeout => FsError::Timeout(path.to_owned()),
        other => FsError::Remote {
            path: path.to_owned(),
            message: other.to_string(),
        },
    }
}

fn to_entry(name: String, path: String, attrs: &FileAttributes) -> Entry {
    let kind = match attrs.file_type() {
        FileType::Dir => EntryKind::Dir,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Other => EntryKind::Other,
    };
    Entry {
        name,
        path,
        kind,
        size: attrs.size.unwrap_or(0),
        modified: attrs.mtime.map(u64::from),
        modified_nanos: None,
        permissions: attrs.permissions.map(|p| p & 0o7777),
        owner: attrs.user.clone(),
        group: attrs.group.clone(),
    }
}

fn file_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_owned()
}

#[async_trait]
impl RemoteFs for SftpFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<Entry>, FsError> {
        let rd = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| map_err(path, e))?;
        let mut out: Vec<Entry> = rd
            .filter(|e| {
                let n = e.file_name();
                n != "." && n != ".."
            })
            .map(|e| {
                let name = e.file_name();
                to_entry(name.clone(), join(path, &name), &e.metadata())
            })
            .collect();
        sort_entries(&mut out);
        Ok(out)
    }

    async fn stat(&self, path: &str) -> Result<Option<Entry>, FsError> {
        match self.session.metadata(path).await {
            Ok(attrs) => Ok(Some(to_entry(file_name(path), path.to_owned(), &attrs))),
            Err(e) => match map_err(path, e) {
                FsError::NotFound(_) => Ok(None),
                other => Err(other),
            },
        }
    }

    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        self.session
            .create_dir(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        self.session
            .remove_file(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        self.session
            .remove_dir(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        self.session
            .rename(from, to)
            .await
            .map_err(|e| map_err(from, e))
    }

    async fn open_read(&self, path: &str, offset: u64) -> Result<BoxRead, FsError> {
        let mut f = self
            .session
            .open(path)
            .await
            .map_err(|e| map_err(path, e))?;
        if offset > 0 {
            f.seek(SeekFrom::Start(offset)).await?;
        }
        Ok(Box::pin(f))
    }

    async fn open_write(&self, path: &str, offset: u64) -> Result<BoxWrite, FsError> {
        let flags = if offset == 0 {
            OpenFlags::CREATE | OpenFlags::WRITE | OpenFlags::TRUNCATE
        } else {
            OpenFlags::CREATE | OpenFlags::WRITE
        };
        let mut f = self
            .session
            .open_with_flags(path, flags)
            .await
            .map_err(|e| map_err(path, e))?;
        if offset > 0 {
            f.seek(SeekFrom::Start(offset)).await?;
        }
        Ok(Box::pin(f))
    }

    async fn set_modified(&self, path: &str, unix_secs: u64) -> Result<(), FsError> {
        let secs = u32::try_from(unix_secs).unwrap_or(u32::MAX);
        let mut attrs = FileAttributes::empty();
        attrs.mtime = Some(secs);
        attrs.atime = Some(secs);
        self.session
            .set_metadata(path, attrs)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn canonicalize(&self, path: &str) -> Result<String, FsError> {
        self.session
            .canonicalize(path)
            .await
            .map_err(|e| map_err(path, e))
    }
}
