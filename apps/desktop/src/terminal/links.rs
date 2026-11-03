//! Clickable output: URLs, file paths, IP addresses and host names in the
//! text of the terminal.
//!
//! Detection works on one logical line (rows joined where the program's
//! output wrapped) and only runs for the line under the mouse pointer, so
//! it costs nothing while the mouse is still. Columns are character
//! indexes: the terminal text is built with one character per cell.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    Path,
    Ip,
    Host,
}

/// What was found in a line of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub kind: LinkKind,
    /// Character range in the line.
    pub range: Range<usize>,
    /// The text, without trailing punctuation.
    pub text: String,
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(?:(?:https?|ftp|ftps|sftp|ssh|file)://|www\.)[^\s<>"'`\x00-\x1f\x7f]+"#,
        )
        .unwrap_or_else(|e| panic!("url regex: {e}"))
    })
}

fn path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Absolute, `~/`, `~user/`, `./` and `../` paths. The character
        // before must not be part of a word (so `and/or` and `1/s` are
        // left alone).
        Regex::new(
            r#"(?:^|[\s"'(\[{<=,;>:])((?:~[A-Za-z0-9._-]*|\.{1,2})?(?:/[A-Za-z0-9._@%+~=,\-]+)+/?|~[A-Za-z0-9._-]*/)"#,
        )
        .unwrap_or_else(|e| panic!("path regex: {e}"))
    })
}

fn ipv4_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d{1,5})?\b")
            .unwrap_or_else(|e| panic!("ipv4 regex: {e}"))
    })
}

fn ipv6_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[0-9A-Fa-f:]*:[0-9A-Fa-f:]*:[0-9A-Fa-f.:]*[0-9A-Fa-f]|::1\b")
            .unwrap_or_else(|e| panic!("ipv6 regex: {e}"))
    })
}

fn host_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+([a-z]{2,24})\b")
            .unwrap_or_else(|e| panic!("host regex: {e}"))
    })
}

/// Top-level domains that look like host names rather than file
/// extensions (`.rs`, `.py`, `.md`, `.sh`, `.pl`, `.so` are left out).
const TLDS: &[&str] = &[
    "com", "net", "org", "edu", "gov", "mil", "int", "io", "dev", "app", "cloud", "local", "lan",
    "internal", "intranet", "corp", "home", "localdomain", "example", "test", "uk", "de", "fr",
    "es", "it", "nl", "be", "ch", "at", "se", "no", "dk", "fi", "pt", "ie", "jp", "kr", "cn", "tw",
    "hk", "sg", "au", "nz", "ca", "us", "br", "mx", "za", "ru", "ua", "eu", "info", "biz", "xyz",
    "online", "site", "tech", "ai", "me", "tv",
];

fn char_range(text: &str, byte: Range<usize>) -> Range<usize> {
    let start = text[..byte.start].chars().count();
    let len = text[byte.clone()].chars().count();
    start..start + len
}

/// Remove punctuation that ends a sentence, not the item (`.`, `,`, `)`
/// when unbalanced, quotes…).
fn trim_end(s: &str) -> &str {
    let mut s = s;
    loop {
        let Some(last) = s.chars().last() else {
            return s;
        };
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '>' | '*' => true,
            ')' => s.matches('(').count() < s.matches(')').count(),
            ']' => s.matches('[').count() < s.matches(']').count(),
            '}' => s.matches('{').count() < s.matches('}').count(),
            _ => false,
        };
        if !strip {
            return s;
        }
        s = &s[..s.len() - last.len_utf8()];
    }
}

fn candidates(line: &str) -> Vec<(u8, Found)> {
    let mut out: Vec<(u8, Found)> = Vec::new();
    let mut push = |prio: u8, kind: LinkKind, byte: Range<usize>, raw: &str| {
        let text = trim_end(raw);
        if text.is_empty() {
            return;
        }
        let trimmed = byte.start..byte.start + text.len();
        out.push((
            prio,
            Found {
                kind,
                range: char_range(line, trimmed),
                text: text.to_owned(),
            },
        ));
    };
    for m in url_re().find_iter(line) {
        push(0, LinkKind::Url, m.range(), m.as_str());
    }
    for c in ipv6_re().find_iter(line) {
        let t = trim_end(c.as_str());
        if t.matches(':').count() >= 2 && t.parse::<Ipv6Addr>().is_ok() && t != "::" {
            push(1, LinkKind::Ip, c.range(), c.as_str());
        }
    }
    for m in ipv4_re().find_iter(line) {
        let (host, port) = match m.as_str().split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (m.as_str(), None),
        };
        let port_ok = port.is_none_or(|p| p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n)));
        // `1.2.3.4.5` is a version, not an address.
        let before = line[..m.start()].chars().last();
        let after = line[m.end()..].chars().next();
        let glued = before == Some('.')
            || after == Some('.')
                && line[m.end() + 1..].starts_with(|d: char| d.is_ascii_digit());
        if host.parse::<Ipv4Addr>().is_ok() && port_ok && !glued {
            push(2, LinkKind::Ip, m.range(), m.as_str());
        }
    }
    for c in host_re().captures_iter(line) {
        let (Some(whole), Some(tld)) = (c.get(0), c.get(1)) else {
            continue;
        };
        if TLDS.contains(&tld.as_str().to_ascii_lowercase().as_str()) {
            // Not the tail of a longer word with a path (`a.com/x` is a URL
            // only with a scheme; the host part is still useful).
            push(3, LinkKind::Host, whole.range(), whole.as_str());
        }
    }
    for c in path_re().captures_iter(line) {
        let Some(m) = c.get(1) else { continue };
        let s = m.as_str();
        // A lone `/` or `~/` carries no information except in `~/`.
        let single_slash_word = s.len() < 2 && s != "~";
        if single_slash_word {
            continue;
        }
        push(4, LinkKind::Path, m.range(), s);
    }
    out
}

/// Everything clickable in `line`, left to right, without overlaps (URLs
/// win over addresses over paths).
pub fn detect(line: &str) -> Vec<Found> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    let mut all = candidates(line);
    all.sort_by(|a, b| {
        a.1.range
            .start
            .cmp(&b.1.range.start)
            .then(a.0.cmp(&b.0))
            .then(b.1.range.len().cmp(&a.1.range.len()))
    });
    let mut out: Vec<Found> = Vec::new();
    for (_, f) in all {
        if out.iter().any(|o| f.range.start < o.range.end && o.range.start < f.range.end) {
            continue;
        }
        out.push(f);
    }
    out
}

/// The item under character column `col`, if any.
pub fn at(line: &str, col: usize) -> Option<Found> {
    detect(line).into_iter().find(|f| f.range.contains(&col))
}

/// `https://…` for a `www.` link.
pub fn url_to_open(text: &str) -> String {
    if text.to_ascii_lowercase().starts_with("www.") {
        format!("https://{text}")
    } else {
        text.to_owned()
    }
}

/// An absolute path for a detected path: `./x` and `../x` against the
/// shell's folder, `~/x` against the home folder (guessed from the folder
/// or the login name). `None` when it cannot be known.
pub fn resolve_path(raw: &str, cwd: Option<&str>, user: Option<&str>) -> Option<String> {
    let abs = if raw.starts_with('/') {
        raw.to_owned()
    } else if let Some(rest) = raw.strip_prefix('~') {
        // `~/x` (this user) only; `~other/x` is somebody else's.
        let rest = rest.strip_prefix('/')?;
        let home = home_from(cwd, user)?;
        format!("{}/{}", home.trim_end_matches('/'), rest)
    } else if raw.starts_with("./") || raw.starts_with("../") {
        let base = cwd?;
        format!("{}/{}", base.trim_end_matches('/'), raw)
    } else {
        return None;
    };
    Some(normalize(&abs))
}

fn home_from(cwd: Option<&str>, user: Option<&str>) -> Option<String> {
    if let Some(c) = cwd {
        let parts: Vec<&str> = c.split('/').filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            ["root", ..] => return Some("/root".into()),
            ["home" | "Users", name, ..] => return Some(format!("/{}/{}", parts[0], name)),
            _ => {}
        }
    }
    match user {
        Some("root") => Some("/root".into()),
        Some(u) if !u.is_empty() => Some(format!("/home/{u}")),
        _ => None,
    }
}

/// Collapse `.` and `..` segments and repeated slashes.
pub fn normalize(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            s => stack.push(s),
        }
    }
    let mut out = String::from("/");
    out.push_str(&stack.join("/"));
    if path.ends_with('/') && out.len() > 1 {
        out.push('/');
    }
    out
}

/// `host` or `host:port` for quick connect from a detected address.
pub fn connect_text(found: &Found) -> String {
    found.text.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(LinkKind, String)> {
        detect(line).into_iter().map(|f| (f.kind, f.text)).collect()
    }

    #[test]
    fn urls() {
        assert_eq!(
            kinds("see https://example.com/a/b?x=1&y=2#top for more"),
            vec![(LinkKind::Url, "https://example.com/a/b?x=1&y=2#top".into())]
        );
        // Sentence punctuation and unbalanced brackets are not part of it.
        assert_eq!(
            kinds("(see http://a.io/x), or http://b.io/y."),
            vec![
                (LinkKind::Url, "http://a.io/x".into()),
                (LinkKind::Url, "http://b.io/y".into())
            ]
        );
        assert_eq!(
            kinds("https://en.wikipedia.org/wiki/Rust_(language)"),
            vec![(LinkKind::Url, "https://en.wikipedia.org/wiki/Rust_(language)".into())]
        );
        assert_eq!(kinds("www.example.com")[0].0, LinkKind::Url);
        assert_eq!(url_to_open("www.example.com"), "https://www.example.com");
        assert_eq!(url_to_open("ftp://x.org/f"), "ftp://x.org/f");
        // A URL swallows the path and host inside it.
        assert_eq!(detect("http://10.0.0.1:8080/var/log/x").len(), 1);
    }

    #[test]
    fn paths() {
        assert_eq!(
            kinds("error in /var/log/nginx/error.log: denied"),
            vec![(LinkKind::Path, "/var/log/nginx/error.log".into())]
        );
        assert_eq!(
            kinds("cp ~/notes.txt ./build/out.o ../x"),
            vec![
                (LinkKind::Path, "~/notes.txt".into()),
                (LinkKind::Path, "./build/out.o".into()),
                (LinkKind::Path, "../x".into())
            ]
        );
        assert_eq!(kinds("ls -l /etc/").len(), 1);
        // Not paths: fractions, "and/or", options, lone slash.
        assert!(kinds("speed 3/4 and/or 10 MB/s a / b").is_empty());
        assert!(kinds("--prefix=value").is_empty());
        assert_eq!(kinds("PATH=/usr/bin:/bin")[0].1, "/usr/bin");
    }

    #[test]
    fn addresses_and_hosts() {
        assert_eq!(
            kinds("connect to 192.168.1.20 or 10.0.0.5:2222 now"),
            vec![
                (LinkKind::Ip, "192.168.1.20".into()),
                (LinkKind::Ip, "10.0.0.5:2222".into())
            ]
        );
        assert!(kinds("version 1.2.3.4.5 and 999.1.1.1").is_empty());
        assert_eq!(
            kinds("inet6 fe80::1ff:fe23:4567:890a/64 and ::1"),
            vec![
                (LinkKind::Ip, "fe80::1ff:fe23:4567:890a".into()),
                (LinkKind::Ip, "::1".into())
            ]
        );
        // MAC addresses and times are not IPv6.
        assert!(kinds("aa:bb:cc:dd:ee:ff at 12:34:56").is_empty());
        assert_eq!(
            kinds("ssh web01.prod.example.com"),
            vec![(LinkKind::Host, "web01.prod.example.com".into())]
        );
        // File names that look like hosts are not.
        assert!(kinds("main.rs README.md config.py a.out libfoo.so").is_empty());
    }

    #[test]
    fn ranges_are_character_columns() {
        let line = "héllo https://x.io/é end";
        let f = at(line, 8).unwrap_or_else(|| panic!("no link"));
        assert_eq!(f.kind, LinkKind::Url);
        let chars: Vec<char> = line.chars().collect();
        let text: String = chars[f.range.clone()].iter().collect();
        assert_eq!(text, f.text);
        assert!(at(line, 0).is_none());
        assert!(at(line, f.range.end).is_none());
    }

    #[test]
    fn path_resolution() {
        assert_eq!(resolve_path("/etc/hosts", None, None).as_deref(), Some("/etc/hosts"));
        assert_eq!(
            resolve_path("./build/x", Some("/srv/app"), None).as_deref(),
            Some("/srv/app/build/x")
        );
        assert_eq!(
            resolve_path("../conf/", Some("/srv/app"), None).as_deref(),
            Some("/srv/conf/")
        );
        assert_eq!(resolve_path("./x", None, None), None);
        assert_eq!(
            resolve_path("~/notes", Some("/home/ann/src"), Some("bob")).as_deref(),
            Some("/home/ann/notes")
        );
        assert_eq!(resolve_path("~/n", None, Some("bob")).as_deref(), Some("/home/bob/n"));
        assert_eq!(resolve_path("~/n", None, Some("root")).as_deref(), Some("/root/n"));
        assert_eq!(resolve_path("~/n", None, None), None);
        assert_eq!(resolve_path("~other/n", Some("/home/a"), None), None);
        assert_eq!(normalize("/a/./b/../c//d/"), "/a/c/d/");
        assert_eq!(normalize("/../.."), "/");
    }

    #[test]
    fn empty_and_huge_lines_are_cheap() {
        assert!(detect("").is_empty());
        assert!(detect("      ").is_empty());
        let long = "word ".repeat(2000);
        assert!(detect(&long).is_empty());
    }
}
