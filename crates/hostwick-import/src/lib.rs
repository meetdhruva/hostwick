//! Importers that turn other tools' saved sessions into [`HostProfile`]s.
//!
//! Every importer parses untrusted input: none of them panic, and anything
//! they cannot represent becomes a human-readable warning instead of an
//! error, so one odd session never blocks importing the rest.

mod putty;
mod raw;
mod solarputty;
mod ssh_config;
mod winscp;

use hostwick_core::{AuthMethod, HostProfile};

pub use putty::{parse_putty_reg, putty_from_registry};
pub use solarputty::parse_solarputty_json;
pub use ssh_config::parse_ssh_config;
pub use winscp::{parse_winscp_ini, winscp_from_registry};

#[derive(Debug, Default)]
pub struct ImportResult {
    pub profiles: Vec<HostProfile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not a recognized {0} file")]
    Format(&'static str),
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Decode a text file that may be UTF-8 (with or without BOM) or UTF-16LE
/// with BOM — regedit exports `.reg` files as UTF-16LE.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Auth order for imported sessions: agent (Pageant holds PuTTY keys), the
/// session's key file, then password and keyboard-interactive prompts.
pub(crate) fn auth_with_key(key: Option<String>) -> Vec<AuthMethod> {
    let mut v = vec![AuthMethod::Agent];
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        v.push(AuthMethod::KeyFile {
            path: k.into(),
            passphrase_credential: None,
        });
    }
    v.push(AuthMethod::Password { credential: None });
    v.push(AuthMethod::KeyboardInteractive);
    v
}

/// Decode `%XX` escapes (PuTTY session names, WinSCP values).
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// `folder/sub/name` → (`folder/sub`, `name`).
pub(crate) fn split_folder(path: &str) -> (String, String) {
    let path = path.trim_matches('/');
    match path.rsplit_once('/') {
        Some((f, n)) => (f.to_owned(), n.to_owned()),
        None => (String::new(), path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bom_variants_and_escapes() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFhi"), "hi");
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(percent_decode("my%20server%2Fx"), "my server/x");
        assert_eq!(percent_decode("bad%2"), "bad%2");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(split_folder("a/b/c"), ("a/b".into(), "c".into()));
        assert_eq!(split_folder("c"), (String::new(), "c".into()));
    }
}
