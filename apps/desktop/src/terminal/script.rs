//! Login scripts and the output watcher they wait on.
//!
//! The parser feeds every output chunk to [`Watch`], which keeps the last
//! stretch of *text* (escape sequences and control characters removed, by a
//! stripper that keeps its state between chunks, so `ESC [ 3` + `2 m` split
//! over two packets is still removed). `@wait` steps search that text.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hostwick_core::ScriptStep;
use tokio::sync::Notify;

/// Text kept for `@wait` to search (older text is dropped).
const KEEP: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Strip {
    #[default]
    Text,
    Esc,
    /// CSI parameters, until a final byte 0x40..=0x7e.
    Csi,
    /// OSC / DCS / APC / PM / SOS string, until BEL or ST.
    Str,
    StrEsc,
    /// One more byte (e.g. `ESC ( B` charset selection).
    Skip1,
}

/// Removes escape sequences and control characters (keeping `\n` and
/// `\t`) from a byte stream.
#[derive(Debug, Default)]
pub struct Stripper {
    st: Strip,
}

impl Stripper {
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            self.st = match (self.st, b) {
                (Strip::Text, 0x1b) => Strip::Esc,
                (Strip::Text, b'\n' | b'\t') => {
                    out.push(b);
                    Strip::Text
                }
                (Strip::Text, 0x00..=0x1f | 0x7f) => Strip::Text,
                (Strip::Text, _) => {
                    out.push(b);
                    Strip::Text
                }
                (Strip::Esc, b'[') => Strip::Csi,
                (Strip::Esc, b']' | b'P' | b'_' | b'^' | b'X') => Strip::Str,
                (Strip::Esc, b'(' | b')' | b'*' | b'+' | b'#' | b' ' | b'%') => Strip::Skip1,
                (Strip::Esc, _) => Strip::Text,
                (Strip::Csi, 0x40..=0x7e) => Strip::Text,
                (Strip::Csi, 0x1b) => Strip::Esc,
                (Strip::Csi, _) => Strip::Csi,
                (Strip::Str, 0x07) => Strip::Text,
                (Strip::Str, 0x1b) => Strip::StrEsc,
                (Strip::Str, _) => Strip::Str,
                (Strip::StrEsc, b'\\') => Strip::Text,
                (Strip::StrEsc, _) => Strip::Str,
                (Strip::Skip1, _) => Strip::Text,
            };
        }
    }
}

#[derive(Debug, Default)]
struct Recent {
    stripper: Stripper,
    /// Absolute offset (in stripped bytes since the session began) of
    /// `text[0]`.
    start: u64,
    text: Vec<u8>,
    last_output: Option<Instant>,
}

impl Recent {
    fn end(&self) -> u64 {
        self.start + self.text.len() as u64
    }
}

/// Recent output text plus a wake-up for anyone waiting on it.
#[derive(Debug, Default)]
pub struct Watch {
    recent: Mutex<Recent>,
    notify: Notify,
    closed: AtomicBool,
}

impl Watch {
    /// Called by the parser for every output chunk.
    pub fn push(&self, bytes: &[u8]) {
        if let Ok(mut r) = self.recent.lock() {
            let r = &mut *r;
            r.stripper.feed(bytes, &mut r.text);
            r.last_output = Some(Instant::now());
            if r.text.len() > 2 * KEEP {
                let drop = r.text.len() - KEEP;
                r.text.drain(..drop);
                r.start += drop as u64;
            }
        }
        self.notify.notify_waiters();
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Offset just past the newest text.
    pub fn end(&self) -> u64 {
        self.recent.lock().map(|r| r.end()).unwrap_or(0)
    }

    /// Where `needle` ends if it occurs at or after offset `from`.
    pub fn find(&self, from: u64, needle: &str) -> Option<u64> {
        let r = self.recent.lock().ok()?;
        let skip = from.saturating_sub(r.start) as usize;
        let hay = r.text.get(skip.min(r.text.len())..)?;
        let n = needle.as_bytes();
        if n.is_empty() {
            return Some(r.end());
        }
        hay.windows(n.len())
            .position(|w| w == n)
            .map(|i| r.start + (skip + i + n.len()) as u64)
    }

    /// Time since the last output, or None if there was none yet.
    pub fn idle(&self) -> Option<Duration> {
        self.recent.lock().ok()?.last_output.map(|t| t.elapsed())
    }

    /// Wait (up to `max`) until there has been output and then `quiet` of
    /// silence — "the shell has printed its prompt". False if the session
    /// ended.
    pub async fn settle(&self, quiet: Duration, max: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + max;
        loop {
            if self.is_closed() {
                return false;
            }
            let wait = match self.idle() {
                Some(idle) if idle >= quiet => return true,
                Some(idle) => quiet - idle,
                None => quiet,
            };
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return true;
            }
            tokio::time::sleep_until((now + wait).min(deadline)).await;
        }
    }

    /// Wait until `needle` appears at or after `from`; returns where it ends.
    async fn wait_for(
        &self,
        from: u64,
        needle: &str,
        timeout: Duration,
    ) -> Result<u64, ScriptError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(end) = self.find(from, needle) {
                return Ok(end);
            }
            if self.is_closed() {
                return Err(ScriptError::Ended);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(ScriptError::Timeout {
                    text: needle.to_owned(),
                    after: timeout,
                });
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    Timeout {
        text: String,
        after: Duration,
    },
    /// The session ended (or input could not be sent) before the script did.
    Ended,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { text, after } => write!(
                f,
                "waited {} for “{text}”, which never appeared",
                human_ms(*after)
            ),
            Self::Ended => f.write_str("the session ended first"),
        }
    }
}

fn human_ms(d: Duration) -> String {
    let ms = d.as_millis();
    if ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{:.1} s", d.as_secs_f64())
    }
}

/// Run script steps. `send` types bytes into the session and returns false
/// once the session is gone. With `from_start`, the first `@wait` also sees
/// output that arrived before the script started (used for login scripts,
/// which start while the banner may already be on screen).
pub async fn run(
    steps: Vec<ScriptStep>,
    watch: &Watch,
    from_start: bool,
    send: impl Fn(Vec<u8>) -> bool,
) -> Result<(), ScriptError> {
    let mut cursor = if from_start { 0 } else { watch.end() };
    for step in steps {
        if watch.is_closed() {
            return Err(ScriptError::Ended);
        }
        match step {
            ScriptStep::Send(text) => {
                cursor = watch.end();
                let mut bytes = text.into_bytes();
                bytes.push(b'\r');
                if !send(bytes) {
                    return Err(ScriptError::Ended);
                }
            }
            ScriptStep::WaitFor { text, timeout_ms } => {
                cursor = watch
                    .wait_for(cursor, &text, Duration::from_millis(timeout_ms))
                    .await?;
            }
            ScriptStep::Sleep(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn strip(chunks: &[&[u8]]) -> String {
        let mut s = Stripper::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn strips_escapes_even_when_split() {
        assert_eq!(
            strip(&[
                b"\x1b[1;3",
                b"2mPass",
                b"word\x1b[0m: \r\n\x1b]0;t\x07$ \x1b(Bok"
            ]),
            "Password: \n$ ok"
        );
        assert_eq!(strip(&[b"a\x1b]2;title\x1b", b"\\b\x07c"]), "abc");
        assert_eq!(strip(&[b"x\x1bPdcs\x1b\\y"]), "xy");
    }

    #[tokio::test]
    async fn waits_send_and_time_out() {
        let watch = Arc::new(Watch::default());
        let sent = Arc::new(Mutex::new(Vec::<u8>::new()));
        watch.push(b"Welcome\r\nlogin: ");
        let w = watch.clone();
        let feeder = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Split escape code and needle across chunks.
            w.push(b"\x1b[1mPass");
            w.push(b"\x1b[0mword: ");
        });
        let s = sent.clone();
        let steps = vec![
            ScriptStep::WaitFor {
                text: "login:".into(),
                timeout_ms: 1000,
            },
            ScriptStep::Send("admin".into()),
            ScriptStep::WaitFor {
                text: "Password:".into(),
                timeout_ms: 2000,
            },
            ScriptStep::Sleep(1),
            ScriptStep::Send("secret".into()),
        ];
        let r = run(steps, &watch, true, move |b| {
            if let Ok(mut v) = s.lock() {
                v.extend(b);
            }
            true
        })
        .await;
        assert_eq!(r, Ok(()));
        assert_eq!(
            sent.lock().map(|v| v.clone()).unwrap_or_default(),
            b"admin\rsecret\r"
        );
        let _ = feeder.await;

        // Text from before a Send does not satisfy a later wait.
        let r = run(
            vec![
                ScriptStep::Send("x".into()),
                ScriptStep::WaitFor {
                    text: "login:".into(),
                    timeout_ms: 100,
                },
            ],
            &watch,
            true,
            |_| true,
        )
        .await;
        assert!(matches!(r, Err(ScriptError::Timeout { .. })), "{r:?}");
        assert!(
            r.err()
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("waited 0.1 s for “login:”")
        );
    }

    #[tokio::test]
    async fn stops_when_the_session_ends() {
        let watch = Arc::new(Watch::default());
        let w = watch.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            w.close();
        });
        let r = run(
            vec![ScriptStep::WaitFor {
                text: "never".into(),
                timeout_ms: 10_000,
            }],
            &watch,
            false,
            |_| true,
        )
        .await;
        assert_eq!(r, Err(ScriptError::Ended));
        let r = run(
            vec![ScriptStep::Send("x".into())],
            &Watch::default(),
            false,
            |_| false,
        )
        .await;
        assert_eq!(r, Err(ScriptError::Ended));
    }

    #[tokio::test]
    async fn settles_after_quiet_output() {
        let watch = Watch::default();
        watch.push(b"$ ");
        let t = Instant::now();
        assert!(
            watch
                .settle(Duration::from_millis(50), Duration::from_secs(2))
                .await
        );
        assert!(t.elapsed() < Duration::from_secs(1));
        // Long-running output beyond `KEEP` is trimmed but offsets still work.
        let big = vec![b'z'; KEEP * 3];
        watch.push(&big);
        watch.push(b"needle");
        assert_eq!(watch.find(0, "needle"), Some(watch.end()));
    }
}
