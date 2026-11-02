//! Encrypted credential vault.
//!
//! One file, JSON header + ciphertext:
//! - master password → Argon2id (64 MiB, t=3) → 256-bit key
//! - secrets map → XChaCha20-Poly1305, random 192-bit nonce per save
//! - the header (version + KDF params) is authenticated as AAD, so tampering
//!   with parameters is detected, not silently "decrypted"
//!
//! Plaintext only lives in zeroizing buffers and `SecretString`s.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hostwick_core::{ExposeSecret, SecretString};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("wrong master password, or the vault file was modified")]
    WrongPassword,
    #[error("vault already exists at {0}")]
    AlreadyExists(PathBuf),
    #[error("no vault at {0}; create one first")]
    Missing(PathBuf),
    #[error("vault file is corrupt: {0}")]
    Corrupt(String),
    #[error("unsupported vault version {0}")]
    UnsupportedVersion(u32),
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Argon2id cost parameters, stored in the file so they can be raised later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        // OWASP-recommended Argon2id baseline; ~0.3–0.6 s on a laptop.
        Self {
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    kdf: String,
    params: KdfParams,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct FileFormat {
    #[serde(flatten)]
    header: Header,
    nonce: String,
    ciphertext: String,
}

pub struct Vault {
    path: PathBuf,
    params: KdfParams,
    salt: [u8; 16],
    key: Zeroizing<[u8; 32]>,
    entries: BTreeMap<String, SecretString>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// Decrypted id → secret map, wiped when dropped.
struct PlainMap(BTreeMap<String, String>);

impl Drop for PlainMap {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for v in self.0.values_mut() {
            v.zeroize();
        }
    }
}

fn derive(
    master: &SecretString,
    salt: &[u8],
    p: KdfParams,
) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let params = Params::new(p.m_cost_kib, p.t_cost, p.p_cost, Some(32))
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(master.expose_secret().as_bytes(), salt, key.as_mut())
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    Ok(key)
}

impl Vault {
    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    pub fn create(path: impl Into<PathBuf>, master: &SecretString) -> Result<Self, VaultError> {
        Self::create_with_params(path, master, KdfParams::default())
    }

    pub fn create_with_params(
        path: impl Into<PathBuf>,
        master: &SecretString,
        params: KdfParams,
    ) -> Result<Self, VaultError> {
        let path = path.into();
        if path.exists() {
            return Err(VaultError::AlreadyExists(path));
        }
        let mut salt = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let key = derive(master, &salt, params)?;
        let v = Self {
            path,
            params,
            salt,
            key,
            entries: BTreeMap::new(),
        };
        v.save()?;
        Ok(v)
    }

    pub fn unlock(path: impl Into<PathBuf>, master: &SecretString) -> Result<Self, VaultError> {
        let path = path.into();
        if !path.exists() {
            return Err(VaultError::Missing(path));
        }
        let text = std::fs::read_to_string(&path)?;
        let file: FileFormat =
            serde_json::from_str(&text).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if file.header.version != VERSION {
            return Err(VaultError::UnsupportedVersion(file.header.version));
        }
        if file.header.kdf != "argon2id" {
            return Err(VaultError::Corrupt(format!(
                "unknown kdf {}",
                file.header.kdf
            )));
        }
        let salt: [u8; 16] = B64
            .decode(&file.header.salt)
            .ok()
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| VaultError::Corrupt("bad salt".into()))?;
        let nonce = B64
            .decode(&file.nonce)
            .map_err(|e| VaultError::Corrupt(e.to_string()))?;
        if nonce.len() != 24 {
            return Err(VaultError::Corrupt("bad nonce".into()));
        }
        let ct = B64
            .decode(&file.ciphertext)
            .map_err(|e| VaultError::Corrupt(e.to_string()))?;
        let key = derive(master, &salt, file.header.params)?;
        let aad =
            serde_json::to_vec(&file.header).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        let cipher = XChaCha20Poly1305::new(key.as_ref().into());
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ct,
                        aad: &aad,
                    },
                )
                .map_err(|_| VaultError::WrongPassword)?,
        );
        let raw = PlainMap(
            serde_json::from_slice(&plain).map_err(|e| VaultError::Corrupt(e.to_string()))?,
        );
        let entries = raw
            .0
            .iter()
            .map(|(k, v)| (k.clone(), SecretString::from(v.clone())))
            .collect();
        Ok(Self {
            path,
            params: file.header.params,
            salt,
            key,
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, id: &str) -> Option<SecretString> {
        self.entries.get(id).cloned()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Set and persist.
    pub fn set(&mut self, id: impl Into<String>, secret: SecretString) -> Result<(), VaultError> {
        self.entries.insert(id.into(), secret);
        self.save()
    }

    /// Remove and persist. Returns whether it existed.
    pub fn remove(&mut self, id: &str) -> Result<bool, VaultError> {
        let existed = self.entries.remove(id).is_some();
        if existed {
            self.save()?;
        }
        Ok(existed)
    }

    /// Re-encrypt under a new master password (new salt).
    pub fn change_master(&mut self, new_master: &SecretString) -> Result<(), VaultError> {
        rand::rngs::OsRng.fill_bytes(&mut self.salt);
        self.key = derive(new_master, &self.salt, self.params)?;
        self.save()
    }

    fn save(&self) -> Result<(), VaultError> {
        let header = Header {
            version: VERSION,
            kdf: "argon2id".into(),
            params: self.params,
            salt: B64.encode(self.salt),
        };
        let aad = serde_json::to_vec(&header).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        let plain = PlainMap(
            self.entries
                .iter()
                .map(|(k, v)| (k.clone(), v.expose_secret().to_owned()))
                .collect(),
        );
        let plain_bytes = Zeroizing::new(
            serde_json::to_vec(&plain.0).map_err(|e| VaultError::Corrupt(e.to_string()))?,
        );
        let mut nonce = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        let ct = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain_bytes,
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::Corrupt("encryption failed".into()))?;
        let file = FileFormat {
            header,
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ct),
        };
        let json =
            serde_json::to_vec_pretty(&file).map_err(|e| VaultError::Corrupt(e.to_string()))?;

        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Atomic replace: a crash mid-save never corrupts the vault.
        let tmp = self.path.with_extension("tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        restrict_permissions(&tmp)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_: &Path) -> std::io::Result<()> {
    // The file lives under the user's profile directory, which is already
    // private to them on Windows.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams {
        m_cost_kib: 1024,
        t_cost: 1,
        p_cost: 1,
    };

    fn s(v: &str) -> SecretString {
        SecretString::from(v.to_owned())
    }

    #[test]
    fn round_trip_wrong_password_and_rekey() -> Result<(), VaultError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("vault.json");
        let mut v = Vault::create_with_params(&path, &s("correct horse"), FAST)?;
        v.set("prod-db", s("hunter2"))?;
        v.set("key-pass", s("p@ss"))?;
        assert!(matches!(
            Vault::create_with_params(&path, &s("x"), FAST),
            Err(VaultError::AlreadyExists(_))
        ));

        let raw = std::fs::read_to_string(&path)?;
        assert!(!raw.contains("hunter2"), "plaintext leaked to disk");

        let v2 = Vault::unlock(&path, &s("correct horse"))?;
        assert_eq!(
            v2.get("prod-db")
                .map(|x| x.expose_secret().to_owned())
                .as_deref(),
            Some("hunter2")
        );
        assert_eq!(v2.ids().collect::<Vec<_>>(), vec!["key-pass", "prod-db"]);

        assert!(matches!(
            Vault::unlock(&path, &s("wrong")),
            Err(VaultError::WrongPassword)
        ));

        let mut v3 = Vault::unlock(&path, &s("correct horse"))?;
        v3.change_master(&s("new master"))?;
        assert!(matches!(
            Vault::unlock(&path, &s("correct horse")),
            Err(VaultError::WrongPassword)
        ));
        let mut v4 = Vault::unlock(&path, &s("new master"))?;
        assert!(v4.remove("prod-db")?);
        assert!(!v4.remove("prod-db")?);
        Ok(())
    }

    #[test]
    fn tampered_header_is_rejected() -> Result<(), VaultError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("vault.json");
        let mut v = Vault::create_with_params(&path, &s("pw"), FAST)?;
        v.set("a", s("b"))?;
        // Weaken the KDF params in the file: AAD check must catch it.
        let raw = std::fs::read_to_string(&path)?.replace("\"t_cost\": 1", "\"t_cost\": 2");
        std::fs::write(&path, raw)?;
        assert!(matches!(
            Vault::unlock(&path, &s("pw")),
            Err(VaultError::WrongPassword)
        ));
        Ok(())
    }
}
