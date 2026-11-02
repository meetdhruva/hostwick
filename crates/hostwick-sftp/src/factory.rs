//! Picking the file protocol for a host.

use std::future::Future;
use std::sync::Arc;

use hostwick_core::{FileProtocol, HostProfile, SecretString, SudoOptions};
use hostwick_ssh::{Connection, SshError};

use crate::{FsError, RemoteFs, ScpFs, SftpFs, SftpOptions, SudoError};

/// Wrong passwords accepted before giving up.
const SUDO_ATTEMPTS: usize = 3;

/// The file system for `profile` over `conn`: its file protocol (SFTP, SCP,
/// or SFTP falling back to SCP when the server has no SFTP subsystem), run
/// through sudo when `profile.sudo.enabled`.
///
/// `sudo_password` is used only if sudo asks for one; without it a password
/// prompt from sudo becomes [`SudoError::PasswordRequired`].
pub async fn open_remote_fs(
    conn: &Connection,
    profile: &HostProfile,
    sudo_password: Option<SecretString>,
) -> Result<Arc<dyn RemoteFs>, FsError> {
    let sudo = profile.sudo.enabled.then_some(&profile.sudo);
    let mut pw = sudo_password;
    open_remote_fs_with(conn, profile.file_protocol, sudo, move |_| {
        let p = pw.take();
        async move { p }
    })
    .await
}

/// Like [`open_remote_fs`] with the elevation chosen by the caller
/// (`sudo: Some` = elevated) and passwords asked for on demand: `ask` gets
/// `None` the first time and the previous error after a wrong password, and
/// returns `None` to give up. It is never called for passwordless sudo.
pub async fn open_remote_fs_with<F, Fut>(
    conn: &Connection,
    protocol: FileProtocol,
    sudo: Option<&SudoOptions>,
    mut ask: F,
) -> Result<Arc<dyn RemoteFs>, FsError>
where
    F: FnMut(Option<SudoError>) -> Fut,
    Fut: Future<Output = Option<SecretString>>,
{
    let opts = SftpOptions::default();
    if let Some(sudo) = sudo {
        if protocol == FileProtocol::Scp {
            return Err(FsError::Unsupported(
                "sudo needs the SFTP server program; switch the host's file protocol to SFTP or Auto"
                    .into(),
            ));
        }
        let mut password: Option<SecretString> = None;
        let mut last: Option<SudoError> = None;
        let mut attempts = 0;
        loop {
            match SftpFs::open_sudo_with_options(conn, sudo, password.clone(), &opts).await {
                Ok(fs) => return Ok(Arc::new(fs)),
                Err(FsError::Sudo(
                    e @ (SudoError::PasswordRequired | SudoError::WrongPassword),
                )) if attempts < SUDO_ATTEMPTS => {
                    if e == SudoError::WrongPassword {
                        attempts += 1;
                        if attempts >= SUDO_ATTEMPTS {
                            return Err(e.into());
                        }
                        last = Some(e);
                    }
                    let prev = last.take();
                    match ask(prev.clone()).await {
                        Some(p) => password = Some(p),
                        None => return Err(prev.unwrap_or(SudoError::PasswordRequired).into()),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
    match protocol {
        FileProtocol::Sftp => Ok(Arc::new(SftpFs::open(conn, &opts).await?)),
        FileProtocol::Scp => Ok(Arc::new(ScpFs::open(conn).await?)),
        FileProtocol::Auto => match SftpFs::open(conn, &opts).await {
            Ok(fs) => Ok(Arc::new(fs)),
            Err(FsError::Ssh(SshError::ChannelRefused(why))) => {
                tracing::info!(%why, "no SFTP subsystem; falling back to SCP");
                match ScpFs::open(conn).await {
                    Ok(fs) => Ok(Arc::new(fs)),
                    Err(FsError::Unsupported(_)) => Err(FsError::Unsupported(format!(
                        "{why}, and scp is not available either"
                    ))),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        },
    }
}
