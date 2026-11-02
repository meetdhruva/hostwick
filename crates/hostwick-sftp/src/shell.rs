//! Building POSIX shell command lines safely.
//!
//! Every path that reaches a remote shell goes through [`quote`]: single
//! quotes make everything literal except the single quote itself, which is
//! written as `'\''` (close, escaped quote, reopen). That works in every
//! POSIX `sh`, busybox `ash`, `dash` and `bash`, whatever the path contains
//! (spaces, `$`, backticks, newlines, `*`).

/// Quote one argument for a POSIX shell.
pub fn quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:,@%".contains(&b))
    {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// A path argument that can never be mistaken for an option: relative paths
/// starting with `-` get a `./` prefix. Quoted.
pub fn path_arg(p: &str) -> String {
    if p.starts_with('-') {
        quote(&format!("./{p}"))
    } else {
        quote(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the quoted text through a real shell and check it comes back
    /// unchanged (Unix only; `sh` is always there).
    #[cfg(unix)]
    fn roundtrip(s: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s' {}", quote(s)))
            .output()
            .unwrap_or_else(|e| panic!("sh: {e}"));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("/usr/lib/x"), "/usr/lib/x");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(path_arg("-rf"), "./-rf");
        assert_eq!(path_arg("/-x"), "/-x");
    }

    #[cfg(unix)]
    #[test]
    fn quoting_survives_a_real_shell() {
        for s in [
            "plain",
            "with space",
            "it's",
            "''",
            "a'b'c",
            "$HOME `id` $(id) \\ \" ; | & > <",
            "new\nline",
            "*?[a]",
            "tab\there",
            "ünïcödé ✓",
            "-n",
        ] {
            assert_eq!(roundtrip(s), s, "for {s:?}");
        }
    }
}
