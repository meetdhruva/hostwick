//! OSC 7 (`ESC ] 7 ; file://host/path BEL`): the shell telling the terminal
//! its current folder. Alacritty's parser ignores it, so the raw output is
//! scanned for it before parsing. The scanner keeps its state between
//! chunks, so a sequence split across network packets is still found, and it
//! skips plain text with a fast byte search for ESC.

/// Longest OSC 7 payload we accept (paths longer than this are ignored).
const MAX: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum St {
    #[default]
    Ground,
    /// Saw ESC.
    Esc,
    /// Saw `ESC ]`.
    Osc,
    /// Saw `ESC ] 7`.
    Osc7,
    /// Collecting the payload after `ESC ] 7 ;`.
    Payload,
    /// Saw ESC inside the payload (expecting `\` to finish).
    PayloadEsc,
}

#[derive(Debug, Default)]
pub struct Osc7Scanner {
    st: St,
    buf: Vec<u8>,
}

impl Osc7Scanner {
    /// Scan a chunk of output; returns the last folder reported in it.
    pub fn feed(&mut self, mut bytes: &[u8]) -> Option<String> {
        let mut found = None;
        while !bytes.is_empty() {
            if self.st == St::Ground {
                match bytes.iter().position(|&b| b == 0x1b) {
                    Some(i) => {
                        self.st = St::Esc;
                        bytes = &bytes[i + 1..];
                        continue;
                    }
                    None => return found,
                }
            }
            let b = bytes[0];
            bytes = &bytes[1..];
            self.st = match (self.st, b) {
                (St::Esc, b']') => St::Osc,
                (St::Osc, b'7') => St::Osc7,
                (St::Osc7, b';') => {
                    self.buf.clear();
                    St::Payload
                }
                (St::Payload, 0x07) => {
                    if let Some(p) = parse_url(&self.buf) {
                        found = Some(p);
                    }
                    St::Ground
                }
                (St::Payload, 0x1b) => St::PayloadEsc,
                (St::PayloadEsc, b'\\') => {
                    if let Some(p) = parse_url(&self.buf) {
                        found = Some(p);
                    }
                    St::Ground
                }
                (St::Payload, b) if self.buf.len() < MAX => {
                    self.buf.push(b);
                    St::Payload
                }
                // ESC starts a new sequence wherever we were.
                (_, 0x1b) => St::Esc,
                _ => St::Ground,
            };
        }
        found
    }
}

/// `file://host/some%20dir` → `/some dir`. Also accepts a bare path and
/// `kitty-shell-cwd://host/path`. Windows drive paths (`/C:/x`) lose the
/// leading slash.
pub fn parse_url(raw: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    let rest = if let Some(r) = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("kitty-shell-cwd://"))
    {
        // Skip the host part.
        &r[r.find('/')?..]
    } else if s.starts_with('/') {
        s
    } else {
        return None;
    };
    let path = percent_decode(rest)?;
    let b = path.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return Some(path[1..].to_owned());
    }
    Some(path)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_osc7_with_either_terminator() {
        let mut s = Osc7Scanner::default();
        assert_eq!(
            s.feed(b"prompt\x1b]7;file://web/home/me\x07$ ").as_deref(),
            Some("/home/me")
        );
        assert_eq!(
            s.feed(b"\x1b[1m\x1b]7;file://web/srv/my%20site\x1b\\")
                .as_deref(),
            Some("/srv/my site")
        );
        assert_eq!(s.feed(b"plain text, no escapes"), None);
        // Other OSCs (titles) are not mistaken for OSC 7.
        assert_eq!(s.feed(b"\x1b]0;title\x07\x1b]777;x\x07"), None);
    }

    #[test]
    fn split_across_chunks() {
        let msg = b"ab\x1b]7;file://h/tmp/%C3%A9t%C3%A9\x1b\\cd";
        for cut in 0..msg.len() {
            let mut s = Osc7Scanner::default();
            let a = s.feed(&msg[..cut]);
            let b = s.feed(&msg[cut..]);
            assert_eq!(a.or(b).as_deref(), Some("/tmp/été"), "cut at {cut}");
        }
    }

    #[test]
    fn last_one_wins_and_bad_ones_are_ignored() {
        let mut s = Osc7Scanner::default();
        assert_eq!(
            s.feed(b"\x1b]7;file://h/a\x07\x1b]7;file://h/b\x07")
                .as_deref(),
            Some("/b")
        );
        assert_eq!(s.feed(b"\x1b]7;http://x/y\x07"), None);
        assert_eq!(s.feed(b"\x1b]7;file://h/\xff\xfe\x07"), None);
        // An unterminated, overlong payload is dropped, and scanning recovers.
        let mut long = b"\x1b]7;file://h/".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX * 2));
        assert_eq!(s.feed(&long), None);
        assert_eq!(s.feed(b"\x1b]7;file://h/ok\x07").as_deref(), Some("/ok"));
    }

    #[test]
    fn url_forms() {
        assert_eq!(
            parse_url(b"file:///C:/Users/me").as_deref(),
            Some("C:/Users/me")
        );
        assert_eq!(parse_url(b"/plain/path").as_deref(), Some("/plain/path"));
        assert_eq!(parse_url(b"file://host").as_deref(), None);
        assert_eq!(parse_url(b"file://h/100%").as_deref(), Some("/100%"));
        assert_eq!(
            parse_url(b"kitty-shell-cwd://h/x%25y").as_deref(),
            Some("/x%y")
        );
    }
}
