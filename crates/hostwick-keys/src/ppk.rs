//! PuTTY private key files (`.ppk`), formats 2 and 3.
//!
//! Layout (PuTTY manual, appendix C):
//!
//! ```text
//! PuTTY-User-Key-File-3: ssh-ed25519
//! Encryption: aes256-cbc            (or none)
//! Comment: me@laptop
//! Public-Lines: 2                   base64, 64 columns
//! ...
//! Key-Derivation: Argon2id          (v3, encrypted only)
//! Argon2-Memory: 8192
//! Argon2-Passes: 21
//! Argon2-Parallelism: 1
//! Argon2-Salt: <hex>
//! Private-Lines: 1
//! ...
//! Private-MAC: <hex>
//! ```
//!
//! - v3: Argon2id(passphrase, salt) → 80 bytes = AES-256 key ‖ CBC IV ‖
//!   HMAC-SHA-256 key. Unencrypted keys use an empty MAC key.
//! - v2: key = SHA-1(00000000 ‖ pass) ‖ SHA-1(00000001 ‖ pass), truncated to
//!   32 bytes, zero IV; MAC = HMAC-SHA-1 keyed with
//!   SHA-1("putty-private-key-file-mac-key" ‖ pass).
//! - The MAC covers string(algorithm), string(encryption), string(comment),
//!   string(public blob), string(private blob, decrypted and padded).
//! - Encrypted private blobs are padded to 16 bytes with the leading bytes of
//!   SHA-1(unpadded blob), as PuTTY does.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha1::{Digest, Sha1};
use ssh_key::PrivateKey;
use ssh_key::private::KeypairData;
use ssh_key::sha2::Sha256;
use zeroize::Zeroizing;

use crate::KeyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PpkVersion {
    /// PuTTY < 0.75, older WinSCP and FileZilla.
    V2,
    /// PuTTY 0.75+ (2021) and current WinSCP: Argon2id passphrase hashing.
    #[default]
    V3,
}

/// Argon2id cost for encrypted v3 files. The default matches `puttygen`
/// (8 MiB, 1 lane, passes calibrated to ~100 ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpkKdf {
    pub memory_kib: u32,
    pub passes: u32,
    pub parallelism: u32,
}

impl Default for PpkKdf {
    fn default() -> Self {
        Self {
            memory_kib: 8192,
            passes: 21,
            parallelism: 1,
        }
    }
}

const V2_MAC_PREFIX: &[u8] = b"putty-private-key-file-mac-key";
const LINE: usize = 64;

/// The parts of a `.ppk` readable without the passphrase.
pub(crate) struct Header {
    pub encrypted: bool,
    pub comment: String,
    pub public_blob: Vec<u8>,
}

pub(crate) fn parse_header(text: &str) -> Result<Header, KeyError> {
    let bad = |m: &str| KeyError::Format(format!("damaged .ppk file: {m}"));
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let first = lines.next().unwrap_or("");
    let version = first
        .strip_prefix("PuTTY-User-Key-File-")
        .and_then(|r| r.split_once(':'))
        .map(|(v, _)| v.trim())
        .ok_or_else(|| bad("missing header"))?;
    if version == "1" {
        return Err(KeyError::Unsupported(
            "PuTTY key format 1 (SSH-1) is not supported".into(),
        ));
    }
    if version != "2" && version != "3" {
        return Err(KeyError::Unsupported(format!(
            "PuTTY key format {version} is newer than this Hostwick understands"
        )));
    }
    let mut encrypted = None;
    let mut comment = String::new();
    let mut public_blob = None;
    while let Some(line) = lines.next() {
        let Some((k, v)) = line.split_once(": ") else {
            continue;
        };
        match k {
            "Encryption" => encrypted = Some(v.trim() != "none"),
            "Comment" => comment = v.to_owned(),
            "Public-Lines" => {
                let n: usize = v.trim().parse().map_err(|_| bad("bad Public-Lines"))?;
                let mut b64 = String::new();
                for _ in 0..n {
                    b64.push_str(lines.next().ok_or_else(|| bad("truncated"))?.trim());
                }
                public_blob = Some(B64.decode(b64).map_err(|_| bad("bad base64"))?);
            }
            _ => {}
        }
    }
    Ok(Header {
        encrypted: encrypted.ok_or_else(|| bad("no Encryption line"))?,
        comment,
        public_blob: public_blob.ok_or_else(|| bad("no public key"))?,
    })
}

fn put_string(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

/// SSH `mpint` from an unsigned big-endian integer.
fn put_mpint_unsigned(out: &mut Vec<u8>, bytes: &[u8]) {
    let trimmed = match bytes.iter().position(|b| *b != 0) {
        Some(i) => &bytes[i..],
        None => &[][..],
    };
    let pad = trimmed.first().is_some_and(|b| b & 0x80 != 0);
    out.extend_from_slice(&((trimmed.len() + usize::from(pad)) as u32).to_be_bytes());
    if pad {
        out.push(0);
    }
    out.extend_from_slice(trimmed);
}

/// The algorithm-specific private fields, as PuTTY stores them.
fn private_blob(key: &PrivateKey) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    let mut out = Zeroizing::new(Vec::new());
    match key.key_data() {
        // PuTTY writes the 32-byte seed as a string (not a true mpint).
        KeypairData::Ed25519(kp) => put_string(&mut out, &kp.private.to_bytes()),
        KeypairData::Ecdsa(kp) => put_mpint_unsigned(&mut out, kp.private_key_bytes()),
        KeypairData::Rsa(kp) => {
            let p = kp.private();
            for m in [p.d(), p.p(), p.q(), p.iqmp()] {
                // `Mpint::as_bytes` is already the two's-complement body.
                put_string(&mut out, m.as_bytes());
            }
        }
        other => {
            return Err(KeyError::Unsupported(format!(
                "{} keys cannot be saved as .ppk",
                other
                    .algorithm()
                    .map(|a| a.as_str().to_owned())
                    .unwrap_or_else(|_| "these".into())
            )));
        }
    }
    Ok(out)
}

struct Secrets {
    aes_key: Zeroizing<[u8; 32]>,
    iv: [u8; 16],
    mac_key: Zeroizing<Vec<u8>>,
}

fn v3_secrets(pass: &str, salt: &[u8], kdf: &PpkKdf) -> Result<Secrets, KeyError> {
    let params = Params::new(kdf.memory_kib, kdf.passes, kdf.parallelism, Some(80))
        .map_err(|e| KeyError::Unsupported(format!("Argon2 parameters: {e}")))?;
    let mut out = Zeroizing::new([0u8; 80]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pass.as_bytes(), salt, out.as_mut())
        .map_err(|e| KeyError::Unsupported(format!("Argon2: {e}")))?;
    let mut aes_key = Zeroizing::new([0u8; 32]);
    aes_key.copy_from_slice(&out[..32]);
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&out[32..48]);
    Ok(Secrets {
        aes_key,
        iv,
        mac_key: Zeroizing::new(out[48..80].to_vec()),
    })
}

fn v2_secrets(pass: &str) -> Secrets {
    let mut aes_key = Zeroizing::new([0u8; 32]);
    let h0 = Sha1::new()
        .chain_update([0, 0, 0, 0])
        .chain_update(pass.as_bytes())
        .finalize();
    let h1 = Sha1::new()
        .chain_update([0, 0, 0, 1])
        .chain_update(pass.as_bytes())
        .finalize();
    aes_key[..20].copy_from_slice(&h0);
    aes_key[20..].copy_from_slice(&h1[..12]);
    Secrets {
        aes_key,
        iv: [0u8; 16],
        mac_key: Zeroizing::new(v2_mac_key(pass)),
    }
}

fn v2_mac_key(pass: &str) -> Vec<u8> {
    Sha1::new()
        .chain_update(V2_MAC_PREFIX)
        .chain_update(pass.as_bytes())
        .finalize()
        .to_vec()
}

fn push_b64_lines(out: &mut String, label: &str, data: &[u8]) {
    let b64 = Zeroizing::new(B64.encode(data));
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(LINE)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    out.push_str(&format!("{label}: {}\n", lines.len()));
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
}

pub(crate) fn encode(
    key: &PrivateKey,
    pass: Option<&str>,
    version: PpkVersion,
    kdf: &PpkKdf,
) -> Result<Zeroizing<String>, KeyError> {
    let algorithm = key.algorithm();
    let algorithm = algorithm.as_str();
    let comment = key.comment().as_str_lossy().replace(['\r', '\n'], " ");
    let public_blob = key.public_key().to_bytes()?;
    let mut private = private_blob(key)?;
    let encryption = if pass.is_some() { "aes256-cbc" } else { "none" };

    if pass.is_some() {
        let rem = private.len() % 16;
        if rem != 0 {
            let digest = Sha1::digest(&private[..]);
            private.extend_from_slice(&digest[..16 - rem]);
        }
    }

    let mut salt = [0u8; 16];
    let secrets = match (pass, version) {
        (Some(p), PpkVersion::V3) => {
            crate::random_bytes(&mut salt)?;
            Some(v3_secrets(p, &salt, kdf)?)
        }
        (Some(p), PpkVersion::V2) => Some(v2_secrets(p)),
        (None, _) => None,
    };
    let mac_key: Vec<u8> = match (&secrets, version) {
        (Some(s), _) => s.mac_key.to_vec(),
        (None, PpkVersion::V3) => Vec::new(),
        (None, PpkVersion::V2) => v2_mac_key(""),
    };

    let mut mac_data = Zeroizing::new(Vec::new());
    put_string(&mut mac_data, algorithm.as_bytes());
    put_string(&mut mac_data, encryption.as_bytes());
    put_string(&mut mac_data, comment.as_bytes());
    put_string(&mut mac_data, &public_blob);
    put_string(&mut mac_data, &private);
    let mac = match version {
        PpkVersion::V3 => {
            let mut h = <Hmac<Sha256> as KeyInit>::new_from_slice(&mac_key)
                .map_err(|_| KeyError::Unsupported("HMAC key".into()))?;
            h.update(&mac_data);
            h.finalize().into_bytes().to_vec()
        }
        PpkVersion::V2 => {
            let mut h = <Hmac<Sha1> as KeyInit>::new_from_slice(&mac_key)
                .map_err(|_| KeyError::Unsupported("HMAC key".into()))?;
            h.update(&mac_data);
            h.finalize().into_bytes().to_vec()
        }
    };

    if let Some(s) = &secrets {
        ssh_key::Cipher::Aes256Cbc
            .encrypt(s.aes_key.as_ref(), &s.iv, &mut private)
            .map_err(|e| KeyError::Unsupported(format!("AES-256-CBC: {e}")))?;
    }

    let v = match version {
        PpkVersion::V2 => 2,
        PpkVersion::V3 => 3,
    };
    let mut out = Zeroizing::new(String::new());
    out.push_str(&format!("PuTTY-User-Key-File-{v}: {algorithm}\n"));
    out.push_str(&format!("Encryption: {encryption}\n"));
    out.push_str(&format!("Comment: {comment}\n"));
    push_b64_lines(&mut out, "Public-Lines", &public_blob);
    if secrets.is_some() && version == PpkVersion::V3 {
        out.push_str("Key-Derivation: Argon2id\n");
        out.push_str(&format!("Argon2-Memory: {}\n", kdf.memory_kib));
        out.push_str(&format!("Argon2-Passes: {}\n", kdf.passes));
        out.push_str(&format!("Argon2-Parallelism: {}\n", kdf.parallelism));
        out.push_str(&format!("Argon2-Salt: {}\n", hex::encode(salt)));
    }
    push_b64_lines(&mut out, "Private-Lines", &private);
    out.push_str(&format!("Private-MAC: {}\n", hex::encode(mac)));
    Ok(out)
}
