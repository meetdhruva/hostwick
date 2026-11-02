//! Which names a listing may hand on.
//!
//! Names in a directory listing come from the server. A well-behaved
//! server never sends `..` or a name with `/` in it, but a malicious one can,
//! an `ls` listing can be confused by unusual names, and a Linux file may
//! legitimately be called `..\..\Startup\x.bat` — which on Windows is a path
//! that climbs out of the download folder. Every listing goes through
//! [`filter_listing`]: entries whose name is not a plain, single path
//! component on *this* computer are left out and reported, and every local
//! path built from remote names is checked again with [`join_checked`].

use crate::{Entry, FsError, join};

/// A listing with the entries that were left out because of their names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Names that were skipped, with the reason.
    pub skipped: Vec<(String, &'static str)>,
}

/// Why `name` cannot be used as one path component here, or `None` if it
/// can. Rejects empty names, `.` and `..`, path separators (`/` and `\`),
/// NUL, and drive prefixes (`C:`); on Windows also characters Windows
/// forbids, reserved device names (`CON`, `NUL`, `COM1`, …) and names
/// ending in a dot or space.
pub fn unsafe_name_reason(name: &str) -> Option<&'static str> {
    if let Some(r) = portable_reason(name) {
        return Some(r);
    }
    if cfg!(windows) {
        windows_reason(name)
    } else {
        None
    }
}

fn portable_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("empty name");
    }
    if name == "." || name == ".." {
        return Some("refers to a folder, not an entry in it");
    }
    if name.contains(['/', '\\']) {
        return Some("contains a path separator");
    }
    if name.contains('\0') {
        return Some("contains a NUL byte");
    }
    let b = name.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some("starts with a drive letter");
    }
    None
}

/// The extra rules for Windows file names (public within the crate so they
/// are tested on every platform).
pub(crate) fn windows_reason(name: &str) -> Option<&'static str> {
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 0x20)
    {
        return Some("contains a character Windows does not allow");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("ends with a dot or space (Windows would change the name)");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Some("is a reserved device name on Windows");
    }
    None
}

/// Split `entries` into the usable ones and the skipped names. Each skipped
/// name is logged.
pub fn filter_listing(dir: &str, entries: Vec<Entry>) -> Listing {
    let mut out = Listing::default();
    for e in entries {
        match unsafe_name_reason(&e.name) {
            None => out.entries.push(e),
            Some(reason) => {
                tracing::warn!(dir, name = ?e.name, reason, "skipping entry with an unsafe name");
                out.skipped.push((e.name, reason));
            }
        }
    }
    out
}

/// `root` joined with the `/`-separated relative path `rel`, refusing any
/// component that is not a safe name (so the result can never leave
/// `root`).
pub fn join_checked(root: &str, rel: &str) -> Result<String, FsError> {
    for part in rel.split('/') {
        if let Some(reason) = unsafe_name_reason(part) {
            return Err(FsError::UnsafeName {
                name: rel.to_owned(),
                reason,
            });
        }
    }
    Ok(join(root, rel))
}

/// Check one name that will become a local path component.
pub fn check_name(name: &str) -> Result<(), FsError> {
    match unsafe_name_reason(name) {
        None => Ok(()),
        Some(reason) => Err(FsError::UnsafeName {
            name: name.to_owned(),
            reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_rules() {
        for ok in [
            "a", "file.txt", ".bashrc", "...", "a b", "ünï", "ab:c", "a.", "con",
        ] {
            assert_eq!(portable_reason(ok), None, "{ok:?}");
        }
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "../x",
            "..\\..\\Startup\\x.bat",
            "a\\b",
            "nul\0byte",
            "C:",
            "c:evil",
            "C:\\Windows",
        ] {
            assert!(portable_reason(bad).is_some(), "{bad:?}");
        }
    }

    #[test]
    fn windows_rules() {
        for bad in [
            "CON",
            "con.txt",
            "Nul",
            "aux.tar.gz",
            "COM1",
            "lpt9.log",
            "a:b",
            "x?",
            "a*",
            "tab\there",
            "dot.",
            "space ",
            "q\"",
        ] {
            assert!(windows_reason(bad).is_some(), "{bad:?}");
        }
        for ok in [
            "CONSOLE",
            "com10",
            "LPT",
            "a.b",
            "normal name.txt",
            ".hidden",
        ] {
            assert_eq!(windows_reason(ok), None, "{ok:?}");
        }
    }

    #[test]
    fn join_refuses_escapes() {
        assert_eq!(
            join_checked("/home/me", "a/b.txt").ok().as_deref(),
            Some("/home/me/a/b.txt")
        );
        assert!(join_checked("/home/me", "../evil").is_err());
        assert!(join_checked("/home/me", "a/../../evil").is_err());
        assert!(join_checked("C:/Users/me", "..\\..\\x.bat").is_err());
        assert!(join_checked("/x", "").is_err());
        assert!(join_checked("/x", "a//b").is_err());
    }
}
