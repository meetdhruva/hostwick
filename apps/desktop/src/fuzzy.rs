//! Fuzzy matching for the command palette and the quick-connect box, and
//! "frecency" (how often and how recently something was used) to rank what
//! people actually use first.
//!
//! Pure logic, independent of GPUI. Matching is case-insensitive, works on
//! characters (not bytes, so accents and CJK text highlight correctly) and
//! takes every word of the query separately: `prod web` finds
//! `prod/eu/web01`. A word must appear in order, not necessarily
//! contiguously (`pw1` finds `prod-web-1`); runs of consecutive characters
//! and matches at word starts rank higher.

use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Fold a character for case-insensitive comparison.
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        // Characters whose lowercase is several characters (İ → i̇) keep
        // their first one, so positions stay one-to-one.
        (Some(f), _) => f,
        _ => c,
    }
}

/// A text prepared once for matching many times: its folded characters.
#[derive(Debug, Clone, Default)]
pub struct Prepared {
    chars: Vec<char>,
}

impl Prepared {
    pub fn new(text: &str) -> Self {
        Self {
            chars: text.chars().map(fold).collect(),
        }
    }
}

/// A parsed query: its words, folded.
#[derive(Debug, Clone, Default)]
pub struct Query {
    words: Vec<Vec<char>>,
}

impl Query {
    pub fn new(text: &str) -> Self {
        Self {
            words: text
                .split_whitespace()
                .map(|w| w.chars().map(fold).collect())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// Where a query matched a candidate's label (and its detail line).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hit {
    pub score: i32,
    /// Matched character positions in the label.
    pub label: Vec<usize>,
    /// Matched character positions in the detail.
    pub detail: Vec<usize>,
}

const SCORE_MATCH: i32 = 16;
const BONUS_CONSECUTIVE: i32 = 18;
const BONUS_BOUNDARY: i32 = 24;
const BONUS_FIRST_CHAR: i32 = 12;
const PENALTY_GAP: i32 = 2;
const PENALTY_GAP_MAX: i32 = 30;
const PENALTY_LEADING_MAX: i32 = 12;
const BONUS_PREFIX: i32 = 30;
const BONUS_EXACT: i32 = 60;
/// A word found only in the detail line counts for less.
const PENALTY_DETAIL: i32 = 20;
/// How many places to try starting a word from.
const MAX_STARTS: usize = 8;

fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '/' | '\\' | '-' | '_' | '.' | '@' | ':' | ',' | '(' | '['
        )
}

/// Is position `i` of `orig` the start of a word?
fn at_boundary(hay: &[char], i: usize) -> bool {
    i == 0 || is_separator(hay[i - 1]) || (hay[i - 1].is_ascii_digit() != hay[i].is_ascii_digit())
}

/// Best placement of `word` in `hay` (both folded): score and positions.
pub fn match_word(word: &[char], hay: &[char]) -> Option<(i32, Vec<usize>)> {
    let first = *word.first()?;
    if word.len() > hay.len() {
        return None;
    }
    let mut best: Option<(i32, Vec<usize>)> = None;
    let starts = hay
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == first)
        .map(|(i, _)| i)
        .take(MAX_STARTS);
    for start in starts {
        // Greedy from this start, preferring a word start or the next
        // character for each following letter.
        let mut pos = Vec::with_capacity(word.len());
        pos.push(start);
        let mut i = start + 1;
        let mut ok = true;
        for &w in &word[1..] {
            // Next occurrence; but if an occurrence right after the previous
            // match exists, take it (keeps runs together).
            let Some(next) = hay[i..].iter().position(|c| *c == w).map(|p| p + i) else {
                ok = false;
                break;
            };
            let chosen = if next == i {
                next
            } else {
                // Prefer a boundary occurrence when there is one.
                hay[next..]
                    .iter()
                    .enumerate()
                    .find(|(k, c)| **c == w && at_boundary(hay, next + k))
                    .map(|(k, _)| next + k)
                    .filter(|b| b - next < 24)
                    .unwrap_or(next)
            };
            pos.push(chosen);
            i = chosen + 1;
        }
        if !ok {
            // Later starts cannot match either.
            break;
        }
        let s = score_positions(&pos, hay, word.len());
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, pos));
        }
    }
    best
}

fn score_positions(pos: &[usize], hay: &[char], word_len: usize) -> i32 {
    let mut s = 0;
    let mut gaps = 0;
    for (k, &p) in pos.iter().enumerate() {
        s += SCORE_MATCH;
        if at_boundary(hay, p) {
            s += BONUS_BOUNDARY;
        }
        if k > 0 {
            let prev = pos[k - 1];
            if p == prev + 1 {
                s += BONUS_CONSECUTIVE;
            } else {
                gaps += (p - prev - 1) as i32;
            }
        }
    }
    s -= (gaps * PENALTY_GAP).min(PENALTY_GAP_MAX);
    let lead = pos.first().copied().unwrap_or(0) as i32;
    if lead == 0 {
        s += BONUS_FIRST_CHAR;
    }
    s -= lead.min(PENALTY_LEADING_MAX);
    // The whole word at the start, or the whole text.
    if pos.first() == Some(&0) && pos.windows(2).all(|w| w[1] == w[0] + 1) {
        s += BONUS_PREFIX;
        if word_len == hay.len() {
            s += BONUS_EXACT;
        }
    }
    s
}

/// Match every word of `q` against a label (preferred) and a detail line.
/// None when a word is in neither.
pub fn match_fields(q: &Query, label: &Prepared, detail: &Prepared) -> Option<Hit> {
    let mut hit = Hit::default();
    for w in &q.words {
        if let Some((s, pos)) = match_word(w, &label.chars) {
            hit.score += s;
            hit.label.extend(pos);
        } else if let Some((s, pos)) = match_word(w, &detail.chars) {
            hit.score += s - PENALTY_DETAIL;
            hit.detail.extend(pos);
        } else {
            return None;
        }
    }
    hit.label.sort_unstable();
    hit.label.dedup();
    hit.detail.sort_unstable();
    hit.detail.dedup();
    Some(hit)
}

/// Byte ranges of `text` covering the characters at `positions` (sorted),
/// merged where they touch: ready for text highlighting.
pub fn byte_ranges(text: &str, positions: &[usize]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut want = positions.iter().copied().peekable();
    for (ci, (bi, c)) in text.char_indices().enumerate() {
        let Some(&next) = want.peek() else {
            break;
        };
        if ci < next {
            continue;
        }
        want.next();
        let r = bi..bi + c.len_utf8();
        match out.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => out.push(r),
        }
    }
    out
}

/// How often and how recently each thing was used; keys look like
/// `host:<id>`, `quick:user@host`, `action:split-right`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Frecency {
    #[serde(default)]
    items: HashMap<String, Use>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Use {
    /// Times used (capped).
    n: u32,
    /// Last use, Unix seconds.
    last: u64,
}

/// At most this many entries are kept.
const FRECENCY_KEEP: usize = 500;
const USES_CAP: u32 = 100;

fn recency_weight(age_secs: u64) -> f64 {
    const DAY: u64 = 86_400;
    match age_secs {
        a if a < DAY => 1.2,
        a if a < 4 * DAY => 1.0,
        a if a < 14 * DAY => 0.7,
        a if a < 31 * DAY => 0.5,
        a if a < 90 * DAY => 0.3,
        _ => 0.1,
    }
}

impl Frecency {
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Count one use of `key` at `now` (Unix seconds).
    pub fn record(&mut self, key: &str, now: u64) {
        let u = self
            .items
            .entry(key.to_owned())
            .or_insert(Use { n: 0, last: 0 });
        u.n = (u.n + 1).min(USES_CAP);
        u.last = u.last.max(now);
        if self.items.len() > FRECENCY_KEEP {
            self.prune(now);
        }
    }

    /// Forget `key` (a deleted host).
    pub fn forget(&mut self, key: &str) {
        self.items.remove(key);
    }

    /// Raw frecency: uses weighted by how recent the last one was.
    pub fn score(&self, key: &str, now: u64) -> f64 {
        self.items.get(key).map_or(0.0, |u| {
            f64::from(u.n) * recency_weight(now.saturating_sub(u.last))
        })
    }

    /// Ranking bonus to add to a match score: grows slowly, so a much
    /// better match still beats a frequently used one.
    pub fn boost(&self, key: &str, now: u64) -> i32 {
        let s = self.score(key, now);
        (s.ln_1p() * 14.0).round() as i32
    }

    fn prune(&mut self, now: u64) {
        let mut v: Vec<(String, f64)> = self
            .items
            .keys()
            .map(|k| (k.clone(), self.score(k, now)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (k, _) in v.into_iter().skip(FRECENCY_KEEP * 9 / 10) {
            self.items.remove(&k);
        }
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(q: &str, label: &str) -> Option<Hit> {
        match_fields(&Query::new(q), &Prepared::new(label), &Prepared::new(""))
    }

    fn rank<'a>(q: &str, labels: &[&'a str]) -> Vec<&'a str> {
        let mut v: Vec<(i32, &str)> = labels
            .iter()
            .filter_map(|l| m(q, l).map(|h| (h.score, *l)))
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        v.into_iter().map(|(_, l)| l).collect()
    }

    #[test]
    fn subsequence_in_order_case_insensitive() {
        assert!(m("pw1", "prod-web-1").is_some());
        assert!(m("PROD", "prod/web").is_some());
        assert!(m("wp", "prod-web").is_none(), "order matters");
        assert!(m("xyz", "prod").is_none());
        assert!(m("", "anything").is_some_and(|h| h.score == 0));
        assert!(m("toolongquery", "short").is_none());
    }

    #[test]
    fn every_word_must_match_in_label_or_detail() {
        let q = Query::new("prod 10.0");
        let label = Prepared::new("prod/web01");
        let detail = Prepared::new("deploy@10.0.0.5");
        let h = match_fields(&q, &label, &detail).unwrap_or_default();
        assert_eq!(h.label, vec![0, 1, 2, 3]);
        assert_eq!(h.detail, vec![7, 8, 9, 10]);
        assert!(match_fields(&Query::new("prod nope"), &label, &detail).is_none());
    }

    #[test]
    fn ranking_prefers_prefix_words_and_runs() {
        // Exact name beats a prefix beats a word start beats scattered letters.
        assert_eq!(
            rank("web", &["xwxexbx", "my-website", "web01", "web"]),
            vec!["web", "web01", "my-website", "xwxexbx"]
        );
        // Word starts ("db" = data-backup) rank above letters inside words.
        let r = rank("db", &["adbc", "data-backup"]);
        assert_eq!(r, vec!["data-backup", "adbc"]);
        // A label match ranks above the same word found only in the detail.
        let q = Query::new("web");
        let a = match_fields(&q, &Prepared::new("web"), &Prepared::new("")).unwrap_or_default();
        let b = match_fields(&q, &Prepared::new("db"), &Prepared::new("web")).unwrap_or_default();
        assert!(a.score > b.score);
    }

    #[test]
    fn highlight_positions_and_unicode() {
        // Positions are characters; highlighting gets byte ranges.
        let h = m("münch", "Server München").unwrap_or_default();
        assert_eq!(h.label, vec![7, 8, 9, 10, 11]);
        let text = "Server München";
        let r = byte_ranges(text, &h.label);
        assert_eq!(r.len(), 1);
        assert_eq!(&text[r[0].clone()], "Münch");
        // CJK and mixed scripts.
        let h = m("服务", "生产服务器").unwrap_or_default();
        assert_eq!(h.label, vec![2, 3]);
        assert_eq!(
            &"生产服务器"[byte_ranges("生产服务器", &h.label)[0].clone()],
            "服务"
        );
        // Non-adjacent positions stay separate ranges.
        assert_eq!(byte_ranges("abcd", &[0, 2, 3]), vec![0..1, 2..4]);
        // Uppercase that lowercases to two characters keeps positions.
        assert!(m("i", "İstanbul").is_some());
    }

    #[test]
    fn frecency_ranks_recent_and_frequent_and_persists() {
        let now = 1_000_000_000;
        let mut f = Frecency::default();
        for _ in 0..5 {
            f.record("host:a", now);
        }
        f.record("host:b", now);
        assert!(f.boost("host:a", now) > f.boost("host:b", now));
        assert_eq!(f.boost("host:none", now), 0);
        // Old use counts less than the same use today.
        let mut old = Frecency::default();
        old.record("host:a", now - 200 * 86_400);
        old.record("host:b", now);
        assert!(old.score("host:b", now) > old.score("host:a", now));
        // Round trip, and junk input starts empty.
        assert_eq!(Frecency::from_json(&f.to_json()), f);
        assert_eq!(Frecency::from_json("not json"), Frecency::default());
        f.forget("host:a");
        assert_eq!(f.score("host:a", now), 0.0);
    }

    #[test]
    fn frecency_breaks_ties_but_not_clear_wins() {
        let now = 2_000_000_000;
        let mut f = Frecency::default();
        for _ in 0..30 {
            f.record("web02", now);
        }
        let score =
            |label: &str| m("web0", label).map_or(i32::MIN, |h| h.score) + f.boost(label, now);
        // Same match quality: the one used a lot wins.
        assert!(score("web02") > score("web01"));
        // A far better match still wins against frecency.
        let mut g = Frecency::default();
        for _ in 0..100 {
            g.record("w-x-e-y-b", now);
        }
        let s1 = m("web", "web").map_or(0, |h| h.score);
        let s2 = m("web", "w-x-e-y-b").map_or(0, |h| h.score) + g.boost("w-x-e-y-b", now);
        assert!(s1 > s2, "{s1} vs {s2}");
    }

    #[test]
    fn frecency_stays_bounded() {
        let mut f = Frecency::default();
        for i in 0..2000u64 {
            f.record(&format!("k{i}"), 1_000 + i);
        }
        assert!(f.items.len() <= FRECENCY_KEEP);
        // The most recent survive.
        assert!(f.score("k1999", 3_000) > 0.0);
    }

    /// Five thousand hosts filter in a few milliseconds (palette and
    /// quick connect run this on every keystroke).
    #[test]
    fn fast_with_thousands_of_hosts() {
        let hosts: Vec<(Prepared, Prepared)> = (0..5000)
            .map(|i| {
                (
                    Prepared::new(&format!("region-{}/cluster-{}/node-{i:04}", i % 7, i % 31)),
                    Prepared::new(&format!(
                        "admin@10.{}.{}.{}:22",
                        i % 250,
                        (i / 250) % 250,
                        i % 13
                    )),
                )
            })
            .collect();
        let started = std::time::Instant::now();
        let mut total = 0;
        for q in [
            "n",
            "no",
            "nod",
            "node",
            "node-04",
            "reg 3 clu",
            "10.1",
            "zzz",
        ] {
            let q = Query::new(q);
            total += hosts
                .iter()
                .filter(|(l, d)| match_fields(&q, l, d).is_some())
                .count();
        }
        let per_keystroke = started.elapsed() / 8;
        assert!(total > 0);
        // Generous for unoptimised debug builds on slow CI machines.
        assert!(
            per_keystroke < std::time::Duration::from_millis(60),
            "{per_keystroke:?} per keystroke"
        );
    }
}
