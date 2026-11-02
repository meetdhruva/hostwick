use russh::keys::agent::client::{AgentClient, AgentStream};

use crate::SshError;

pub(crate) type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// Connect to whichever SSH agent is running.
///
/// Windows: Pageant first (PuTTY users, Git for Windows), then the built-in
/// OpenSSH agent's named pipe. Unix: `SSH_AUTH_SOCK`.
pub(crate) async fn connect() -> Result<Agent, SshError> {
    #[cfg(windows)]
    {
        if let Ok(a) = AgentClient::connect_pageant().await {
            return Ok(a.dynamic());
        }
        const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
        return AgentClient::connect_named_pipe(OPENSSH_PIPE)
            .await
            .map(AgentClient::dynamic)
            .map_err(|e| {
                SshError::Agent(format!(
                    "neither Pageant nor the OpenSSH agent answered ({e}); start one with `Start-Service ssh-agent` or Pageant"
                ))
            });
    }
    #[cfg(not(windows))]
    {
        AgentClient::connect_env()
            .await
            .map(AgentClient::dynamic)
            .map_err(|e| SshError::Agent(format!("SSH_AUTH_SOCK: {e}")))
    }
}
