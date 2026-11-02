//! Passphrase-sealed blobs: the vault's construction for any payload.
//!
//! JSON envelope: header (format, version, purpose, Argon2id parameters,
//! salt) + nonce + ciphertext. The key is Argon2id(passphrase, salt); the
//! payload is XChaCha20-Poly1305 with the serialized header as associated
//! data, so changed KDF parameters, salt or purpose are detected as
//! tampering. `purpose` keeps a blob made for one feature (e.g.
//! `settings-sync`) from being accepted by another.

use argon2::Params;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hostwick_core::SecretString;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{KdfParams, VaultError, derive};

const FORMAT: &str = "hostwick-sealed";
const VERSION: u32 = 1;

/// Refuse parameters that would make opening a hostile file exhaust memory
/// or time (1 GiB, 64 passes, 16 lanes).
const MAX_M_COST_KIB: u32 = 1024 * 1024;
const MAX_T_COST: u32 = 64;
const MAX_P_COST: u32 = 16;

#[derive(Serialize, Deserialize)]
struct Header {
    format: String,
    version: u32,
    purpose: String,
    kdf: String,
    params: KdfParams,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(flatten)]
    header: Header,
    nonce: String,
    ciphertext: String,
}

/// A derived key with its salt and parameters. Keep it to re-seal without
/// paying for Argon2 again.
pub struct SealKey {
    params: KdfParams,
    salt: [u8; 16],
    key: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealKey")
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl SealKey {
    /// Derive a key for `passphrase` with a fresh random salt.
    pub fn new(passphrase: &SecretString, params: KdfParams) -> Result<Self, VaultError> {
        let mut salt = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let key = derive(passphrase, &salt, params)?;
        Ok(Self { params, salt, key })
    }

    pub fn params(&self) -> KdfParams {
        self.params
    }

    /// Encrypt `plaintext` (fresh nonce every call).
    pub fn seal(&self, plaintext: &[u8], purpose: &str) -> Result<Vec<u8>, VaultError> {
        let header = Header {
            format: FORMAT.into(),
            version: VERSION,
            purpose: purpose.into(),
            kdf: "argon2id".into(),
            params: self.params,
            salt: B64.encode(self.salt),
        };
        let aad = serde_json::to_vec(&header).map_err(|e| VaultError::Corrupt(e.to_string()))?;
        let mut nonce = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = XChaCha20Poly1305::new(self.key.as_ref().into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::Corrupt("encryption failed".into()))?;
        serde_json::to_vec_pretty(&Envelope {
            header,
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ct),
        })
        .map_err(|e| VaultError::Corrupt(e.to_string()))
    }
}

/// Is this a sealed envelope at all (whatever its passphrase)?
pub fn is_sealed(data: &[u8]) -> bool {
    serde_json::from_slice::<Envelope>(data).is_ok_and(|e| e.header.format == FORMAT)
}

/// Decrypt a sealed blob. Returns the plaintext and the key, so the caller
/// can seal an updated payload with the same salt cheaply.
///
/// A wrong passphrase and a modified file both give
/// [`VaultError::WrongPassword`]; they cannot be told apart by design.
pub fn open(
    data: &[u8],
    passphrase: &SecretString,
    purpose: &str,
) -> Result<(Zeroizing<Vec<u8>>, SealKey), VaultError> {
    open_with(data, purpose, |params, salt| {
        derive(passphrase, salt, params)
    })
}

/// [`open`] with a cache: reuse `key` when the blob has the same salt and
/// parameters, otherwise derive from `passphrase`.
pub fn open_cached(
    data: &[u8],
    passphrase: &SecretString,
    purpose: &str,
    cache: Option<&SealKey>,
) -> Result<(Zeroizing<Vec<u8>>, SealKey), VaultError> {
    open_with(data, purpose, |params, salt| match cache {
        Some(k) if k.params == params && k.salt[..] == *salt => Ok(k.key.clone()),
        _ => derive(passphrase, salt, params),
    })
}

fn open_with(
    data: &[u8],
    purpose: &str,
    key_for: impl FnOnce(KdfParams, &[u8]) -> Result<Zeroizing<[u8; 32]>, VaultError>,
) -> Result<(Zeroizing<Vec<u8>>, SealKey), VaultError> {
    let env: Envelope =
        serde_json::from_slice(data).map_err(|e| VaultError::Corrupt(e.to_string()))?;
    let h = &env.header;
    if h.format != FORMAT {
        return Err(VaultError::Corrupt(format!("unknown format {}", h.format)));
    }
    if h.version != VERSION {
        return Err(VaultError::UnsupportedVersion(h.version));
    }
    if h.purpose != purpose {
        return Err(VaultError::Corrupt(format!(
            "this file is for `{}`, not `{purpose}`",
            h.purpose
        )));
    }
    if h.kdf != "argon2id" {
        return Err(VaultError::Corrupt(format!("unknown kdf {}", h.kdf)));
    }
    let p = h.params;
    if p.m_cost_kib > MAX_M_COST_KIB || p.t_cost > MAX_T_COST || p.p_cost > MAX_P_COST {
        return Err(VaultError::Corrupt(
            "key-derivation parameters are out of range".into(),
        ));
    }
    Params::new(p.m_cost_kib, p.t_cost, p.p_cost, Some(32))
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    let salt: [u8; 16] = B64
        .decode(&h.salt)
        .ok()
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| VaultError::Corrupt("bad salt".into()))?;
    let nonce = B64
        .decode(&env.nonce)
        .ok()
        .filter(|n| n.len() == 24)
        .ok_or_else(|| VaultError::Corrupt("bad nonce".into()))?;
    let ct = B64
        .decode(&env.ciphertext)
        .map_err(|e| VaultError::Corrupt(e.to_string()))?;
    let aad = serde_json::to_vec(h).map_err(|e| VaultError::Corrupt(e.to_string()))?;
    let key = key_for(p, &salt)?;
    let plain = XChaCha20Poly1305::new(key.as_ref().into())
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ct,
                aad: &aad,
            },
        )
        .map_err(|_| VaultError::WrongPassword)?;
    Ok((
        Zeroizing::new(plain),
        SealKey {
            params: p,
            salt,
            key,
        },
    ))
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
    fn seal_open_wrong_pass_purpose_and_tamper() -> Result<(), VaultError> {
        let k = SealKey::new(&s("pw"), FAST)?;
        let blob = k.seal(b"hello", "test")?;
        assert!(is_sealed(&blob));
        assert!(!is_sealed(b"{}"));
        assert!(!String::from_utf8_lossy(&blob).contains("hello"));
        let (plain, k2) = open(&blob, &s("pw"), "test")?;
        assert_eq!(&plain[..], b"hello");
        assert!(matches!(
            open(&blob, &s("nope"), "test"),
            Err(VaultError::WrongPassword)
        ));
        assert!(matches!(
            open(&blob, &s("pw"), "other"),
            Err(VaultError::Corrupt(_))
        ));
        // Re-sealing with the returned key opens with the passphrase and
        // via the cache.
        let again = k2.seal(b"bye", "test")?;
        assert_ne!(again, blob);
        assert_eq!(&open(&again, &s("pw"), "test")?.0[..], b"bye");
        assert_eq!(
            &open_cached(&again, &s("ignored"), "test", Some(&k2))?.0[..],
            b"bye"
        );

        let text = String::from_utf8_lossy(&blob).replace("\"t_cost\": 1", "\"t_cost\": 2");
        assert!(matches!(
            open(text.as_bytes(), &s("pw"), "test"),
            Err(VaultError::WrongPassword)
        ));
        let huge = String::from_utf8_lossy(&blob).replace("\"t_cost\": 1", "\"t_cost\": 9999");
        assert!(matches!(
            open(huge.as_bytes(), &s("pw"), "test"),
            Err(VaultError::Corrupt(_))
        ));
        Ok(())
    }
}
