//! Paste protection: when a paste is confirmed first, and what the
//! confirmation shows.
//!
//! A newline in pasted text runs the line before it, so a multi-line paste
//! (a script copied from a web page, with a surprise in it) deserves a look
//! first. Production hosts always ask; for the others a setting decides.

use serde::{Deserialize, Serialize};

/// Pastes longer than this many bytes are confirmed even without newlines.
pub const LARGE: usize = 8 * 1024;

/// Settings → Terminal → "Confirm multi-line paste".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteConfirm {
    /// Every multi-line or large paste.
    #[default]
    Always,
    /// Only on production hosts (where it is always on).
    ProdOnly,
    /// Not on other hosts.
    Never,
}

impl PasteConfirm {
    pub const ALL: [PasteConfirm; 3] = [Self::Always, Self::ProdOnly, Self::Never];

    pub fn key(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::ProdOnly => "prod_only",
            Self::Never => "never",
        }
    }
}

/// Whether `text` is risky to paste without a look.
pub fn is_risky(text: &str) -> bool {
    text.len() > LARGE || has_newline(text)
}

/// A newline anywhere counts, a trailing one too: it runs the command.
pub fn has_newline(text: &str) -> bool {
    text.contains('\n') || text.contains('\r')
}

/// Ask before pasting `text`?
pub fn needs_confirm(text: &str, mode: PasteConfirm, prod: bool) -> bool {
    if !is_risky(text) {
        return false;
    }
    prod || mode == PasteConfirm::Always
}

/// The text as one line: line breaks become single spaces, a trailing
/// backslash (line continuation) goes, empty lines are dropped.
pub fn single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .map(|l| l.strip_suffix('\\').map_or(l, str::trim_end))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the dialog shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The first lines, each cut to a readable width.
    pub lines: Vec<String>,
    /// How many more lines there are.
    pub more_lines: usize,
    pub total_lines: usize,
    pub bytes: usize,
}

/// First `max_lines` lines, each at most `max_cols` characters. Control
/// characters show as a middle dot so an escape sequence is visible.
pub fn preview(text: &str, max_lines: usize, max_cols: usize) -> Preview {
    let all: Vec<&str> = text.lines().collect();
    let total_lines = all.len().max(1);
    let lines = all
        .iter()
        .take(max_lines)
        .map(|l| {
            let clean: String = l
                .chars()
                .map(|c| if c.is_control() && c != '\t' { '·' } else { c })
                .collect();
            if clean.chars().count() > max_cols {
                let mut cut: String = clean.chars().take(max_cols.saturating_sub(1)).collect();
                cut.push('…');
                cut
            } else {
                clean
            }
        })
        .collect();
    Preview {
        lines,
        more_lines: total_lines.saturating_sub(max_lines),
        total_lines,
        bytes: text.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        use PasteConfirm::*;
        // Plain single lines never ask.
        for prod in [false, true] {
            for mode in PasteConfirm::ALL {
                assert!(!needs_confirm("ls -la", mode, prod));
                assert!(!needs_confirm("", mode, prod));
            }
        }
        let multi = "a\nb";
        assert!(needs_confirm(multi, Always, false));
        assert!(!needs_confirm(multi, ProdOnly, false));
        assert!(!needs_confirm(multi, Never, false));
        // Production always asks.
        for mode in PasteConfirm::ALL {
            assert!(needs_confirm(multi, mode, true));
        }
        // A trailing newline runs the command: it counts.
        assert!(needs_confirm("rm -rf x\n", Always, false));
        assert!(needs_confirm("a\r\nb", Always, false));
        assert!(needs_confirm("a\rb", Always, false));
        // Large single lines.
        let big = "x".repeat(LARGE + 1);
        assert!(needs_confirm(&big, Always, false));
        assert!(!needs_confirm(&big, Never, false));
        assert!(needs_confirm(&big, Never, true));
        assert!(!needs_confirm(&"x".repeat(LARGE), Always, false));
    }

    #[test]
    fn single_line_joins() {
        assert_eq!(single_line("a\nb\r\n\n  c  \n"), "a b c");
        assert_eq!(single_line("apt update \\\n && apt upgrade"), "apt update && apt upgrade");
        assert_eq!(single_line("one"), "one");
        assert_eq!(single_line("\n\n"), "");
        assert!(!has_newline(&single_line("a\nb\n")));
    }

    #[test]
    fn previews() {
        let p = preview("one\ntwo\nthree\nfour", 2, 10);
        assert_eq!(p.lines, vec!["one", "two"]);
        assert_eq!((p.more_lines, p.total_lines, p.bytes), (2, 4, 18));
        let p = preview("abcdefghijklmnop", 5, 8);
        assert_eq!(p.lines, vec!["abcdefg…"]);
        assert_eq!(p.more_lines, 0);
        let p = preview("echo \x1b[31mhi\x07", 5, 40);
        assert_eq!(p.lines, vec!["echo ·[31mhi·"]);
        assert_eq!(preview("", 3, 10).total_lines, 1);
    }

    #[test]
    fn mode_round_trips() {
        for m in PasteConfirm::ALL {
            let j = serde_json::to_string(&m).unwrap_or_default();
            assert_eq!(j, format!("\"{}\"", m.key()));
            assert_eq!(serde_json::from_str::<PasteConfirm>(&j).ok(), Some(m));
        }
        assert_eq!(PasteConfirm::default(), PasteConfirm::Always);
    }
}
