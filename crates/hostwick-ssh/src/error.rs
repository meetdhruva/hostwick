use std::time::Duration;

/// Errors surfaced to the UI. Messages are written for the person reading
/// them, not for the developer.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("could not reach {host}:{port}: {reason}")]
    Connect {
        host: String,
        port: u16,
        reason: std::io::Error,
    },
    #[error("timed out after {0:?} connecting to {1}")]
    Timeout(Duration, String),
    #[error("host key for {0} was rejected")]
    HostKeyRejected(String),
    #[error("authentication failed for {user}@{host} (server allows: {allowed}). Tried: {tried}")]
    AuthFailed {
        user: String,
        host: String,
        allowed: String,
        /// What each configured method did, e.g. "key a.ppk: wrong passphrase".
        tried: String,
    },
    #[error("could not load key {path}: {reason}")]
    Key { path: String, reason: String },
    #[error("SSH agent unavailable: {0}")]
    Agent(String),
    /// The SOCKS/HTTP proxy answered but would not connect us, e.g.
    /// "proxy 10.0.0.1:1080 refused: connection not allowed by ruleset".
    #[error("proxy {proxy} {reason}")]
    Proxy { proxy: String, reason: String },
    #[error("channel request refused: {0}")]
    ChannelRefused(String),
    #[error("connection closed")]
    Closed,
    #[error("known_hosts: {0}")]
    KnownHosts(String),
    #[error("port forward {spec}: {reason}")]
    Forward { spec: String, reason: String },
    #[error("SSH protocol error: {0}")]
    Protocol(#[from] russh::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SshError {
    /// Whether retrying (after backoff) can reasonably succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Connect { .. } | Self::Timeout(..) | Self::Closed | Self::Io(_)
        ) || matches!(self, Self::Protocol(e) if matches!(e, russh::Error::Disconnect | russh::Error::SendError | russh::Error::IO(_)))
    }
}
