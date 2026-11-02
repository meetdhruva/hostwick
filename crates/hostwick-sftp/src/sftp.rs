use std::io::SeekFrom;

use async_trait::async_trait;
use hostwick_core::{SecretString, SudoOptions};
use hostwick_ssh::Connection;
use russh_sftp::client::{Config, SftpSession, error::Error as SftpError};
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use tokio::io::{AsyncRead, AsyncSeekExt, AsyncWrite};

use crate::sudo::{self, SudoCommand};
use crate::{
    BoxRead, BoxWrite, DiskUsage, Entry, EntryKind, FsError, HashAlgo, RemoteFs, join, remote_cmd,
    sort_entries,
};

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
    /// For server-side helpers (checksums, folder sizes, user names).
    conn: Option<Connection>,
    /// `Some(user)` when running through sudo.
    sudo_user: Option<String>,
    /// OpenSSH's server swaps the SYMLINK arguments; detected on first use.
    openssh: tokio::sync::OnceCell<bool>,
}

fn config(opts: &SftpOptions) -> Config {
    Config {
        max_packet_len: opts.max_packet_len,
        max_concurrent_reads: opts.max_concurrent_reads,
        max_concurrent_writes: opts.max_concurrent_writes,
        max_write_packet_len: opts.max_write_packet_len,
        request_timeout_secs: opts.request_timeout_secs,
    }
}

impl SftpFs {
    /// The server's `sftp` subsystem. Fails quickly with
    /// [`hostwick_ssh::SshError::ChannelRefused`] when there is none.
    pub async fn open(conn: &Connection, opts: &SftpOptions) -> Result<Self, FsError> {
        let stream = conn.open_subsystem_checked("sftp").await?;
        let session = SftpSession::new_with_config(stream, config(opts))
            .await
            .map_err(|e| map_err("<init>", e))?;
        Ok(Self::with_session(session, Some(conn.clone()), None))
    }

    /// SFTP over any byte stream already speaking the protocol.
    pub async fn from_stream<S>(stream: S, opts: &SftpOptions) -> Result<Self, FsError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let session = SftpSession::new_with_config(stream, config(opts))
            .await
            .map_err(|e| map_err("<init>", e))?;
        Ok(Self::with_session(session, None, None))
    }

    fn with_session(
        session: SftpSession,
        conn: Option<Connection>,
        sudo_user: Option<String>,
    ) -> Self {
        Self {
            session,
            conn,
            sudo_user,
            openssh: tokio::sync::OnceCell::new(),
        }
    }

    /// SFTP as another user: the SFTP server program runs under
    /// `sudo -u <user>` (root when `sudo.user` is empty). Works with
    /// passwordless sudo, and with a password when one is given.
    ///
    /// Errors worth handling: [`crate::SudoError::PasswordRequired`] (ask
    /// for the password and call again) and
    /// [`crate::SudoError::WrongPassword`] (ask again).
    pub async fn open_sudo(
        conn: &Connection,
        sudo: &SudoOptions,
        password: Option<SecretString>,
    ) -> Result<Self, FsError> {
        Self::open_sudo_with_options(conn, sudo, password, &SftpOptions::default()).await
    }

    pub async fn open_sudo_with_options(
        conn: &Connection,
        sudo_opts: &SudoOptions,
        password: Option<SecretString>,
        opts: &SftpOptions,
    ) -> Result<Self, FsError> {
        let server = sudo::detect_sftp_server(conn, &sudo_opts.sftp_server).await?;
        let cmd = SudoCommand::new(sudo_opts, server.clone());
        match sudo::probe(conn, &cmd).await? {
            sudo::Probe::Passwordless => {}
            sudo::Probe::NeedsPassword => match &password {
                None => return Err(sudo::SudoError::PasswordRequired.into()),
                Some(p) => sudo::validate(conn, &cmd, p).await?,
            },
        }
        let (stream, marker) = sudo::start(conn, &cmd, password.as_ref()).await?;
        let (stdio, mut stderr, mut exit) = stream.into_parts();
        let init = SftpSession::new_with_config(stdio, config(opts));
        let session = tokio::select! {
            r = init => match r {
                Ok(s) => s,
                Err(e) => {
                    let why = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        sudo::watch_failure(&mut stderr, &mut exit, marker.as_deref(), &server),
                    )
                    .await;
                    return Err(match why {
                        Ok(sudo_err) => sudo_err.into(),
                        Err(_) => map_err("<sudo sftp init>", e),
                    });
                }
            },
            e = sudo::watch_failure(&mut stderr, &mut exit, marker.as_deref(), &server) => {
                return Err(e.into());
            }
        };
        tracing::info!(user = cmd.user(), %server, "SFTP via sudo");
        Ok(Self::with_session(
            session,
            Some(conn.clone()),
            Some(cmd.user().to_owned()),
        ))
    }

    /// The user the session runs as through sudo, if any.
    pub fn sudo_user(&self) -> Option<&str> {
        self.sudo_user.as_deref()
    }

    pub async fn close(&self) {
        let _ = self.session.close().await;
    }

    /// Server-side helpers run as the login user, so they are only used when
    /// the session is not elevated (root may read what the user cannot).
    fn helper_conn(&self) -> Option<&Connection> {
        self.conn.as_ref().filter(|_| self.sudo_user.is_none())
    }

    async fn is_openssh(&self) -> bool {
        *self
            .openssh
            .get_or_init(|| async {
                // OpenSSH's server is the one implementing statvfs@openssh.com.
                self.session
                    .fs_info(".".to_owned())
                    .await
                    .ok()
                    .flatten()
                    .is_some()
            })
            .await
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
        uid: attrs.uid,
        gid: attrs.gid,
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

    fn protocol(&self) -> String {
        match &self.sudo_user {
            Some(u) => format!("SFTP (sudo {u})"),
            None => "SFTP".into(),
        }
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> Result<(), FsError> {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(mode & 0o7777);
        self.session
            .set_metadata(path, attrs)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn set_owner(
        &self,
        path: &str,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<(), FsError> {
        if uid.is_none() && gid.is_none() {
            return Ok(());
        }
        // SFTP v3 sets uid and gid together: fill in the one we keep.
        let (uid, gid) = match (uid, gid) {
            (Some(u), Some(g)) => (u, g),
            _ => {
                let cur = self
                    .session
                    .symlink_metadata(path)
                    .await
                    .map_err(|e| map_err(path, e))?;
                match (uid.or(cur.uid), gid.or(cur.gid)) {
                    (Some(u), Some(g)) => (u, g),
                    _ => {
                        return Err(FsError::Unsupported(format!(
                            "{path}: the server does not report owners"
                        )));
                    }
                }
            }
        };
        let mut attrs = FileAttributes::empty();
        attrs.uid = Some(uid);
        attrs.gid = Some(gid);
        self.session
            .set_metadata(path, attrs)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn symlink(&self, target: &str, link: &str) -> Result<(), FsError> {
        let res = if self.is_openssh().await {
            // OpenSSH: (target, link), against the specification's order.
            self.session
                .symlink(target.to_owned(), link.to_owned())
                .await
        } else {
            self.session
                .symlink(link.to_owned(), target.to_owned())
                .await
        };
        res.map_err(|e| map_err(link, e))
    }

    async fn read_link(&self, path: &str) -> Result<String, FsError> {
        self.session
            .read_link(path)
            .await
            .map_err(|e| map_err(path, e))
    }

    async fn lookup_ids(
        &self,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> (Option<String>, Option<String>) {
        match &self.conn {
            Some(c) => remote_cmd::lookup_ids(c, uid, gid).await,
            None => (None, None),
        }
    }

    async fn checksum(&self, path: &str, algo: HashAlgo) -> Result<String, FsError> {
        if let Some(c) = self.helper_conn()
            && let Some(d) = remote_cmd::checksum(c, path, algo).await
        {
            return Ok(d);
        }
        crate::stream_checksum(self, path, algo).await
    }

    async fn disk_usage(&self, path: &str) -> Result<DiskUsage, FsError> {
        if let Some(c) = self.helper_conn()
            && let Some(u) = remote_cmd::disk_usage(c, path).await
        {
            return Ok(u);
        }
        crate::walk_disk_usage(self, path).await
    }
}
