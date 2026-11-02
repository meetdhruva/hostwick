//! SSH key generation, conversion and inspection — Hostwick's `ssh-keygen`
//! and `puttygen`.
//!
//! - [`generate`] Ed25519, ECDSA (P-256/384/521) and RSA (2048–16384 bit) keys.
//! - [`write_openssh`] writes an OpenSSH private key (passphrase → bcrypt-pbkdf
//!   + aes256-ctr, exactly like `ssh-keygen`) and its `.pub` file.
//! - [`write_ppk`] writes a PuTTY `.ppk` file, format 3 (Argon2id +
//!   aes256-cbc + HMAC-SHA-256) or format 2 for old PuTTY/WinSCP builds.
//! - [`load_any`] reads OpenSSH, PuTTY (v2/v3) and legacy PEM (`BEGIN RSA
//!   PRIVATE KEY`, PKCS#8) private keys; [`inspect`] describes a key file
//!   without needing its passphrase.
//!
//! Private key files are created `0600` on Unix and never overwrite an
//! existing file unless asked to (`force`).

mod ppk;

use std::io::Write;
use std::path::{Path, PathBuf};

use secrecy::ExposeSecret;
use ssh_key::private::{EcdsaKeypair, Ed25519Keypair, KeypairData, RsaKeypair};
use ssh_key::public::KeyData;
use ssh_key::{EcdsaCurve, HashAlg, LineEnding};
use zeroize::Zeroizing;

pub use ppk::{PpkKdf, PpkVersion};
pub use secrecy::SecretString;
pub use ssh_key::{PrivateKey, PublicKey};

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("{0} already exists; choose another name or overwrite it explicitly")]
    Exists(PathBuf),
    #[error("this key is protected by a passphrase")]
    PassphraseRequired,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("{0}")]
    Unsupported(String),
    #[error("{0} is a public key; a private key is needed")]
    PublicOnly(String),
    #[error("not a private key Hostwick can read: {0}")]
    Format(String),
    #[error("key error: {0}")]
    Ssh(#[from] ssh_key::Error),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl KeyError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// Key algorithm and size to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Ed25519,
    EcdsaP256,
    EcdsaP384,
    EcdsaP521,
    Rsa { bits: u32 },
}

impl KeyType {
    /// Choices for a "generate key" dialog, recommended first.
    pub const CHOICES: [KeyType; 7] = [
        KeyType::Ed25519,
        KeyType::EcdsaP256,
        KeyType::EcdsaP384,
        KeyType::EcdsaP521,
        KeyType::Rsa { bits: 2048 },
        KeyType::Rsa { bits: 3072 },
        KeyType::Rsa { bits: 4096 },
    ];

    /// `ssh-keygen -t TYPE [-b BITS]`: `ed25519`, `ecdsa` (256, 384, 521;
    /// default 256), `rsa` (2048–16384; default 3072). Full algorithm names
    /// such as `ssh-ed25519` or `ecdsa-sha2-nistp384` work too.
    pub fn parse(name: &str, bits: Option<u32>) -> Result<Self, KeyError> {
        let name = name.trim().to_ascii_lowercase();
        let bad_bits = |what: &str| {
            Err(KeyError::Unsupported(format!(
                "{what} keys cannot be {} bits",
                bits.unwrap_or(0)
            )))
        };
        match name.as_str() {
            "ed25519" | "ssh-ed25519" => match bits {
                None | Some(256) => Ok(Self::Ed25519),
                _ => bad_bits("Ed25519"),
            },
            "ecdsa" => match bits {
                None | Some(256) => Ok(Self::EcdsaP256),
                Some(384) => Ok(Self::EcdsaP384),
                Some(521) => Ok(Self::EcdsaP521),
                _ => bad_bits("ECDSA (256, 384 or 521)"),
            },
            "ecdsa-sha2-nistp256" | "p256" | "nistp256" => Self::parse("ecdsa", Some(256)),
            "ecdsa-sha2-nistp384" | "p384" | "nistp384" => Self::parse("ecdsa", Some(384)),
            "ecdsa-sha2-nistp521" | "p521" | "nistp521" => Self::parse("ecdsa", Some(521)),
            "rsa" | "ssh-rsa" => {
                let bits = bits.unwrap_or(3072);
                if !(2048..=16384).contains(&bits) || !bits.is_multiple_of(8) {
                    return Err(KeyError::Unsupported(format!(
                        "RSA keys must be 2048 to 16384 bits (a multiple of 8), not {bits}"
                    )));
                }
                Ok(Self::Rsa { bits })
            }
            "dsa" | "ssh-dss" => Err(KeyError::Unsupported(
                "DSA keys are insecure and no longer accepted by OpenSSH; use ed25519".into(),
            )),
            other => Err(KeyError::Unsupported(format!(
                "unknown key type `{other}` (use ed25519, ecdsa or rsa)"
            ))),
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            Self::Ed25519 | Self::EcdsaP256 => 256,
            Self::EcdsaP384 => 384,
            Self::EcdsaP521 => 521,
            Self::Rsa { bits } => bits,
        }
    }

    /// `Ed25519`, `ECDSA P-384`, `RSA 4096`.
    pub fn label(self) -> String {
        match self {
            Self::Ed25519 => "Ed25519".into(),
            Self::EcdsaP256 => "ECDSA P-256".into(),
            Self::EcdsaP384 => "ECDSA P-384".into(),
            Self::EcdsaP521 => "ECDSA P-521".into(),
            Self::Rsa { bits } => format!("RSA {bits}"),
        }
    }

    /// OpenSSH's default file name for this type (`id_ed25519`, ...).
    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::Ed25519 => "id_ed25519",
            Self::EcdsaP256 | Self::EcdsaP384 | Self::EcdsaP521 => "id_ecdsa",
            Self::Rsa { .. } => "id_rsa",
        }
    }
}

fn rng() -> getrandom::rand_core::UnwrapErr<getrandom::SysRng> {
    getrandom::rand_core::UnwrapErr(getrandom::SysRng)
}

pub(crate) fn random_bytes(buf: &mut [u8]) -> Result<(), KeyError> {
    getrandom::fill(buf)
        .map_err(|e| KeyError::Unsupported(format!("the system random generator failed: {e}")))
}

/// Generate a new key pair. RSA keys take a moment (seconds for 4096 bits),
/// so call this off the UI thread.
pub fn generate(kind: KeyType, comment: &str) -> Result<PrivateKey, KeyError> {
    let mut rng = rng();
    let data = match kind {
        KeyType::Ed25519 => KeypairData::from(Ed25519Keypair::random(&mut rng)),
        KeyType::EcdsaP256 => EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP256)?.into(),
        KeyType::EcdsaP384 => EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP384)?.into(),
        KeyType::EcdsaP521 => EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP521)?.into(),
        KeyType::Rsa { bits } => {
            KeyType::parse("rsa", Some(bits))?;
            RsaKeypair::random(&mut rng, bits as usize)?.into()
        }
    };
    Ok(PrivateKey::new(data, single_line(comment))?)
}

/// Comments end up on one line in `.pub` and `.ppk` files.
fn single_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ").trim().to_owned()
}

/// Replace a key's comment.
pub fn set_comment(key: &mut PrivateKey, comment: &str) {
    key.set_comment(single_line(comment));
}

fn passphrase_of(p: Option<&SecretString>) -> Option<&str> {
    p.map(|s| s.expose_secret()).filter(|s| !s.is_empty())
}

fn ensure_plain(key: &PrivateKey) -> Result<(), KeyError> {
    if key.is_encrypted() {
        Err(KeyError::PassphraseRequired)
    } else {
        Ok(())
    }
}

/// OpenSSH private key text (`-----BEGIN OPENSSH PRIVATE KEY-----`). With a
/// non-empty passphrase it is encrypted like `ssh-keygen` does
/// (bcrypt-pbkdf, 16 rounds, aes256-ctr).
pub fn to_openssh(
    key: &PrivateKey,
    passphrase: Option<&SecretString>,
) -> Result<Zeroizing<String>, KeyError> {
    ensure_plain(key)?;
    let text = match passphrase_of(passphrase) {
        Some(p) => key
            .encrypt(&mut getrandom::SysRng, p)?
            .to_openssh(LineEnding::LF)?,
        None => key.to_openssh(LineEnding::LF)?,
    };
    Ok(text)
}

/// `ssh-ed25519 AAAA... comment` — the line for `authorized_keys`.
pub fn public_openssh(key: &PublicKey) -> String {
    key.to_openssh().unwrap_or_default()
}

/// `SHA256:...`, as `ssh-keygen -l` prints it.
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// OpenSSH's "randomart" picture of the SHA-256 fingerprint
/// (`ssh-keygen -lv`).
pub fn randomart(key: &PublicKey) -> String {
    let header = format!("[{} {}]", type_label(key), key_bits(key));
    key.fingerprint(HashAlg::Sha256).to_randomart(&header)
}

/// Key size in bits (Ed25519 → 256, P-384 → 384, RSA → modulus size).
pub fn key_bits(key: &PublicKey) -> u32 {
    match key.key_data() {
        KeyData::Ed25519(_) | KeyData::SkEd25519(_) => 256,
        KeyData::Ecdsa(e) => match e.curve() {
            EcdsaCurve::NistP256 => 256,
            EcdsaCurve::NistP384 => 384,
            EcdsaCurve::NistP521 => 521,
        },
        KeyData::SkEcdsaSha2NistP256(_) => 256,
        KeyData::Rsa(r) => r.key_size(),
        KeyData::Dsa(d) => {
            u32::try_from(d.p().as_positive_bytes().map_or(0, <[u8]>::len) * 8).unwrap_or(0)
        }
        _ => 0,
    }
}

/// `ED25519`, `ECDSA`, `RSA`, ... as `ssh-keygen` labels them.
pub fn type_label(key: &PublicKey) -> &'static str {
    match key.key_data() {
        KeyData::Ed25519(_) => "ED25519",
        KeyData::SkEd25519(_) => "ED25519-SK",
        KeyData::Ecdsa(_) => "ECDSA",
        KeyData::SkEcdsaSha2NistP256(_) => "ECDSA-SK",
        KeyData::Rsa(_) => "RSA",
        KeyData::Dsa(_) => "DSA",
        _ => "UNKNOWN",
    }
}

/// On-disk private key format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFormat {
    /// `-----BEGIN OPENSSH PRIVATE KEY-----`
    OpenSsh,
    /// PuTTY `.ppk`.
    Ppk(PpkVersion),
    /// Legacy PEM: `BEGIN RSA PRIVATE KEY`, `BEGIN EC PRIVATE KEY`, PKCS#8.
    Pem,
    /// A public key line (`.pub`).
    Public,
}

impl std::fmt::Display for KeyFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OpenSsh => "OpenSSH",
            Self::Ppk(PpkVersion::V2) => "PuTTY PPK v2",
            Self::Ppk(PpkVersion::V3) => "PuTTY PPK v3",
            Self::Pem => "PEM (legacy)",
            Self::Public => "public key",
        })
    }
}

/// What a key file holds, readable without its passphrase.
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub format: KeyFormat,
    pub encrypted: bool,
    /// `ssh-ed25519`, `ecdsa-sha2-nistp256`, `ssh-rsa`, ...
    pub algorithm: String,
    /// `ED25519`, `ECDSA`, `RSA`.
    pub type_label: &'static str,
    pub bits: u32,
    pub comment: String,
    /// `SHA256:...`
    pub fingerprint: String,
    pub public_key: PublicKey,
}

impl KeyInfo {
    fn new(format: KeyFormat, encrypted: bool, public_key: PublicKey) -> Self {
        Self {
            format,
            encrypted,
            algorithm: public_key.algorithm().as_str().to_owned(),
            type_label: type_label(&public_key),
            bits: key_bits(&public_key),
            comment: public_key.comment().as_str_lossy().to_owned(),
            fingerprint: fingerprint(&public_key),
            public_key,
        }
    }
}

fn read_text(path: &Path) -> Result<String, KeyError> {
    let bytes = std::fs::read(path).map_err(|e| KeyError::io(path, e))?;
    String::from_utf8(bytes)
        .map_err(|_| KeyError::Format(format!("{} is not a text key file", path.display())))
}

/// `id_ed25519` → `id_ed25519.pub`.
pub fn public_path(private: &Path) -> PathBuf {
    let mut s = private.as_os_str().to_owned();
    s.push(".pub");
    PathBuf::from(s)
}

fn detect(text: &str) -> Option<KeyFormat> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("PuTTY-User-Key-File-2") {
        Some(KeyFormat::Ppk(PpkVersion::V2))
    } else if t.starts_with("PuTTY-User-Key-File-") {
        Some(KeyFormat::Ppk(PpkVersion::V3))
    } else if t.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        Some(KeyFormat::OpenSsh)
    } else if t.contains("-----BEGIN ") && t.contains("PRIVATE KEY-----") {
        Some(KeyFormat::Pem)
    } else if PublicKey::from_openssh(t.lines().next().unwrap_or("")).is_ok() {
        Some(KeyFormat::Public)
    } else {
        None
    }
}

/// Describe a key file: format, type, size, comment, fingerprint and whether
/// it is passphrase-protected. Never needs the passphrase (for an encrypted
/// legacy PEM key the public half comes from the `.pub` file beside it).
pub fn inspect(path: &Path) -> Result<KeyInfo, KeyError> {
    let text = read_text(path)?;
    let sibling = std::fs::read_to_string(public_path(path))
        .ok()
        .and_then(|t| PublicKey::from_openssh(t.trim()).ok());
    inspect_text(&text, sibling)
}

/// [`inspect`] for key text; `public` is the matching `.pub` key if known
/// (used for the comment of encrypted OpenSSH keys and for encrypted PEM).
pub fn inspect_text(text: &str, public: Option<PublicKey>) -> Result<KeyInfo, KeyError> {
    let format = detect(text).ok_or_else(|| KeyError::Format("unrecognised key format".into()))?;
    match format {
        KeyFormat::Ppk(_) => {
            let h = ppk::parse_header(text)?;
            let mut pk = PublicKey::from_bytes(&h.public_blob)?;
            pk.set_comment(h.comment);
            Ok(KeyInfo::new(format, h.encrypted, pk))
        }
        KeyFormat::OpenSsh => {
            let k = PrivateKey::from_openssh(text.trim())?;
            let mut pk = k.public_key().clone();
            // An encrypted key keeps its comment inside the encrypted part.
            if k.is_encrypted()
                && let Some(p) = public.filter(|p| p.key_data() == pk.key_data())
            {
                pk.set_comment(p.comment().clone());
            }
            Ok(KeyInfo::new(format, k.is_encrypted(), pk))
        }
        KeyFormat::Pem => {
            let encrypted = text.contains("ENCRYPTED");
            if encrypted {
                let pk = public.ok_or(KeyError::PassphraseRequired)?;
                Ok(KeyInfo::new(format, true, pk))
            } else {
                let k = parse_any(text, None)?;
                Ok(KeyInfo::new(format, false, k.public_key().clone()))
            }
        }
        KeyFormat::Public => {
            let pk = PublicKey::from_openssh(text.trim().lines().next().unwrap_or(""))?;
            Ok(KeyInfo::new(format, false, pk))
        }
    }
}

/// Load and decrypt a private key in any supported format.
pub fn load_any(path: &Path, passphrase: Option<&SecretString>) -> Result<PrivateKey, KeyError> {
    parse_any(&read_text(path)?, passphrase)
}

/// [`load_any`] for key text.
pub fn parse_any(text: &str, passphrase: Option<&SecretString>) -> Result<PrivateKey, KeyError> {
    let pass = passphrase_of(passphrase);
    match detect(text) {
        Some(KeyFormat::Ppk(_)) => {
            let h = ppk::parse_header(text)?;
            if h.encrypted && pass.is_none() {
                return Err(KeyError::PassphraseRequired);
            }
            let pass = if h.encrypted { pass } else { None };
            PrivateKey::from_ppk(text.trim_start_matches('\u{feff}'), pass.map(str::to_owned))
                .map_err(|e| {
                    // The MAC covers the decrypted key, so a wrong passphrase
                    // shows up as a MAC mismatch.
                    let bad_mac =
                        matches!(&e, ssh_key::Error::Ppk(p) if p.to_string().contains("MAC"));
                    match (bad_mac, h.encrypted) {
                        (true, true) => KeyError::WrongPassphrase,
                        (true, false) => KeyError::Format(
                            "the .ppk file is damaged (its integrity check failed)".into(),
                        ),
                        _ => KeyError::Format(e.to_string()),
                    }
                })
        }
        Some(KeyFormat::OpenSsh) => {
            let k = PrivateKey::from_openssh(text.trim())?;
            if !k.is_encrypted() {
                return Ok(k);
            }
            let pass = pass.ok_or(KeyError::PassphraseRequired)?;
            k.decrypt(pass).map_err(|_| KeyError::WrongPassphrase)
        }
        Some(KeyFormat::Pem) => {
            let encrypted = text.contains("ENCRYPTED");
            if encrypted && pass.is_none() {
                return Err(KeyError::PassphraseRequired);
            }
            russh::keys::decode_secret_key(text, if encrypted { pass } else { None }).map_err(|e| {
                if encrypted {
                    KeyError::WrongPassphrase
                } else {
                    KeyError::Format(e.to_string())
                }
            })
        }
        Some(KeyFormat::Public) => Err(KeyError::PublicOnly("this file".into())),
        None => Err(KeyError::Format(
            "expected an OpenSSH, PuTTY (.ppk) or PEM private key".into(),
        )),
    }
}

/// PuTTY `.ppk` text. A non-empty passphrase encrypts it (v3: Argon2id with
/// `kdf`, v2: PuTTY's SHA-1 scheme).
pub fn to_ppk(
    key: &PrivateKey,
    passphrase: Option<&SecretString>,
    version: PpkVersion,
    kdf: &PpkKdf,
) -> Result<Zeroizing<String>, KeyError> {
    ensure_plain(key)?;
    ppk::encode(key, passphrase_of(passphrase), version, kdf)
}

/// Write `path` (private key, `0600`) and `path.pub`. Refuses to replace
/// either file unless `force`.
pub fn write_openssh(
    key: &PrivateKey,
    path: &Path,
    passphrase: Option<&SecretString>,
    force: bool,
) -> Result<(), KeyError> {
    let pub_path = public_path(path);
    if !force {
        refuse_existing(path)?;
        refuse_existing(&pub_path)?;
    }
    let text = to_openssh(key, passphrase)?;
    write_file(path, text.as_bytes(), true, force)?;
    write_public(key.public_key(), &pub_path, force)
}

/// Write a PuTTY `.ppk` file (`0600`). Refuses to replace it unless `force`.
pub fn write_ppk(
    key: &PrivateKey,
    path: &Path,
    passphrase: Option<&SecretString>,
    version: PpkVersion,
    force: bool,
) -> Result<(), KeyError> {
    if !force {
        refuse_existing(path)?;
    }
    let text = to_ppk(key, passphrase, version, &PpkKdf::default())?;
    write_file(path, text.as_bytes(), true, force)
}

/// Write the one-line OpenSSH public key.
pub fn write_public(key: &PublicKey, path: &Path, force: bool) -> Result<(), KeyError> {
    let line = format!("{}\n", public_openssh(key));
    write_file(path, line.as_bytes(), false, force)
}

/// Re-encrypt a key file in place under a new passphrase (`None` or empty
/// removes it), keeping its format. A legacy PEM key is rewritten in the
/// OpenSSH format, like `ssh-keygen -p` does. Returns the format written.
pub fn change_passphrase(
    path: &Path,
    old: Option<&SecretString>,
    new: Option<&SecretString>,
) -> Result<KeyFormat, KeyError> {
    let text = read_text(path)?;
    let key = parse_any(&text, old)?;
    match detect(&text) {
        Some(KeyFormat::Ppk(v)) => {
            let out = to_ppk(&key, new, v, &PpkKdf::default())?;
            write_file(path, out.as_bytes(), true, true)?;
            Ok(KeyFormat::Ppk(v))
        }
        _ => {
            let out = to_openssh(&key, new)?;
            write_file(path, out.as_bytes(), true, true)?;
            Ok(KeyFormat::OpenSsh)
        }
    }
}

fn refuse_existing(path: &Path) -> Result<(), KeyError> {
    if path.exists() {
        Err(KeyError::Exists(path.to_path_buf()))
    } else {
        Ok(())
    }
}

/// Write via a temporary file in the same folder and rename, so a crash never
/// leaves half a key. Private files are created `0600` from the start.
fn write_file(path: &Path, data: &[u8], private: bool, force: bool) -> Result<(), KeyError> {
    if !force {
        refuse_existing(path)?;
    }
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir).map_err(|e| KeyError::io(&dir, e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "key".into());
    let mut nonce = [0u8; 6];
    random_bytes(&mut nonce)?;
    let tmp = dir.join(format!(".{name}.{}.tmp", hex::encode(nonce)));
    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(if private { 0o600 } else { 0o644 });
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(KeyError::io(path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
