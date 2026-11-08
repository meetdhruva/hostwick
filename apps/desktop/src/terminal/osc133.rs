//! OSC 133 "semantic prompt" marks (`ESC ] 133 ; A|B|C|D ... BEL`): the shell
//! telling the terminal where prompts and commands start and end. Hostwick's
//! shell integration emits `D;<exit>` (command ended), `A` (prompt drawn)
//! and `C;cmd=<text>` (command started). Used to tell when a long command
//! has finished.
//!
//! Like the OSC 7 scanner, this one scans the raw output before the
//! terminal parser sees it, keeps its state between chunks and skips plain
//! text with a byte search for ESC.

use std::time::{Duration, Instant};

/// Longest OSC payload that is kept (anything longer is not ours).
const MAX: usize = 1024;

/// One mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// `A`: the prompt is about to be drawn.
    PromptStart,
    /// `B`: the prompt is drawn, input starts.
    InputStart,
    /// `C`: the command starts running. Carries its text when known.
    CommandStart { command: Option<String> },
    /// `D`: the command ended.
    CommandEnd { exit: Option<i32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum St {
    #[default]
    Ground,
    Esc,
    /// Inside `ESC ]`, collecting the payload.
    Osc,
    /// ESC inside the payload (expecting `\`).
    OscEsc,
}

#[derive(Debug, Default)]
pub struct Osc133Scanner {
    st: St,
    buf: Vec<u8>,
    /// Payload too long: skip until the end.
    overflow: bool,
}

impl Osc133Scanner {
    /// Scan a chunk of output; returns the marks found, in order.
    pub fn feed(&mut self, mut bytes: &[u8]) -> Vec<Mark> {
        let mut out = Vec::new();
        while !bytes.is_empty() {
            if self.st == St::Ground {
                match bytes.iter().position(|&b| b == 0x1b) {
                    Some(i) => {
                        self.st = St::Esc;
                        bytes = &bytes[i + 1..];
                        continue;
                    }
                    None => return out,
                }
            }
            let b = bytes[0];
            bytes = &bytes[1..];
            self.st = match (self.st, b) {
                (St::Esc | St::OscEsc, b']') => {
                    self.buf.clear();
                    self.overflow = false;
                    St::Osc
                }
                (St::Osc, 0x07) => {
                    self.finish(&mut out);
                    St::Ground
                }
                (St::Osc, 0x1b) => St::OscEsc,
                (St::OscEsc, b'\\') => {
                    self.finish(&mut out);
                    St::Ground
                }
                (St::Osc, b) => {
                    if self.buf.len() < MAX {
                        self.buf.push(b);
                    } else {
                        self.overflow = true;
                    }
                    St::Osc
                }
                // ESC starts a new sequence wherever we were.
                (_, 0x1b) => St::Esc,
                _ => St::Ground,
            };
        }
        out
    }

    fn finish(&mut self, out: &mut Vec<Mark>) {
        if !self.overflow
            && let Some(m) = parse(&self.buf)
        {
            out.push(m);
        }
        self.buf.clear();
    }
}

/// The payload after `ESC ]`, e.g. `133;D;1`.
pub fn parse(payload: &[u8]) -> Option<Mark> {
    let s = std::str::from_utf8(payload).ok()?;
    let rest = s.strip_prefix("133;")?;
    let mut parts = rest.splitn(2, ';');
    let kind = parts.next()?;
    let args = parts.next().unwrap_or("");
    match kind {
        "A" => Some(Mark::PromptStart),
        "B" => Some(Mark::InputStart),
        "C" => Some(Mark::CommandStart {
            command: command_text(args),
        }),
        "D" => {
            let exit = args
                .split(';')
                .next()
                .and_then(|a| a.trim().parse::<i32>().ok());
            Some(Mark::CommandEnd { exit })
        }
        _ => None,
    }
}

/// `cmd=<text>` (the text runs to the end, `;` included), trimmed and
/// limited to a readable length.
fn command_text(args: &str) -> Option<String> {
    let i = args.find("cmd=")?;
    let text: String = args[i + 4..]
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut out: String = text.chars().take(120).collect();
    if text.chars().count() > 120 {
        out.push('…');
    }
    Some(out)
}

/// A command that has finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub command: Option<String>,
    pub exit: Option<i32>,
    pub took: Duration,
}

/// Follows the marks of one terminal and reports finished commands.
///
/// A command is "running" from `C` (or, for shells that do not send `C`,
/// from the Enter the user typed) until `D`.
#[derive(Debug, Default)]
pub struct CommandTracker {
    running: Option<Running>,
    /// A mark has arrived: the shell reports prompts, so the Enter key is
    /// worth timing. Without it, the first `D` after the integration is
    /// injected would report a command that started long ago.
    active: bool,
}

#[derive(Debug)]
struct Running {
    since: Instant,
    command: Option<String>,
}

impl CommandTracker {
    /// The user pressed Enter.
    pub fn entered(&mut self, now: Instant) {
        if self.active && self.running.is_none() {
            self.running = Some(Running {
                since: now,
                command: None,
            });
        }
    }

    /// A mark arrived. Returns the command when this mark ended one.
    pub fn mark(&mut self, mark: Mark, now: Instant) -> Option<Finished> {
        self.active = true;
        match mark {
            Mark::CommandStart { command } => {
                self.running = Some(Running {
                    since: now,
                    command,
                });
                None
            }
            Mark::CommandEnd { exit } => {
                let r = self.running.take()?;
                Some(Finished {
                    command: r.command,
                    exit,
                    took: now.saturating_duration_since(r.since),
                })
            }
            Mark::PromptStart | Mark::InputStart => None,
        }
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

/// A command line as shown in the app (the in-app toast), never in an
/// operating-system notification: control and invisible formatting
/// characters become spaces, runs of spaces one, and it is cut at `max`
/// characters. Commands often hold secrets (`mysql -pSECRET`,
/// `export TOKEN=…`), and notifications show on lock screens and in
/// notification history.
pub fn display_command(command: &str, max: usize) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in command.trim().chars() {
        if c.is_control() || c.is_whitespace() || hostwick_term::xfer::is_invisible_format(c) {
            gap = !out.is_empty();
            continue;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        out.push(c);
    }
    if out.chars().count() > max {
        let mut cut: String = out.chars().take(max.saturating_sub(1)).collect();
        cut.push('…');
        return cut;
    }
    out
}

/// `2m 13s`, `45s`, `1h 02m`, `0.4s`.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs == 0 {
        return format!("{:.1}s", d.as_secs_f32().max(0.1));
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commands_are_shown_short_and_clean() {
        use super::display_command as d;
        assert_eq!(d("ls -la", 80), "ls -la");
        assert_eq!(
            d("  echo\t\u{1b}[31mhi \u{202e}there\n", 80),
            "echo [31mhi there"
        );
        assert_eq!(d(&"x".repeat(200), 10), "xxxxxxxxx…");
        assert_eq!(d("", 10), "");
        assert_eq!(d("\u{7}\u{7}", 10), "");
    }

    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<Mark> {
        let mut sc = Osc133Scanner::default();
        chunks.iter().flat_map(|c| sc.feed(c)).collect()
    }

    #[test]
    fn parses_marks_with_both_terminators() {
        assert_eq!(
            scan(&[b"x\x1b]133;D;0\x07y\x1b]133;A\x1b\\z\x1b]133;B\x07"]),
            vec![
                Mark::CommandEnd { exit: Some(0) },
                Mark::PromptStart,
                Mark::InputStart
            ]
        );
        assert_eq!(
            scan(&[b"\x1b]133;C;cmd=make build -j 4; echo done\x07"]),
            vec![Mark::CommandStart {
                command: Some("make build -j 4; echo done".into())
            }]
        );
        assert_eq!(
            scan(&[b"\x1b]133;D\x07\x1b]133;D;130;extra\x07\x1b]133;D;x\x07"]),
            vec![
                Mark::CommandEnd { exit: None },
                Mark::CommandEnd { exit: Some(130) },
                Mark::CommandEnd { exit: None }
            ]
        );
        // No command text: None, and control characters are blanked.
        assert_eq!(
            scan(&[b"\x1b]133;C\x07\x1b]133;C;cmd=\x07\x1b]133;C;cmd=a\tb\x07"]),
            vec![
                Mark::CommandStart { command: None },
                Mark::CommandStart { command: None },
                Mark::CommandStart {
                    command: Some("a b".into())
                }
            ]
        );
    }

    #[test]
    fn works_across_chunk_boundaries_and_ignores_other_oscs() {
        assert_eq!(
            scan(&[b"\x1b", b"]13", b"3;D;", b"2", b"\x07"]),
            vec![Mark::CommandEnd { exit: Some(2) }]
        );
        assert_eq!(
            scan(&[b"\x1b]133;D;7\x1b", b"\\"]),
            vec![Mark::CommandEnd { exit: Some(7) }]
        );
        assert!(scan(&[b"\x1b]0;title\x07\x1b]7;file:///x\x07\x1b[31mred\x1b[0m"]).is_empty());
        assert!(scan(&[b"\x1b]1330;D;1\x07\x1b]13;D\x07"]).is_empty());
        // An interrupted sequence does not swallow the next one.
        assert_eq!(
            scan(&[b"\x1b]133;D;\x1b]133;A\x07"]),
            vec![Mark::PromptStart]
        );
    }

    #[test]
    fn huge_payloads_are_dropped() {
        let mut big = b"\x1b]133;C;cmd=".to_vec();
        big.extend(std::iter::repeat_n(b'a', 5000));
        big.push(0x07);
        big.extend_from_slice(b"\x1b]133;A\x07");
        assert_eq!(scan(&[&big]), vec![Mark::PromptStart]);
    }

    #[test]
    fn long_command_text_is_shortened() {
        let mut p = b"\x1b]133;C;cmd=".to_vec();
        p.extend(std::iter::repeat_n(b'x', 300));
        p.push(0x07);
        let m = scan(&[&p]);
        let Some(Mark::CommandStart { command: Some(c) }) = m.first() else {
            panic!("{m:?}");
        };
        assert_eq!(c.chars().count(), 121);
        assert!(c.ends_with('…'));
    }

    #[test]
    fn tracker_reports_duration_and_command() {
        let t0 = Instant::now();
        let mut t = CommandTracker::default();
        assert_eq!(t.mark(Mark::CommandEnd { exit: Some(0) }, t0), None);
        t.entered(t0);
        // C replaces the Enter-based start.
        t.mark(
            Mark::CommandStart {
                command: Some("make build".into()),
            },
            t0 + Duration::from_secs(1),
        );
        assert!(t.is_running());
        // A second Enter while running changes nothing.
        t.entered(t0 + Duration::from_secs(50));
        let f = t.mark(
            Mark::CommandEnd { exit: Some(2) },
            t0 + Duration::from_secs(134),
        );
        assert_eq!(
            f,
            Some(Finished {
                command: Some("make build".into()),
                exit: Some(2),
                took: Duration::from_secs(133)
            })
        );
        assert!(!t.is_running());
    }

    #[test]
    fn tracker_without_c_times_from_enter() {
        let t0 = Instant::now();
        let mut t = CommandTracker::default();
        // Before the shell has sent any mark, Enter is not timed.
        t.entered(t0);
        assert_eq!(
            t.mark(
                Mark::CommandEnd { exit: Some(0) },
                t0 + Duration::from_secs(5)
            ),
            None
        );
        t.mark(Mark::PromptStart, t0);
        t.entered(t0);
        let f = t.mark(
            Mark::CommandEnd { exit: Some(0) },
            t0 + Duration::from_secs(12),
        );
        assert_eq!(
            f.map(|f| (f.command, f.took)),
            Some((None, Duration::from_secs(12)))
        );
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(format_duration(Duration::from_millis(400)), "0.4s");
        assert_eq!(format_duration(Duration::from_millis(0)), "0.1s");
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(133)), "2m 13s");
        assert_eq!(format_duration(Duration::from_secs(3720)), "1h 02m");
    }
}
