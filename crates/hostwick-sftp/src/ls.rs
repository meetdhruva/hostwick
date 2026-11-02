//! Parser for `ls -la` output, for servers reached over SCP (no SFTP).
//!
//! Handles GNU coreutils (with `--time-style=+%s`, `long-iso`, `full-iso` or
//! the default), BSD/macOS (with or without `-T`) and busybox (with or
//! without `-e`). Names keep their inner and trailing spaces; symlinks are
//! split at ` -> `; device files (`1, 3` instead of a size) are recognised.

use crate::{Entry, EntryKind, join};

/// Which `ls` invocation produced the output; picked once per server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LsFlavor {
    /// GNU `ls --time-style=+%s`: Unix seconds, no time-zone guessing.
    Epoch,
    /// busybox `ls -e` / BSD `ls -T`: full date and time to the second.
    Full,
    /// Plain `ls -la`: `Nov  2 18:38` or `Nov  2  2024`.
    Plain,
}

/// One parsed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LsLine {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<u64>,
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub link_target: Option<String>,
    /// How many tokens the date took (1 epoch, 2–3 ISO, 3 plain, 4 BSD -T,
    /// 5 busybox -e): tells which `ls` options a server understood.
    pub date_tokens: usize,
}

impl LsLine {
    pub fn into_entry(self, dir: &str) -> Entry {
        let uid = self.owner.parse().ok();
        let gid = self.group.parse().ok();
        Entry {
            path: join(dir, &self.name),
            name: self.name,
            kind: self.kind,
            size: self.size,
            modified: self.modified,
            modified_nanos: None,
            permissions: Some(self.permissions),
            owner: (uid.is_none()).then_some(self.owner),
            group: (gid.is_none()).then_some(self.group),
            uid,
            gid,
        }
    }
}

/// Parse a whole listing of `dir`, skipping `total`, `.`, `..` and lines
/// that are not entries (error messages). `tz_offset` is the server's UTC
/// offset in seconds (for formats without a time zone); `now` is used to
/// pick the year of `Nov  2 18:38`-style dates.
pub fn parse_ls_output(text: &str, dir: &str, tz_offset: i64, now: u64) -> Vec<Entry> {
    parse_ls_output_with(text, dir, tz_offset, now, false)
}

/// [`parse_ls_output`] for `ls -b` output (`escaped`): names (and link
/// targets) are written with C-style escapes, so a line break in a name
/// cannot start a fake line; they are decoded here.
pub fn parse_ls_output_with(
    text: &str,
    dir: &str,
    tz_offset: i64,
    now: u64,
    escaped: bool,
) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| parse_ls_line(l, tz_offset, now))
        .map(|mut l| {
            if escaped {
                l.name = decode_ls_escapes(&l.name);
                l.link_target = l.link_target.as_deref().map(decode_ls_escapes);
            }
            l
        })
        .filter(|l| l.name != "." && l.name != "..")
        .map(|l| l.into_entry(dir))
        .collect()
}

/// Undo `ls -b` / `--quoting-style=escape`: `\ooo` octal bytes, `\n`,
/// `\t` and friends, and `\X` for any other character `X` (GNU writes
/// `\ ` for a space and `\\` for a backslash).
pub fn decode_ls_escapes(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        if (b'0'..=b'7').contains(&c) {
            let mut v: u32 = 0;
            let mut j = i + 1;
            while j < b.len() && j < i + 4 && (b'0'..=b'7').contains(&b[j]) {
                v = v * 8 + u32::from(b[j] - b'0');
                j += 1;
            }
            out.push(u8::try_from(v & 0xff).unwrap_or(0));
            i = j;
            continue;
        }
        out.push(match c {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'v' => 0x0b,
            b'e' => 0x1b,
            other => other,
        });
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse one `ls -l` line. `None` for anything that is not an entry.
pub fn parse_ls_line(line: &str, tz_offset: i64, now: u64) -> Option<LsLine> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let toks = tokens(line);
    if toks.len() < 6 {
        return None;
    }
    let (kind, permissions) = parse_mode(toks[0].1)?;
    // links must be a number (rules out "total 12" and error messages)
    toks[1].1.parse::<u64>().ok()?;
    let device = matches!(toks[0].1.as_bytes()[0], b'c' | b'b');

    // Where the date starts: after owner, group and size (or "major, minor").
    let mut candidates: Vec<usize> = Vec::with_capacity(3);
    if device {
        if toks.get(4).is_some_and(|t| t.1.ends_with(',')) {
            candidates.push(6);
        }
        candidates.push(5);
    } else {
        candidates.push(5);
        // No group column (`ls -o`, some embedded systems).
        candidates.push(4);
    }
    for &i in &candidates {
        if i >= toks.len() {
            continue;
        }
        let size_tok = toks[i - 1].1;
        let size = if device {
            0
        } else {
            match size_tok.parse::<u64>() {
                Ok(s) => s,
                Err(_) => continue,
            }
        };
        let Some((mtime, used)) = parse_date(&toks[i..], tz_offset, now) else {
            continue;
        };
        let last = &toks[i + used - 1];
        let name_start = last.0 + last.1.len() + 1;
        if name_start > line.len() {
            continue;
        }
        let rest = &line[name_start..];
        if rest.is_empty() {
            continue;
        }
        let (name, link_target) = match kind {
            EntryKind::Symlink => match rest.split_once(" -> ") {
                Some((n, t)) => (n.to_owned(), Some(t.to_owned())),
                None => (rest.to_owned(), None),
            },
            _ => (rest.to_owned(), None),
        };
        let (owner, group) = if i == 4 {
            (toks[2].1.to_owned(), String::new())
        } else {
            (toks[2].1.to_owned(), toks[3].1.to_owned())
        };
        return Some(LsLine {
            name,
            kind,
            size,
            modified: mtime,
            permissions,
            owner,
            group,
            link_target,
            date_tokens: used,
        });
    }
    None
}

/// Space-separated tokens with their byte offsets.
fn tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in line.char_indices() {
        if c == ' ' || c == '\t' {
            if let Some(s) = start.take() {
                out.push((s, &line[s..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

/// `drwxr-sr-t` → (Dir, 0o3755). Accepts a trailing ACL/xattr marker
/// (`+`, `.`, `@`).
fn parse_mode(m: &str) -> Option<(EntryKind, u32)> {
    let m = m.trim_end_matches(['+', '.', '@']);
    let b = m.as_bytes();
    if b.len() != 10 {
        return None;
    }
    let kind = match b[0] {
        b'-' => EntryKind::File,
        b'd' => EntryKind::Dir,
        b'l' => EntryKind::Symlink,
        b'c' | b'b' | b'p' | b's' | b'D' => EntryKind::Other,
        _ => return None,
    };
    let mut mode = 0u32;
    for (i, &c) in b[1..].iter().enumerate() {
        let bit = 1 << (8 - i);
        let exec_slot = i % 3 == 2;
        match c {
            b'r' if i % 3 == 0 => mode |= bit,
            b'w' if i % 3 == 1 => mode |= bit,
            b'x' if exec_slot => mode |= bit,
            b's' | b'S' | b't' | b'T' if exec_slot => {
                if c.is_ascii_lowercase() {
                    mode |= bit;
                }
                mode |= match i {
                    2 => 0o4000,
                    5 => 0o2000,
                    _ => 0o1000,
                };
            }
            b'-' => {}
            _ => return None,
        }
    }
    Some((kind, mode))
}

fn month(s: &str) -> Option<u32> {
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    M.iter()
        .position(|m| m.eq_ignore_ascii_case(s))
        .map(|i| i as u32 + 1)
}

fn is_weekday(s: &str) -> bool {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .iter()
        .any(|d| d.eq_ignore_ascii_case(s))
}

/// `HH:MM` or `HH:MM:SS[.frac]` → seconds into the day.
fn hms(s: &str) -> Option<i64> {
    let s = s.split('.').next()?;
    let mut parts = s.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let sec: i64 = match parts.next() {
        Some(x) => x.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() || h > 23 || m > 59 || sec > 60 {
        return None;
    }
    Some(h * 3600 + m * 60 + sec)
}

/// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `CCYYMMDDhhmm.SS` (UTC) for `touch -t`.
pub(crate) fn touch_stamp(secs: u64) -> String {
    let secs = i64::try_from(secs).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}.{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn year_of(secs: i64) -> i64 {
    // Inverse of days_from_civil, year only.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(m <= 2)
}

fn to_unix(local: i64, tz_offset: i64) -> Option<u64> {
    u64::try_from(local - tz_offset).ok()
}

fn tz_token(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 5 || !(b[0] == b'+' || b[0] == b'-') {
        return None;
    }
    if !b[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let digit = |i: usize| i64::from(b[i] - b'0');
    let off = (digit(1) * 10 + digit(2)) * 3600 + (digit(3) * 10 + digit(4)) * 60;
    Some(if b[0] == b'-' { -off } else { off })
}

/// Dates outside this range are not real `ls` output; refusing them also
/// keeps the calendar arithmetic far from overflow.
fn plausible(y: i64, d: u32) -> bool {
    (1..=9999).contains(&y) && (1..=31).contains(&d)
}

/// The date at the start of `t`: `(unix seconds, tokens used)`.
fn parse_date(t: &[(usize, &str)], tz: i64, now: u64) -> Option<(Option<u64>, usize)> {
    let tok = |i: usize| t.get(i).map(|x| x.1);
    // Needs at least one more token for the name.
    let fits = |used: usize| t.len() > used;

    // busybox -e: "Wed Nov  2 18:38:00 2026"
    if let (Some(w), Some(mo), Some(d), Some(tm), Some(y)) =
        (tok(0), tok(1), tok(2), tok(3), tok(4))
        && is_weekday(w)
        && fits(5)
        && let (Some(mo), Ok(d), Some(secs), Ok(y)) =
            (month(mo), d.parse::<u32>(), hms(tm), y.parse::<i64>())
        && plausible(y, d)
    {
        let local = days_from_civil(y, mo, d) * 86_400 + secs;
        return Some((to_unix(local, tz), 5));
    }
    // BSD -T: "Nov  2 18:38:00 2026"
    if let (Some(mo), Some(d), Some(tm), Some(y)) = (tok(0), tok(1), tok(2), tok(3))
        && fits(4)
        && tm.contains(':')
        && let (Some(mo), Ok(d), Some(secs), Ok(y)) =
            (month(mo), d.parse::<u32>(), hms(tm), y.parse::<i64>())
        && y > 1900
        && plausible(y, d)
    {
        let local = days_from_civil(y, mo, d) * 86_400 + secs;
        return Some((to_unix(local, tz), 4));
    }
    // ISO: "2026-11-02 18:38[:00.123] [+0100]"
    if let (Some(date), Some(tm)) = (tok(0), tok(1))
        && date.len() == 10
        && date.as_bytes()[4] == b'-'
        && let Some(secs) = hms(tm)
    {
        let mut parts = date.split('-');
        let y: i64 = parts.next()?.parse().ok()?;
        let mo: u32 = parts.next()?.parse().ok()?;
        let d: u32 = parts.next()?.parse().ok()?;
        if !plausible(y, d) {
            return None;
        }
        let local = days_from_civil(y, mo, d) * 86_400 + secs;
        if let Some(off) = tok(2).and_then(tz_token)
            && fits(3)
        {
            return Some((to_unix(local, off), 3));
        }
        if fits(2) {
            return Some((to_unix(local, tz), 2));
        }
    }
    // Plain: "Nov  2 18:38" or "Nov  2  2024"
    if let (Some(mo), Some(d), Some(x)) = (tok(0), tok(1), tok(2))
        && fits(3)
        && let (Some(mo), Ok(d)) = (month(mo), d.parse::<u32>())
        && (1..=31).contains(&d)
    {
        if let Some(secs) = hms(x) {
            let now_local = i64::try_from(now).unwrap_or(i64::MAX / 2) + tz;
            let mut y = year_of(now_local);
            let mut local = days_from_civil(y, mo, d) * 86_400 + secs;
            // ls shows the time (not the year) for recent files; a date
            // "in the future" means last year.
            if local > now_local + 2 * 86_400 {
                y -= 1;
                local = days_from_civil(y, mo, d) * 86_400 + secs;
            }
            return Some((to_unix(local, tz), 3));
        }
        if let Ok(y) = x.parse::<i64>()
            && plausible(y, d)
        {
            let local = days_from_civil(y, mo, d) * 86_400;
            return Some((to_unix(local, tz), 3));
        }
    }
    // GNU --time-style=+%s
    if let Some(s) = tok(0)
        && fits(1)
        && !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
    {
        return Some((s.parse().ok(), 1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-02 12:00:00 UTC
    const NOW: u64 = 1_793_620_800;

    fn p(line: &str) -> LsLine {
        parse_ls_line(line, 0, NOW).unwrap_or_else(|| panic!("no parse: {line:?}"))
    }

    /// Escaped (`ls -b`) names decode; a line break in a name stays in the
    /// name instead of starting a fake line.
    #[test]
    fn escaped_names() {
        assert_eq!(
            decode_ls_escapes(r"a\ b\\c\nd\303\274\t"),
            "a b\\c\nd\u{fc}\t"
        );
        assert_eq!(decode_ls_escapes("plain"), "plain");
        assert_eq!(decode_ls_escapes("trailing\\"), "trailing\\");
        let text = "-rw-r--r-- 1 a b 2 1700000000 x\\n-rw-r--r--\\ 1\\ a\\ b\\ 5\\ 1700000000\\ ..\n\
                    lrwxrwxrwx 1 a b 3 1700000000 my\\ link -> tar\\ get\n";
        let e = parse_ls_output_with(text, "/d", 0, NOW, true);
        assert_eq!(e.len(), 2, "{e:?}");
        assert_eq!(e[0].name, "x\n-rw-r--r-- 1 a b 5 1700000000 ..");
        assert_eq!(e[1].name, "my link");
    }

    /// Server output must never panic the parser: random bytes, random
    /// UTF-8, and lines assembled from ls-like pieces.
    #[test]
    fn never_panics_on_garbage() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let pieces = [
            "-rw-r--r--",
            "drwxr-xr-x",
            "lrwxrwxrwx",
            "crw-rw----",
            "1",
            "0",
            "root",
            "a",
            "5",
            "1,",
            "3",
            "Nov",
            "Wed",
            "2",
            "18:38",
            "18:38:00",
            "18:38:00.123",
            "2026",
            "99999999999999999999",
            "-99999999999",
            "2024-01-01",
            "9999-99-99",
            "+0100",
            "+1\u{e9}2",
            "-\u{e9}12",
            "1700000000",
            "\u{e9}",
            "->",
            "name",
            " ",
            "\t",
            "\u{1f600}",
            "total",
            "",
        ];
        for _ in 0..20_000 {
            let n = (next() % 12) as usize;
            let line: Vec<&str> = (0..n)
                .map(|_| pieces[(next() % pieces.len() as u64) as usize])
                .collect();
            let line = line.join(" ");
            let _ = parse_ls_line(&line, 3600, NOW);
            let _ = parse_ls_output_with(&line, "/", -3600, NOW, true);
            let bytes: Vec<u8> = (0..(next() % 64)).map(|_| next() as u8).collect();
            let s = String::from_utf8_lossy(&bytes);
            let _ = parse_ls_line(&s, 0, NOW);
            let _ = decode_ls_escapes(&s);
            let chars: String = (0..(next() % 40))
                .filter_map(|_| char::from_u32((next() % 0x2_0000) as u32))
                .collect();
            let _ = parse_ls_line(&format!("-rw-r--r-- 1 a b 5 {chars}"), 0, NOW);
            let _ = tz_token(&chars);
        }
        // The two cases that used to panic.
        let _ = parse_ls_line("-rw-r--r-- 1 a b 5 2024-01-01 10:00 +1\u{e9}2 name", 0, NOW);
        let _ = parse_ls_line("-rw-r--r-- 1 a b 5 Nov 2 99999999999999999 name", 0, NOW);
    }

    #[test]
    fn civil() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(year_of(NOW as i64), 2026);
        assert_eq!(year_of(0), 1970);
        assert_eq!(touch_stamp(0), "197001010000.00");
        assert_eq!(touch_stamp(1_700_000_000), "202311142213.20");
    }

    #[test]
    fn gnu_epoch() {
        let l = p("-rw-r--r-- 1 alice staff 1234 1700000000 notes.txt");
        assert_eq!(l.name, "notes.txt");
        assert_eq!(l.size, 1234);
        assert_eq!(l.modified, Some(1_700_000_000));
        assert_eq!(l.permissions, 0o644);
        assert_eq!((l.owner.as_str(), l.group.as_str()), ("alice", "staff"));
        assert_eq!(l.kind, EntryKind::File);

        let l = p("drwxr-xr-x 2 root root 4096 1700000000 my  dir with spaces ");
        assert_eq!(l.name, "my  dir with spaces ");
        assert_eq!(l.kind, EntryKind::Dir);

        let l = p("lrwxrwxrwx 1 root root 11 1700000000 link name -> /etc/target dir");
        assert_eq!(l.name, "link name");
        assert_eq!(l.link_target.as_deref(), Some("/etc/target dir"));
        assert_eq!(l.kind, EntryKind::Symlink);

        // Numeric owner/group (ls -n) end up as uid/gid.
        let e = p("-rwsr-sr-t 1 0 1000 5 1700000000 x").into_entry("/bin");
        assert_eq!((e.uid, e.gid, e.owner.clone()), (Some(0), Some(1000), None));
        assert_eq!(e.permissions, Some(0o7755));
        assert_eq!(e.path, "/bin/x");
    }

    #[test]
    fn gnu_default_and_iso() {
        // Recent file: time, no year → this year.
        let l = p("-rw-r--r--  1 bob users   42 Nov  1 09:30 a b.txt");
        assert_eq!(l.name, "a b.txt");
        assert_eq!(
            l.modified,
            Some((days_from_civil(2026, 11, 1) * 86_400 + 9 * 3600 + 30 * 60) as u64)
        );
        // Old file: year.
        let l = p("-rw-r--r--  1 bob users   42 Dec 25  2023 old");
        assert_eq!(
            l.modified,
            Some(days_from_civil(2023, 12, 25) as u64 * 86_400)
        );
        // A date "in the future" is last year's.
        let l = p("-rw-r--r--  1 bob users   42 Dec 31 23:00 nye");
        assert_eq!(
            l.modified,
            Some((days_from_civil(2025, 12, 31) * 86_400 + 23 * 3600) as u64)
        );
        let l = p("-rw-r--r-- 1 bob users 42 2024-02-29 13:14 leap");
        assert_eq!(
            l.modified,
            Some((days_from_civil(2024, 2, 29) * 86_400 + 13 * 3600 + 14 * 60) as u64)
        );
        let l = p("-rw-r--r-- 1 bob users 42 2024-02-29 13:14:15.123456789 +0100 full iso");
        assert_eq!(l.name, "full iso");
        assert_eq!(
            l.modified,
            Some((days_from_civil(2024, 2, 29) * 86_400 + 12 * 3600 + 14 * 60 + 15) as u64)
        );
    }

    #[test]
    fn server_time_zone_applies_to_local_dates() {
        let l = parse_ls_line("-rw-r--r-- 1 a b 1 Dec 25  2023 x", 3600, NOW)
            .unwrap_or_else(|| panic!("parse"));
        assert_eq!(
            l.modified,
            Some((days_from_civil(2023, 12, 25) * 86_400 - 3600) as u64)
        );
    }

    #[test]
    fn bsd_macos() {
        let l = p("-rw-r--r--@  1 alice  staff  1024 Nov  2 10:00:05 2026 Screen Shot.png");
        assert_eq!(l.name, "Screen Shot.png");
        assert_eq!(
            l.modified,
            Some((days_from_civil(2026, 11, 2) * 86_400 + 36_005) as u64)
        );
        let l = p("drwxr-xr-x+ 12 alice  staff   384 Oct 30 08:00 Documents");
        assert_eq!(l.kind, EntryKind::Dir);
        assert_eq!(l.name, "Documents");
        let l = p("lrwxr-xr-x   1 root  wheel    11 Jan  1  2020 tmp -> private/tmp");
        assert_eq!(l.link_target.as_deref(), Some("private/tmp"));
    }

    #[test]
    fn busybox() {
        let l = p("-rw-r--r--    1 root     root          1810 Wed Nov  2 18:38:00 2026 hosts");
        assert_eq!(l.name, "hosts");
        assert_eq!(l.size, 1810);
        assert_eq!(
            l.modified,
            Some((days_from_civil(2026, 11, 2) * 86_400 + 18 * 3600 + 38 * 60) as u64)
        );
        let l = p("drwxr-xr-x    2 root     root             0 Jan  1  1970 proc");
        assert_eq!(l.modified, Some(0));
        let l = p("lrwxrwxrwx    1 root     root            12 Nov  2 18:38 sh -> /bin/busybox");
        assert_eq!((l.name.as_str(), l.size), ("sh", 12));
        // Devices: "major, minor" instead of a size.
        let l = p("crw-rw-rw-    1 root     root        1,   3 Nov  2 18:38 null");
        assert_eq!(
            (l.name.as_str(), l.kind, l.size),
            ("null", EntryKind::Other, 0)
        );
        let l = p("brw-rw----    1 root     disk      8,   0 Wed Nov  2 18:38:00 2026 sda");
        assert_eq!(l.name, "sda");
        let l = p("crw--w---- 1 root tty 4,0 1700000000 tty0");
        assert_eq!(l.name, "tty0");
    }

    #[test]
    fn rejects_noise_and_skips_dots() {
        for junk in [
            "total 48",
            "ls: cannot access 'x': No such file or directory",
            "",
            "drwxr-xr-x",
            "?????????? ? ? ? ? ? broken",
        ] {
            assert!(parse_ls_line(junk, 0, NOW).is_none(), "{junk:?}");
        }
        let out = "total 8\n\
            drwxr-xr-x 2 root root 4096 1700000000 .\n\
            drwxr-xr-x 3 root root 4096 1700000000 ..\n\
            -rw-r--r-- 1 root root 3 1700000000 a\r\n\
            prw-r--r-- 1 root root 0 1700000000 fifo\n";
        let e = parse_ls_output(out, "/srv", 0, NOW);
        let names: Vec<_> = e.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["/srv/a", "/srv/fifo"]);
    }

    #[test]
    fn group_column_missing() {
        let l = p("-rw-r--r-- 1 alice 99 1700000000 x");
        // Ambiguous on its own; with a group column absent the size is 99.
        assert_eq!(l.name, "x");
    }
}
