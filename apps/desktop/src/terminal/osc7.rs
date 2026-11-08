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
    pub fn feed(&mut self, mut bytes: &[u8]) -> Option<Cwd> {
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
                    if let Some(p) = parse_cwd(&self.buf) {
                        found = Some(p);
                    }
                    St::Ground
                }
                (St::Payload, 0x1b) => St::PayloadEsc,
                (St::PayloadEsc, b'\\') => {
                    if let Some(p) = parse_cwd(&self.buf) {
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

/// A folder the shell reported: where it says it is, and on which host.
/// Anything that prints to the terminal can send this, so the host decides
/// whether it is believed (see [`HostCheck`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cwd {
    /// The host part of the URL; `None` for a bare path or `file:///path`.
    pub host: Option<String>,
    pub path: String,
}

/// `file://host/some%20dir` → host `host`, path `/some dir`. Also accepts a
/// bare path and `kitty-shell-cwd://host/path`. Windows drive paths
/// (`/C:/x`) lose the leading slash.
pub fn parse_cwd(raw: &[u8]) -> Option<Cwd> {
    let s = std::str::from_utf8(raw).ok()?.trim();
    let (host, rest) = if let Some(r) = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("kitty-shell-cwd://"))
    {
        let slash = r.find('/')?;
        let host = percent_decode(&r[..slash]).filter(|h| !h.is_empty());
        (host, &r[slash..])
    } else if s.starts_with('/') {
        (None, s)
    } else {
        return None;
    };
    let path = percent_decode(rest)?;
    let b = path.as_bytes();
    let path = if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        path[1..].to_owned()
    } else {
        path
    };
    Some(Cwd { host, path })
}

/// How far a reported folder can be believed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The host in the report is the host this session is connected to.
    Verified,
    /// Usable, but nothing ties it to this host: no host in the report, or
    /// a host name that is only known because the shell said so first.
    Unverified,
    /// Names another host than this session's: ignored. (Output from a file
    /// or another program that wants the file browser or an upload to go
    /// somewhere else.)
    Mismatch,
}

/// Decides which OSC 7 reports are believed for one session. The session's
/// own host name is known from the profile; a server often calls itself
/// something else (an IP address in the profile, `ip-10-0-0-5` on the
/// machine), so the first host the shell reports is remembered and later
/// reports must agree with it. That stops later output from redirecting
/// anything; it cannot tell the first report from a forgery, which is why
/// the report is only [`Trust::Unverified`] then and uploads always ask.
#[derive(Debug, Default, Clone)]
pub struct HostCheck {
    reported: Option<String>,
}

fn same_host(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('.'), b.trim_end_matches('.'));
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    // `web1` and `web1.example.com`: the short name of a machine. Not for
    // IP addresses (10.0.0.5 vs 10.0.0.6 share a first "label" otherwise).
    let ip = |h: &str| h.parse::<std::net::IpAddr>().is_ok() || h.starts_with('[');
    if ip(a) || ip(b) {
        return false;
    }
    let first = |h: &str| h.split('.').next().unwrap_or(h).to_ascii_lowercase();
    (!a.contains('.') || !b.contains('.')) && first(a) == first(b)
}

impl HostCheck {
    /// A new session starts: forget what the last one said.
    pub fn reset(&mut self) {
        self.reported = None;
    }

    /// Judge a report from a session connected to `session_host`.
    pub fn check(&mut self, reported: Option<&str>, session_host: &str) -> Trust {
        let Some(h) = reported.filter(|h| !h.is_empty()) else {
            return Trust::Unverified;
        };
        if same_host(h, session_host) {
            return Trust::Verified;
        }
        match &self.reported {
            None => {
                self.reported = Some(h.to_owned());
                Trust::Unverified
            }
            Some(first) if same_host(first, h) => Trust::Unverified,
            Some(_) => Trust::Mismatch,
        }
    }
}

/// `%XX` escapes decoded; `None` when the result is not UTF-8.
pub fn percent_decode_str(s: &str) -> Option<String> {
    percent_decode(s)
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

    fn parse_path(raw: &[u8]) -> Option<String> {
        parse_cwd(raw).map(|c| c.path)
    }

    #[test]
    fn finds_osc7_with_either_terminator() {
        let mut s = Osc7Scanner::default();
        assert_eq!(
            s.feed(b"prompt\x1b]7;file://web/home/me\x07$ ")
                .map(|c| c.path)
                .as_deref(),
            Some("/home/me")
        );
        let c = s.feed(b"\x1b[1m\x1b]7;file://web/srv/my%20site\x1b\\");
        assert_eq!(c.as_ref().map(|c| c.path.as_str()), Some("/srv/my site"));
        assert_eq!(c.and_then(|c| c.host).as_deref(), Some("web"));
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
            assert_eq!(
                a.or(b).map(|c| c.path).as_deref(),
                Some("/tmp/été"),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn last_one_wins_and_bad_ones_are_ignored() {
        let mut s = Osc7Scanner::default();
        assert_eq!(
            s.feed(b"\x1b]7;file://h/a\x07\x1b]7;file://h/b\x07")
                .map(|c| c.path)
                .as_deref(),
            Some("/b")
        );
        assert_eq!(s.feed(b"\x1b]7;http://x/y\x07"), None);
        assert_eq!(s.feed(b"\x1b]7;file://h/\xff\xfe\x07"), None);
        // An unterminated, overlong payload is dropped, and scanning recovers.
        let mut long = b"\x1b]7;file://h/".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX * 2));
        assert_eq!(s.feed(&long), None);
        assert_eq!(
            s.feed(b"\x1b]7;file://h/ok\x07").map(|c| c.path).as_deref(),
            Some("/ok")
        );
    }

    #[test]
    fn url_forms() {
        assert_eq!(
            parse_path(b"file:///C:/Users/me").as_deref(),
            Some("C:/Users/me")
        );
        assert_eq!(parse_path(b"/plain/path").as_deref(), Some("/plain/path"));
        assert_eq!(parse_path(b"file://host").as_deref(), None);
        assert_eq!(parse_path(b"file://h/100%").as_deref(), Some("/100%"));
        assert_eq!(
            parse_path(b"kitty-shell-cwd://h/x%25y").as_deref(),
            Some("/x%y")
        );
    }

    #[test]
    fn the_host_part_is_kept() {
        let c = parse_cwd(b"file://Web1.Example.com/srv").unwrap_or_else(|| panic!("c"));
        assert_eq!(c.host.as_deref(), Some("Web1.Example.com"));
        assert_eq!(c.path, "/srv");
        assert_eq!(parse_cwd(b"file:///srv").map(|c| c.host), Some(None));
        assert_eq!(parse_cwd(b"/srv").map(|c| c.host), Some(None));
        assert_eq!(
            parse_cwd(b"file://h%61/x").and_then(|c| c.host).as_deref(),
            Some("ha")
        );
    }

    #[test]
    fn reports_for_another_host_are_ignored() {
        let mut c = HostCheck::default();
        // Matches what the profile says (case, trailing dot, short name).
        assert_eq!(c.check(Some("web1"), "web1"), Trust::Verified);
        assert_eq!(c.check(Some("WEB1"), "web1"), Trust::Verified);
        assert_eq!(c.check(Some("web1.example.com"), "web1"), Trust::Verified);
        assert_eq!(c.check(Some("web1"), "web1.example.com."), Trust::Verified);
        // A different host while the profile names this one, and nothing was
        // reported before: believed once (remembered), as unverified.
        let mut c = HostCheck::default();
        assert_eq!(c.check(Some("ip-10-0-0-5"), "10.0.0.5"), Trust::Unverified);
        assert_eq!(c.check(Some("ip-10-0-0-5"), "10.0.0.5"), Trust::Unverified);
        // After that another host name is ignored.
        assert_eq!(c.check(Some("attacker"), "10.0.0.5"), Trust::Mismatch);
        assert_eq!(c.check(Some("db1"), "10.0.0.5"), Trust::Mismatch);
        // The remembered one still works; the profile's own name too.
        assert_eq!(c.check(Some("ip-10-0-0-5"), "10.0.0.5"), Trust::Unverified);
        assert_eq!(c.check(Some("10.0.0.5"), "10.0.0.5"), Trust::Verified);
        // No host in the report: usable, unverified.
        assert_eq!(c.check(None, "10.0.0.5"), Trust::Unverified);
        assert_eq!(c.check(Some(""), "10.0.0.5"), Trust::Unverified);
        // A new session forgets the remembered name.
        c.reset();
        assert_eq!(c.check(Some("db1"), "10.0.0.5"), Trust::Unverified);
    }

    #[test]
    fn ip_addresses_are_compared_whole() {
        let mut c = HostCheck::default();
        assert_eq!(c.check(Some("10.0.0.6"), "10.0.0.5"), Trust::Unverified);
        assert_eq!(c.check(Some("10.0.0.7"), "10.0.0.5"), Trust::Mismatch);
        // Two different FQDNs sharing a first label are different hosts.
        let mut c = HostCheck::default();
        assert_eq!(
            c.check(Some("web1.evil.com"), "web1.example.com"),
            Trust::Unverified
        );
        assert_eq!(
            c.check(Some("web1.other.org"), "web1.example.com"),
            Trust::Mismatch
        );
    }
}
