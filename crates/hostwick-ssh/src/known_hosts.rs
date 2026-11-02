use std::io::Write;
use std::path::{Path, PathBuf};

use russh::keys::{HashAlg, PublicKey};

use crate::SshError;
use crate::interaction::HostKeyStatus;

/// OpenSSH-format `known_hosts` file. Hostwick shares `~/.ssh/known_hosts`
/// with OpenSSH by default, so trust decisions carry over in both directions.
#[derive(Debug, Clone)]
pub struct KnownHosts {
    path: PathBuf,
}

impl KnownHosts {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `~/.ssh/known_hosts`.
    pub fn openssh_default() -> Option<Self> {
        dirs::home_dir().map(|h| Self::new(h.join(".ssh").join("known_hosts")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus, SshError> {
        if !self.path.exists() {
            return Ok(HostKeyStatus::Unknown);
        }
        match russh::keys::check_known_hosts_path(host, port, key, &self.path) {
            Ok(true) => Ok(HostKeyStatus::Known),
            Ok(false) => Ok(HostKeyStatus::Unknown),
            Err(russh::keys::Error::KeyChanged { line }) => Ok(HostKeyStatus::Changed { line }),
            Err(e) => Err(SshError::KnownHosts(e.to_string())),
        }
    }

    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), SshError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        russh::keys::known_hosts::learn_known_hosts_path(host, port, key, &self.path)
            .map_err(|e| SshError::KnownHosts(e.to_string()))
    }

    /// Replace the key recorded on `line` (1-based, as reported by
    /// [`HostKeyStatus::Changed`]) with `key`.
    pub fn replace(
        &self,
        line: usize,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<(), SshError> {
        let text = std::fs::read_to_string(&self.path)?;
        let kept: Vec<&str> = text
            .lines()
            .enumerate()
            .filter(|(i, _)| i + 1 != line)
            .map(|(_, l)| l)
            .collect();
        // Atomic rewrite: temp file in the same directory, then rename.
        let tmp = self.path.with_extension("hostwick-tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            for l in kept {
                writeln!(f, "{l}")?;
            }
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        self.learn(host, port, key)
    }
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b64: &str) -> PublicKey {
        russh::keys::parse_public_key_base64(b64).unwrap_or_else(|e| panic!("{e}"))
    }

    // Two arbitrary, valid ed25519 public keys.
    const K1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAvL1GGwODl262OAAcFSein2LSIyeWeK4bCI913E0PMB";
    const K2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIIdL8bVLpSDTnT06ap4qNlK/6WoKb3qqsRlFQednSzHA";

    #[test]
    fn tofu_learn_then_detect_change_then_replace() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let kh = KnownHosts::new(dir.path().join("known_hosts"));
        let (k1, k2) = (key(K1), key(K2));

        assert_eq!(kh.check("srv", 22, &k1).ok(), Some(HostKeyStatus::Unknown));
        kh.learn("srv", 22, &k1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(kh.check("srv", 22, &k1).ok(), Some(HostKeyStatus::Known));
        // Non-default port is a different host entry.
        assert_eq!(
            kh.check("srv", 2222, &k1).ok(),
            Some(HostKeyStatus::Unknown)
        );

        let status = kh.check("srv", 22, &k2).unwrap_or_else(|e| panic!("{e}"));
        let HostKeyStatus::Changed { line } = status else {
            panic!("expected Changed, got {status:?}")
        };
        kh.replace(line, "srv", 22, &k2)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(kh.check("srv", 22, &k2).ok(), Some(HostKeyStatus::Known));
        assert!(matches!(
            kh.check("srv", 22, &k1),
            Ok(HostKeyStatus::Changed { .. })
        ));
    }

    #[test]
    fn fingerprint_is_openssh_style() {
        assert!(fingerprint(&key(K1)).starts_with("SHA256:"));
    }
}
