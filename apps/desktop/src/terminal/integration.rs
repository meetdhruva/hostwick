//! Shell integration: a one-time command typed into bash or zsh that makes
//! the shell report its folder with OSC 7 before every prompt.
//!
//! - Existing hooks are kept: bash's `PROMPT_COMMAND` is prefixed (an array
//!   `PROMPT_COMMAND` keeps its other elements), zsh uses `add-zsh-hook`.
//!   Running it twice installs the hook once.
//! - It starts with a space, so shells with `HISTCONTROL=ignorespace`/
//!   `ignoreboth` (bash's default on Debian/Ubuntu) or zsh's
//!   `HIST_IGNORE_SPACE` keep it out of history.
//! - It ends by moving the cursor back up over the prompt line and the echo
//!   of the command itself and erasing them, so the next prompt appears
//!   where the old one was and nothing is left on screen.
//! - `%` and spaces in the path are percent-encoded (the rest is sent as
//!   is; the parser decodes `%XX`).

/// The name of the prompt hook. The shell's echo of the setup command
/// contains it; that line is erased on screen and is left out of what
/// screen readers hear too.
pub const HOOK: &str = "__hw_osc7";

/// The command as typed, without the final Enter. `cursor_col` is where
/// the cursor sits after the prompt (0-based) and `cols` the terminal
/// width: together they tell how many screen rows prompt + command take.
pub fn setup_command(cursor_col: usize, cols: usize) -> String {
    let body = |rows: usize| {
        format!(
            r#" if [ -n "${{ZSH_VERSION-}}" ]; then __hw_osc7() {{ printf '\033]7;file://%s%s\033\\' "${{HOST-}}" "${{${{PWD//\%/%25}}// /%20}}"; }}; autoload -Uz add-zsh-hook && add-zsh-hook precmd __hw_osc7; elif [ -n "${{BASH_VERSION-}}" ]; then __hw_osc7() {{ local p="${{PWD//\%/%25}}"; printf '\033]7;file://%s%s\033\\' "${{HOSTNAME-}}" "${{p// /%20}}"; }}; case ";${{PROMPT_COMMAND-}};" in *";__hw_osc7;"*) ;; *) PROMPT_COMMAND="__hw_osc7${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}";; esac; fi; printf '\033[%sA\r\033[J' {rows:03}"#
        )
    };
    // The length does not depend on the (zero-padded) row count.
    let len = body(0).chars().count();
    body(rows_used(cursor_col, len, cols))
}

/// Screen rows taken by a line that starts at `col` and is `len` cells long
/// (the terminal wraps at `cols`; a line ending exactly at the margin does
/// not take an extra row).
pub fn rows_used(col: usize, len: usize, cols: usize) -> usize {
    let cols = cols.max(1);
    let end = col + len;
    end.div_ceil(cols).clamp(1, 999)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_counting() {
        assert_eq!(rows_used(0, 0, 80), 1);
        assert_eq!(rows_used(2, 10, 80), 1);
        assert_eq!(rows_used(10, 70, 80), 1);
        assert_eq!(rows_used(10, 71, 80), 2);
        assert_eq!(rows_used(20, 300, 80), 4);
    }

    #[test]
    fn command_shape() {
        let c = setup_command(20, 80);
        assert!(
            c.starts_with(" if "),
            "leading space keeps it out of history"
        );
        let rows = rows_used(20, c.chars().count(), 80);
        assert!(c.ends_with(&format!("' {rows:03}")), "{c}");
        assert!(!c.contains('\n'));
        assert!(c.contains(HOOK));
    }

    /// Run the command in real shells (when installed) and check that the
    /// prompt hook reports the folder, percent-encoded, and that existing
    /// hooks survive.
    #[cfg(unix)]
    #[test]
    fn works_in_bash_and_zsh() {
        use std::process::{Command, Stdio};
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let sub = dir.path().join("my dir%");
        std::fs::create_dir_all(&sub).unwrap_or_else(|e| panic!("{e}"));
        let cmd = setup_command(0, 80);
        for (shell, args, prior) in [
            (
                "bash",
                &["--norc", "--noprofile", "-i"][..],
                "PROMPT_COMMAND='echo prior-hook'",
            ),
            ("zsh", &["-f", "-i"][..], "precmd() { echo prior-hook; }"),
        ] {
            let Ok(mut child) = Command::new(shell)
                .args(args)
                .env("PS1", "$ ")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            else {
                eprintln!("{shell} not installed; skipping");
                continue;
            };
            let script = format!("{prior}\n{cmd}\n{cmd}\ncd '{}'\nexit\n", sub.display());
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write as _;
                let _ = stdin.write_all(script.as_bytes());
            }
            let out = child.wait_with_output().unwrap_or_else(|e| panic!("{e}"));
            let all = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let want = "/my%20dir%25\x1b\\";
            assert!(
                all.contains("\x1b]7;file://") && all.contains(want),
                "{shell}: no OSC 7 for the folder in {all:?}"
            );
            let reported = all.matches(want).count();
            assert_eq!(reported, 1, "{shell}: hook installed twice? {all:?}");
            assert!(
                all.contains("prior-hook"),
                "{shell}: existing hook lost: {all:?}"
            );
        }
    }
}
