use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg};
use tokio::sync::mpsc;

use crate::SshError;
use crate::handler::ClientHandler;
use crate::x11::X11Forwarding;

/// What to forward over a session channel (`ssh -A`, `ssh -X`).
#[derive(Clone, Default)]
pub(crate) struct SessionForwarding {
    pub agent: bool,
    pub x11: Option<Arc<X11Forwarding>>,
}

/// How long to wait for the server to answer the X11 request.
const X11_REPLY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct PtyOptions {
    pub term: String,
    pub cols: u32,
    pub rows: u32,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self {
            term: "xterm-256color".into(),
            cols: 80,
            rows: 24,
        }
    }
}

/// Input to a running shell or command.
#[derive(Debug)]
pub enum ShellCommand {
    Data(Bytes),
    Resize {
        cols: u32,
        rows: u32,
    },
    /// Close stdin (EOF), e.g. after piping a file into `exec`.
    Eof,
    Close,
}

/// Output from a running shell or command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Stdout(Bytes),
    Stderr(Bytes),
    Exit(u32),
    /// Killed by a signal (e.g. `TERM`).
    ExitSignal(String),
    Closed,
}

/// A session channel running a shell or command, driven by its own task.
///
/// Both queues are bounded. If the consumer stops reading output, the task
/// stops reading the channel, the SSH window fills, and the server pauses the
/// remote process: end-to-end backpressure with no unbounded buffering.
///
/// Dropping the session (or every input sender) closes the channel; the pump
/// task then exits once the server confirms the close.
pub struct ShellSession {
    input: mpsc::Sender<ShellCommand>,
    output: mpsc::Receiver<ShellEvent>,
}

const QUEUE: usize = 256;

impl ShellSession {
    pub(crate) async fn open(
        handle: &Handle<ClientHandler>,
        pty: Option<PtyOptions>,
        command: Option<&str>,
        fwd: &SessionForwarding,
    ) -> Result<Self, SshError> {
        let mut channel = handle.channel_open_session().await?;
        if fwd.agent {
            channel.agent_forward(false).await?;
        }
        if let Some(x11) = &fwd.x11 {
            request_x11(&mut channel, x11).await?;
        }
        if let Some(pty) = &pty {
            channel
                .request_pty(true, &pty.term, pty.cols, pty.rows, 0, 0, &[])
                .await?;
        }
        match command {
            Some(cmd) => channel.exec(true, cmd).await?,
            None => channel.request_shell(true).await?,
        }

        let (in_tx, mut in_rx) = mpsc::channel::<ShellCommand>(QUEUE);
        let (out_tx, out_rx) = mpsc::channel::<ShellEvent>(QUEUE);
        let (mut read, write) = channel.split();

        tokio::spawn(async move {
            let mut input_open = true;
            loop {
                tokio::select! {
                    cmd = in_rx.recv(), if input_open => {
                        let res = match cmd {
                            Some(ShellCommand::Data(b)) => write.data_bytes(b).await,
                            Some(ShellCommand::Resize { cols, rows }) => write.window_change(cols, rows, 0, 0).await,
                            Some(ShellCommand::Eof) => write.eof().await,
                            Some(ShellCommand::Close) | None => {
                                // All senders gone or explicit close: hang up.
                                input_open = false;
                                write.close().await
                            }
                        };
                        if let Err(e) = res {
                            tracing::debug!(error = %e, "shell write failed");
                        }
                    }
                    msg = read.wait() => {
                        let event = match msg {
                            Some(ChannelMsg::Data { data }) => ShellEvent::Stdout(data),
                            Some(ChannelMsg::ExtendedData { data, .. }) => ShellEvent::Stderr(data),
                            Some(ChannelMsg::ExitStatus { exit_status }) => ShellEvent::Exit(exit_status),
                            Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                                ShellEvent::ExitSignal(format!("{signal_name:?}"))
                            }
                            Some(ChannelMsg::Close) | None => {
                                let _ = out_tx.send(ShellEvent::Closed).await;
                                break;
                            }
                            Some(_) => continue,
                        };
                        if out_tx.send(event).await.is_err() {
                            // Consumer gone: close the channel.
                            let _ = write.close().await;
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            input: in_tx,
            output: out_rx,
        })
    }

    /// A handle for sending input; clone it freely (e.g. broadcast input).
    pub fn input(&self) -> mpsc::Sender<ShellCommand> {
        self.input.clone()
    }

    pub async fn recv(&mut self) -> Option<ShellEvent> {
        self.output.recv().await
    }

    pub async fn send(&self, cmd: ShellCommand) -> Result<(), SshError> {
        self.input.send(cmd).await.map_err(|_| SshError::Closed)
    }

    /// Split into the input sender and output receiver, for UIs that pump
    /// them from different tasks.
    pub fn split(self) -> (mpsc::Sender<ShellCommand>, mpsc::Receiver<ShellEvent>) {
        (self.input, self.output)
    }
}

/// Ask for X11 forwarding and wait for the answer, so a refusal is reported
/// (like OpenSSH's "X11 forwarding request failed") instead of silently
/// leaving GUI programs without a display. Never fails the session.
async fn request_x11(channel: &mut Channel<Msg>, x11: &X11Forwarding) -> Result<(), SshError> {
    let Some((proto, cookie, screen)) = x11.request_params().await else {
        return Ok(());
    };
    channel
        .request_x11(true, false, proto, cookie, screen)
        .await?;
    let answer = tokio::time::timeout(X11_REPLY_TIMEOUT, async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => return Some(true),
                Some(ChannelMsg::Failure) => return Some(false),
                Some(ChannelMsg::Close) | None => return None,
                Some(_) => {}
            }
        }
    })
    .await;
    match answer {
        Ok(Some(true)) => tracing::debug!("X11 forwarding enabled"),
        Ok(Some(false)) => x11.report_refused(),
        Ok(None) => return Err(SshError::Closed),
        Err(_) => tracing::warn!("no answer to the X11 forwarding request; continuing"),
    }
    Ok(())
}
