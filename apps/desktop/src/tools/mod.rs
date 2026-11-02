//! Tools menu: key generator, key inspector/converter, known_hosts manager.

pub mod keyconv;
pub mod keygen;
pub mod knownhosts;

use std::path::{Path, PathBuf};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

/// Private key file format to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutFormat {
    OpenSsh,
    Ppk3,
    Ppk2,
}

impl OutFormat {
    pub const ALL: [OutFormat; 3] = [OutFormat::OpenSsh, OutFormat::Ppk3, OutFormat::Ppk2];

    pub fn label(self) -> &'static str {
        match self {
            OutFormat::OpenSsh => "OpenSSH",
            OutFormat::Ppk3 => "PuTTY .ppk v3",
            OutFormat::Ppk2 => "PuTTY .ppk v2",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            OutFormat::OpenSsh => {
                "For OpenSSH, Hostwick, Git and most tools (a .pub file is written too)."
            }
            OutFormat::Ppk3 => "For PuTTY 0.75+ and WinSCP (Argon2-protected).",
            OutFormat::Ppk2 => "For older PuTTY, WinSCP and FileZilla versions.",
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/.ssh` (where keys normally live).
pub fn default_key_dir() -> PathBuf {
    home()
        .map(|h| h.join(".ssh"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A path typed by the person: `~/` expanded; a bare name goes in `~/.ssh`.
pub fn ssh_dir_path(text: &str) -> PathBuf {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("~/").or_else(|| t.strip_prefix("~\\")) {
        return home()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(t));
    }
    let p = Path::new(t);
    if p.components().count() == 1 && !p.is_absolute() {
        return default_key_dir().join(p);
    }
    p.to_path_buf()
}

/// Hard-wrap long unbroken text (base64 keys) every `n` characters.
pub fn wrap_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars
        .chunks(n.max(1))
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Monospace block (fingerprints, randomart, public keys).
pub fn mono(text: String, cx: &App) -> Div {
    div()
        .font_family(crate::terminal::monospace_family())
        .text_size(px(12.0))
        .p(px(6.0))
        .rounded(px(4.0))
        .bg(cx.theme().muted)
        .children(
            text.lines()
                .map(|l| div().child(l.to_owned()))
                .collect::<Vec<_>>(),
        )
}

#[cfg(test)]
mod tests {
    use super::{default_key_dir, ssh_dir_path, wrap_chars};

    #[test]
    fn typed_key_paths() {
        assert_eq!(ssh_dir_path("work"), default_key_dir().join("work"));
        assert_eq!(ssh_dir_path("/tmp/k"), std::path::PathBuf::from("/tmp/k"));
        assert!(ssh_dir_path("~/x/id").ends_with("x/id"));
        assert_eq!(wrap_chars("abcdefg", 3), "abc\ndef\ng");
    }
}
