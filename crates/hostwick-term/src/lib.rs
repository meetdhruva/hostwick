//! Terminal transports for Hostwick.
//!
//! Every way of reaching a terminal — an SSH shell channel, a telnet
//! connection or a local serial port — is presented as the same pair of
//! queues hostwick-ssh's [`ShellSession`] already uses: [`ShellCommand`] in,
//! [`ShellEvent`] out. A terminal UI written against [`TerminalSession`]
//! works with all three unchanged.
//!
//! Both queues are bounded, and each transport stops reading from the wire
//! while its output queue is full, so a slow consumer applies backpressure
//! all the way to the remote end instead of buffering without limit.

mod error;
pub mod serial;
pub mod telnet;

use tokio::sync::mpsc;

pub use error::TermError;
pub use hostwick_ssh::{ShellCommand, ShellEvent, ShellSession};
pub use serial::{PortInfo, list_ports};

/// Capacity of the input and output queues of telnet and serial sessions
/// (the same as SSH shell channels).
pub(crate) const QUEUE: usize = 256;

/// Which transport a session runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Ssh,
    Telnet,
    Serial,
}

impl TransportKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Telnet => "Telnet",
            Self::Serial => "Serial",
        }
    }
}

/// Out-of-band requests that are not terminal data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Send a BREAK: a line break condition on a serial port (about 250 ms),
    /// `IAC BRK` on telnet. Cisco ROMMON, Linux magic SysRq and many
    /// bootloaders listen for it.
    Break,
}

/// A running terminal session, whatever the transport.
pub struct TerminalSession {
    input: mpsc::Sender<ShellCommand>,
    output: mpsc::Receiver<ShellEvent>,
    control: Option<mpsc::Sender<Control>>,
    kind: TransportKind,
}

/// The pieces of a [`TerminalSession`], for UIs that drive them from
/// different tasks.
pub struct SessionParts {
    pub input: mpsc::Sender<ShellCommand>,
    pub output: mpsc::Receiver<ShellEvent>,
    /// Present when the transport supports [`Control`] requests.
    pub control: Option<mpsc::Sender<Control>>,
    pub kind: TransportKind,
}

impl TerminalSession {
    /// Wrap an existing pair of queues.
    pub fn new(
        input: mpsc::Sender<ShellCommand>,
        output: mpsc::Receiver<ShellEvent>,
        kind: TransportKind,
    ) -> Self {
        Self {
            input,
            output,
            control: None,
            kind,
        }
    }

    pub(crate) fn with_control(mut self, control: mpsc::Sender<Control>) -> Self {
        self.control = Some(control);
        self
    }

    pub fn kind(&self) -> TransportKind {
        self.kind
    }

    /// Whether [`Control::Break`] does anything on this transport.
    pub fn can_break(&self) -> bool {
        self.control.is_some()
    }

    /// A handle for sending input; clone it freely.
    pub fn input(&self) -> mpsc::Sender<ShellCommand> {
        self.input.clone()
    }

    pub async fn recv(&mut self) -> Option<ShellEvent> {
        self.output.recv().await
    }

    pub async fn send(&self, cmd: ShellCommand) -> Result<(), TermError> {
        self.input.send(cmd).await.map_err(|_| TermError::Closed)
    }

    /// Send an out-of-band request; an error if the transport has no such
    /// thing (SSH) or the session has ended.
    pub async fn control(&self, c: Control) -> Result<(), TermError> {
        match &self.control {
            Some(tx) => tx.send(c).await.map_err(|_| TermError::Closed),
            None => Err(TermError::Unsupported(format!(
                "{} sessions cannot send a break",
                self.kind.label()
            ))),
        }
    }

    /// Split into the input sender and output receiver (dropping control).
    pub fn split(self) -> (mpsc::Sender<ShellCommand>, mpsc::Receiver<ShellEvent>) {
        (self.input, self.output)
    }

    pub fn into_parts(self) -> SessionParts {
        SessionParts {
            input: self.input,
            output: self.output,
            control: self.control,
            kind: self.kind,
        }
    }
}

impl From<ShellSession> for TerminalSession {
    fn from(shell: ShellSession) -> Self {
        let (input, output) = shell.split();
        Self::new(input, output, TransportKind::Ssh)
    }
}

/// Bytes shown in the terminal when a transport fails, so the reason is on
/// screen next to the output that preceded it.
pub(crate) fn failure_text(message: &str) -> bytes::Bytes {
    bytes::Bytes::from(format!("\r\n[{message}]\r\n"))
}
