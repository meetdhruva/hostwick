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

/// Run `script` with `/bin/sh`, whatever the account's login shell is.
///
/// sshd hands every command to the login shell, and [`quote`]'s quoting is
/// POSIX: fish, for one, reads `\'` inside single quotes as an escaped
/// quote, so a crafted file name could end the quoting there. The command
/// built here is the same in every shell (sh, bash, zsh, fish, csh, tcsh,
/// nushell): `exec /bin/sh -c '...'`, where the single-quoted part contains
/// no `'`, no `\\`, no `!` and no line break, and decodes the real script
/// with `printf` (every byte that could mean something is written as a
/// `\ooo` octal escape) and runs it with `eval`.
pub fn portable(script: &str) -> String {
    use std::fmt::Write as _;
    let mut enc = String::with_capacity(script.len() + script.len() / 2);
    for b in script.bytes() {
        if b.is_ascii_alphanumeric() || b" _./=+:,@-;|&<>()[]{}*?#~^".contains(&b) {
            enc.push(char::from(b));
        } else {
            let _ = write!(enc, "\\{b:03o}");
        }
    }
    format!("exec /bin/sh -c 'eval \"$(printf \"{enc}\")\"'")
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

    /// The wrapper must be inert in non-POSIX shells: nothing in it that
    /// fish (`\\`, `\'`), csh (`!`, line breaks) or any shell (`'`) would
    /// treat specially inside single quotes.
    #[test]
    fn portable_wrapper_is_inert_in_any_shell() {
        let nasty = "printf '%s' 'x\\';rm -rf ~;#' \"$HOME\" `id` !! \n\t%%";
        let w = portable(nasty);
        let inner = w
            .strip_prefix("exec /bin/sh -c '")
            .and_then(|r| r.strip_suffix('\''))
            .unwrap_or_else(|| panic!("shape: {w}"));
        assert!(!inner.contains('\''), "{w}");
        assert!(!inner.contains("\\\\"), "{w}");
        assert!(!inner.contains('!'), "{w}");
        assert!(!inner.contains('\n') && !inner.contains('\r'), "{w}");
        // Every backslash starts an octal escape (fish keeps `\1` as is).
        for (i, _) in inner.match_indices('\\') {
            assert!(inner.as_bytes()[i + 1].is_ascii_digit(), "{w}");
        }
    }

    /// The wrapped script runs the same whichever shell receives it.
    #[cfg(unix)]
    #[test]
    fn portable_runs_in_every_installed_shell() {
        let weird = "it's \\'; echo INJECTED; #\n%s $HOME `id` \"q\" ünï ✓\t!";
        let script = format!("printf '%s|' {}; exit 3", quote(weird));
        let wrapped = portable(&script);
        let mut ran = 0;
        for shell in [
            &["sh"][..],
            &["bash"],
            &["zsh", "-f"],
            &["busybox", "sh"],
            &["dash"],
            &["fish"],
            &["tcsh"],
        ] {
            let Ok(out) = std::process::Command::new(shell[0])
                .args(&shell[1..])
                .arg("-c")
                .arg(&wrapped)
                .output()
            else {
                eprintln!("{} not installed; skipping", shell[0]);
                continue;
            };
            ran += 1;
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                format!("{weird}|"),
                "{shell:?}: stderr {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.status.code(), Some(3), "{shell:?}");
        }
        assert!(ran > 0);
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
