use russh::keys::agent::client::{AgentClient, AgentStream};

use crate::SshError;

pub(crate) type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// Connect to whichever SSH agent is running.
///
/// Windows: Pageant first (PuTTY users, Git for Windows), then the built-in
/// OpenSSH agent's named pipe. Unix: `SSH_AUTH_SOCK`.
#[cfg(windows)]
pub(crate) async fn connect() -> Result<Agent, SshError> {
    if let Ok(a) = AgentClient::connect_pageant().await {
        return Ok(a.dynamic());
    }
    const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
    AgentClient::connect_named_pipe(OPENSSH_PIPE)
        .await
        .map(AgentClient::dynamic)
        .map_err(|e| {
            SshError::Agent(format!(
                "neither Pageant nor the OpenSSH agent answered ({e}); start one with `Start-Service ssh-agent` or Pageant"
            ))
        })
}

#[cfg(not(windows))]
pub(crate) async fn connect() -> Result<Agent, SshError> {
    if std::env::var_os("SSH_AUTH_SOCK").is_none_or(|s| s.is_empty()) {
        return Err(SshError::Agent("no agent running".into()));
    }
    AgentClient::connect_env()
        .await
        .map(AgentClient::dynamic)
        .map_err(|e| SshError::Agent(format!("agent not reachable ({e})")))
}
