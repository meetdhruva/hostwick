use std::path::Path;

use async_trait::async_trait;
use hostwick_core::SecretString;

/// What `known_hosts` says about a presented host key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Matches a recorded key.
    Known,
    /// First connection to this host (no key of this type recorded).
    Unknown,
    /// A different key of the same type is recorded: possible MITM.
    Changed { line: usize },
}

#[derive(Debug, Clone)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    /// `SHA256:...`, as printed by OpenSSH.
    pub fingerprint: String,
    pub status: HostKeyStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyDecision {
    Reject,
    /// Trust for this connection only.
    AcceptOnce,
    /// Trust and record in known_hosts (replacing a changed key).
    AcceptAndRemember,
}

/// Everything the SSH layer may need to ask the person. The CLI implements it
/// with terminal prompts; the desktop app with dialogs.
#[async_trait]
pub trait Interaction: Send + Sync {
    /// Only called for `Unknown` or `Changed` keys; `Known` keys pass silently.
    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision;
    async fn password(&self, user: &str, host: &str) -> Option<SecretString>;
    async fn key_passphrase(&self, path: &Path) -> Option<SecretString>;
    /// `prompts` are `(text, echo)`. Return one answer per prompt.
    async fn keyboard_interactive(
        &self,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Vec<String>>;
    /// Pre-auth banner from the server (e.g. legal notice).
    fn banner(&self, _text: &str) {}
    /// Non-fatal problem worth showing (e.g. "key x.ppk: wrong passphrase").
    fn notice(&self, _text: &str) {}
}

/// Lookup of stored secrets (the vault) by credential id.
pub trait Credentials: Send + Sync {
    fn get(&self, credential_id: &str) -> Option<SecretString>;
}

/// No stored credentials: everything comes from prompts.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCredentials;

impl Credentials for NoCredentials {
    fn get(&self, _: &str) -> Option<SecretString> {
        None
    }
}
