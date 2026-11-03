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
    let len = BODY.replace("@ROWS@", "000").chars().count();
    let rows = rows_used(cursor_col, len, cols);
    BODY.replace("@ROWS@", &format!("{rows:03}"))
}

/// The single line typed into the shell. Besides OSC 7 (folder) it emits
/// OSC 133 marks: `D;<exit status>` when a command ends, `A` when the
/// prompt is drawn and `C;cmd=<text>` when a command starts (zsh: from
/// `preexec`; bash 4.4+: from `PS0`, with the text from `history 1`).
const BODY: &str = r#" if [ -n "${ZSH_VERSION-}" ]; then __hw_osc7() { local e=$?; printf '\033]133;D;%s\007\033]7;file://%s%s\033\\\033]133;A\007' "$e" "${HOST-}" "${${PWD//\%/%25}// /%20}"; }; __hw_pre() { printf '\033]133;C;cmd=%s\007' "${1//[[:cntrl:]]/ }"; }; autoload -Uz add-zsh-hook && add-zsh-hook precmd __hw_osc7 && add-zsh-hook preexec __hw_pre; elif [ -n "${BASH_VERSION-}" ]; then __hw_osc7() { local e=$? p="${PWD//\%/%25}"; printf '\033]133;D;%s\007\033]7;file://%s%s\033\\\033]133;A\007' "$e" "${HOSTNAME-}" "${p// /%20}"; }; __hw_cmd() { local h; read -r _ h <<< "$(HISTTIMEFORMAT= history 1)"; printf %s "${h//[[:cntrl:]]/ }"; }; case ";${PROMPT_COMMAND-};" in *";__hw_osc7;"*) ;; *) PROMPT_COMMAND="__hw_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}";; esac; case "${PS0-}" in *__hw_cmd*) ;; *) PS0='\e]133;C;cmd=$(__hw_cmd)\a'"${PS0-}";; esac; fi; printf '\033[%sA\r\033[J' @ROWS@"#;

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
            let script = format!("{prior}\n{cmd}\n{cmd}\nfalse\ncd '{}'\nexit\n", sub.display());
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
            // OSC 133: the failed command's exit status, the prompt mark and
            // the start of the command with its text.
            assert!(all.contains("\x1b]133;D;1\x07"), "{shell}: no exit status: {all:?}");
            assert!(all.contains("\x1b]133;A\x07"), "{shell}: no prompt mark: {all:?}");
            assert!(
                all.contains("\x1b]133;C;cmd=false\x07"),
                "{shell}: no command start: {all:?}"
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
