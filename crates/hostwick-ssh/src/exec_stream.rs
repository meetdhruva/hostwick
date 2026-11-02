//! A command's stdin/stdout as one byte stream, with stderr and the exit
//! status reported on the side.
//!
//! [`Connection::open_subsystem`] gives a raw stream for a subsystem, but
//! programs started through a command (`sudo … sftp-server`, `scp -f`) need
//! their stderr watched (password prompts, error messages) while stdout
//! carries a binary protocol. [`ExecStream`] separates the two.

use std::sync::Arc;

use bytes::Bytes;
use russh::ChannelMsg;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{Connection, SshError};

/// Buffer between the channel and the consumer, per direction. Large enough
/// that a pipelined protocol never stalls on it; the SSH window still bounds
/// what the server can send.
const PIPE: usize = 1024 * 1024;

/// A running command. `stdio` reads the command's stdout and writes its
/// stdin; shutting down the write side sends EOF. Dropping the stream sends
/// EOF and hangs up once the command has nothing more to say.
pub struct ExecStream {
    pub stdio: DuplexStream,
    /// Chunks of stderr as they arrive. Ends when the channel closes.
    pub stderr: mpsc::UnboundedReceiver<Bytes>,
    /// `None` until the command exits.
    pub exit: watch::Receiver<Option<ExitStatus>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitStatus {
    Code(u32),
    /// Killed by a signal (e.g. `TERM`).
    Signal(String),
    /// Channel closed without a status.
    Closed,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        *self == Self::Code(0)
    }
}

impl std::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code(c) => write!(f, "exit status {c}"),
            Self::Signal(s) => write!(f, "killed by signal {s}"),
            Self::Closed => f.write_str("closed without an exit status"),
        }
    }
}

impl ExecStream {
    /// Wait for the command to exit.
    pub async fn wait_exit(&mut self) -> ExitStatus {
        wait_exit(&mut self.exit).await
    }

    /// Split into the parts, for consumers that hand stdio to a protocol
    /// client and watch stderr/exit from another task.
    pub fn into_parts(
        self,
    ) -> (
        DuplexStream,
        mpsc::UnboundedReceiver<Bytes>,
        watch::Receiver<Option<ExitStatus>>,
    ) {
        (self.stdio, self.stderr, self.exit)
    }
}

/// Wait on an [`ExecStream::exit`] receiver.
pub async fn wait_exit(exit: &mut watch::Receiver<Option<ExitStatus>>) -> ExitStatus {
    loop {
        if let Some(s) = exit.borrow().clone() {
            return s;
        }
        if exit.changed().await.is_err() {
            return exit.borrow().clone().unwrap_or(ExitStatus::Closed);
        }
    }
}

impl Connection {
    /// Run `command` (no PTY, no agent or X11 forwarding) with its stdio as a
    /// byte stream.
    pub async fn open_exec_stream(&self, command: &str) -> Result<ExecStream, SshError> {
        let channel = self.handle().channel_open_session().await?;
        channel.exec(true, command).await?;
        let (mut read, write) = channel.split();
        let write = Arc::new(write);

        let (ours, theirs) = tokio::io::duplex(PIPE);
        let (mut from_user, mut to_user) = tokio::io::split(ours);
        let (err_tx, err_rx) = mpsc::unbounded_channel();
        let (exit_tx, exit_rx) = watch::channel(None);
        let (closed_tx, mut closed_rx) = oneshot::channel::<()>();

        // Channel → consumer.
        let hangup = write.clone();
        tokio::spawn(async move {
            let mut consumer_gone = false;
            loop {
                match read.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if !consumer_gone && to_user.write_all(&data).await.is_err() {
                            // Nobody is reading: stop the command.
                            consumer_gone = true;
                            let _ = hangup.close().await;
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = err_tx.send(data);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        let _ = exit_tx.send(Some(ExitStatus::Code(exit_status)));
                    }
                    Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                        let _ = exit_tx.send(Some(ExitStatus::Signal(format!("{signal_name:?}"))));
                    }
                    Some(ChannelMsg::Eof) => {
                        let _ = to_user.shutdown().await;
                    }
                    Some(ChannelMsg::Close) | None => break,
                    Some(_) => {}
                }
            }
            let _ = to_user.shutdown().await;
            exit_tx.send_if_modified(|s| {
                let unset = s.is_none();
                if unset {
                    *s = Some(ExitStatus::Closed);
                }
                unset
            });
            let _ = closed_tx.send(());
        });

        // Consumer → channel, in its own task so neither direction can block
        // the other.
        tokio::spawn(async move {
            let mut buf = vec![0u8; 32 * 1024];
            loop {
                tokio::select! {
                    n = from_user.read(&mut buf) => match n {
                        Ok(0) | Err(_) => {
                            let _ = write.eof().await;
                            break;
                        }
                        Ok(n) => {
                            if write.data_bytes(Bytes::copy_from_slice(&buf[..n])).await.is_err() {
                                break;
                            }
                        }
                    },
                    _ = &mut closed_rx => break,
                }
            }
        });

        Ok(ExecStream {
            stdio: theirs,
            stderr: err_rx,
            exit: exit_rx,
        })
    }
}

impl Connection {
    /// Like [`Connection::open_subsystem`], but waits for the server to
    /// accept the request, so a missing subsystem (no `Subsystem sftp` in
    /// sshd_config) fails at once instead of when the first request times
    /// out.
    pub async fn open_subsystem_checked(
        &self,
        name: &str,
    ) -> Result<russh::ChannelStream<russh::client::Msg>, SshError> {
        let mut ch = self.handle().channel_open_session().await?;
        ch.request_subsystem(true, name).await?;
        let answer = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                match ch.wait().await {
                    Some(ChannelMsg::Success) => return Ok(()),
                    Some(ChannelMsg::Failure) => {
                        return Err(SshError::ChannelRefused(format!(
                            "the server has no {name} subsystem"
                        )));
                    }
                    Some(ChannelMsg::Close) | Some(ChannelMsg::Eof) | None => {
                        return Err(SshError::ChannelRefused(format!(
                            "the server closed the {name} subsystem"
                        )));
                    }
                    Some(_) => {}
                }
            }
        })
        .await
        .map_err(|_| {
            SshError::ChannelRefused(format!("no answer to the {name} subsystem request"))
        })?;
        answer?;
        Ok(ch.into_stream())
    }
}
