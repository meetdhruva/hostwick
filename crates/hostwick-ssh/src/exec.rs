use crate::SshError;
use crate::shell::{ShellEvent, ShellSession};

/// Collected result of a non-interactive command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `None` if the server closed the channel without reporting a status
    /// (e.g. the command was killed by a signal).
    pub exit_status: Option<u32>,
    pub exit_signal: Option<String>,
}

impl ExecOutput {
    pub(crate) async fn collect(mut session: ShellSession) -> Result<Self, SshError> {
        let mut out = Self::default();
        // No stdin for collected commands.
        session.send(crate::ShellCommand::Eof).await.ok();
        while let Some(ev) = session.recv().await {
            match ev {
                ShellEvent::Stdout(b) => out.stdout.extend_from_slice(&b),
                ShellEvent::Stderr(b) => out.stderr.extend_from_slice(&b),
                ShellEvent::Exit(code) => out.exit_status = Some(code),
                ShellEvent::ExitSignal(sig) => out.exit_signal = Some(sig),
                ShellEvent::Closed => break,
            }
        }
        Ok(out)
    }

    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}
