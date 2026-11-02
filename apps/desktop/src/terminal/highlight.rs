//! Keyword highlighting (SecureCRT/MobaXterm style): text matching a rule is
//! drawn in the rule's colour. Only the foreground changes, and only on
//! visible rows, so it costs nothing while output floods past.
//!
//! Rules are compiled once per settings change and shared by every
//! terminal. Each terminal caches the result per row *content*, so an
//! unchanged screen is never re-matched.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::sync::{Arc, Mutex};

use alacritty_terminal::vte::ansi::Rgb;
use hostwick_core::HighlightRule;
use regex::Regex;

/// A coloured run of columns `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
}

#[derive(Debug, Default)]
pub struct Highlighter {
    /// Unique per compilation, so caches notice a settings change.
    id: u64,
    rules: Vec<(Regex, Rgb)>,
}

impl Highlighter {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Compile the enabled rules. Bad patterns or colours are skipped and
    /// returned as messages.
    pub fn compile(rules: &[HighlightRule]) -> (Self, Vec<String>) {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for r in rules
            .iter()
            .filter(|r| r.enabled && !r.pattern.trim().is_empty())
        {
            let Some(color) = parse_color(&r.color) else {
                errors.push(format!(
                    "highlight `{}`: bad colour `{}`",
                    r.pattern, r.color
                ));
                continue;
            };
            let source = if r.regex {
                r.pattern.clone()
            } else {
                word_pattern(r.pattern.trim())
            };
            match regex::RegexBuilder::new(&source)
                .size_limit(1 << 20)
                .build()
            {
                Ok(re) => out.push((re, color)),
                Err(e) => errors.push(format!("highlight `{}`: {e}", r.pattern)),
            }
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (Self { id, rules: out }, errors)
    }

    /// Coloured spans for one row of text (one `char` per column), in paint
    /// order: where rules overlap, the earlier rule is painted last and wins.
    pub fn spans(&self, row: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        if self.rules.is_empty() || row.trim().is_empty() {
            return spans;
        }
        // Byte offset → column.
        let mut col_of = vec![0usize; row.len() + 1];
        let mut col = 0;
        for (i, ch) in row.char_indices() {
            for c in col_of.iter_mut().skip(i).take(ch.len_utf8()) {
                *c = col;
            }
            col += 1;
        }
        col_of[row.len()] = col;
        for (re, color) in self.rules.iter().rev() {
            for m in re.find_iter(row) {
                if m.start() == m.end() {
                    continue;
                }
                spans.push(Span {
                    start: col_of[m.start()],
                    end: col_of[m.end()],
                    color: *color,
                });
            }
        }
        spans
    }
}

/// A plain word: case-insensitive, and whole-word at whichever ends are
/// word characters (so `[ERR]` and `error:` both work).
fn word_pattern(word: &str) -> String {
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let pre = if is_word(word.chars().next()) {
        r"\b"
    } else {
        ""
    };
    let post = if is_word(word.chars().last()) {
        r"\b"
    } else {
        ""
    };
    format!("(?i){pre}{}{post}", regex::escape(word))
}

pub fn parse_color(s: &str) -> Option<Rgb> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    })
}

/// The compiled highlighter for these rules, shared by all terminals and
/// recompiled only when the rules change.
pub fn shared(rules: &[HighlightRule]) -> Arc<Highlighter> {
    static CACHE: Mutex<Option<(Vec<HighlightRule>, Arc<Highlighter>)>> = Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return Arc::new(Highlighter::compile(rules).0);
    };
    if let Some((r, h)) = cache.as_ref()
        && r.as_slice() == rules
    {
        return h.clone();
    }
    let (h, errors) = Highlighter::compile(rules);
    for e in errors {
        tracing::warn!("{e}");
    }
    let h = Arc::new(h);
    *cache = Some((rules.to_vec(), h.clone()));
    h
}

/// Per-terminal cache: row text → spans.
#[derive(Default)]
pub struct RowCache {
    owner: u64,
    map: HashMap<u64, Arc<[Span]>>,
}

impl RowCache {
    const MAX: usize = 4096;

    pub fn get(&mut self, h: &Arc<Highlighter>, row: &str) -> Arc<[Span]> {
        // A different highlighter (settings changed): start over.
        if self.owner != h.id {
            self.owner = h.id;
            self.map.clear();
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        row.hash(&mut hasher);
        let key = hasher.finish();
        if let Some(s) = self.map.get(&key) {
            return s.clone();
        }
        if self.map.len() >= Self::MAX {
            self.map.clear();
        }
        let spans: Arc<[Span]> = h.spans(row).into();
        self.map.insert(key, spans.clone());
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(p: &str, regex: bool, c: &str) -> HighlightRule {
        HighlightRule {
            pattern: p.into(),
            regex,
            color: c.into(),
            enabled: true,
        }
    }

    #[test]
    fn plain_words_are_case_insensitive_whole_words() {
        let (h, errs) = Highlighter::compile(&[rule("error", false, "#ff0000")]);
        assert!(errs.is_empty());
        let s = h.spans("ERROR: no errors, Error!");
        let cols: Vec<_> = s.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(cols, vec![(0, 5), (18, 23)]);
        assert_eq!(s[0].color, Rgb { r: 255, g: 0, b: 0 });
        // Non-word edges.
        let (h, _) = Highlighter::compile(&[rule("[err]", false, "#00ff00")]);
        assert_eq!(h.spans("x [ERR] y").len(), 1);
        // Regex metacharacters in a plain word are literal.
        let (h, _) = Highlighter::compile(&[rule("a.b", false, "#00ff00")]);
        assert_eq!(h.spans("axb a.b").len(), 1);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let (h, _) = Highlighter::compile(&[rule("fail", false, "#ff0000")]);
        let s = h.spans("été fail");
        assert_eq!((s[0].start, s[0].end), (4, 8));
    }

    #[test]
    fn defaults_and_bad_rules() {
        let (h, errs) = Highlighter::compile(&HighlightRule::defaults());
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(h.spans("Active: active (running) since").len(), 1);
        assert_eq!(h.spans("Permission denied").len(), 1);
        let (h, errs) = Highlighter::compile(&[
            rule("(unclosed", true, "#ff0000"),
            rule("x", false, "red"),
            HighlightRule {
                enabled: false,
                ..rule("y", false, "#ffffff")
            },
            rule("", false, "#ffffff"),
            rule("^", true, "#ffffff"),
        ]);
        assert_eq!(errs.len(), 2, "{errs:?}");
        // Empty matches are ignored.
        assert!(h.spans("abc").is_empty());
    }

    #[test]
    fn earlier_rules_win_overlaps() {
        let (h, _) = Highlighter::compile(&[
            rule("disk error", false, "#ff0000"),
            rule("disk", false, "#0000ff"),
        ]);
        let s = h.spans("disk error");
        // Painted in order; the last span is the earlier rule.
        assert_eq!(s.last().map(|s| s.color), Some(Rgb { r: 255, g: 0, b: 0 }));
    }

    #[test]
    fn shared_and_cached() {
        let rules = vec![rule("ok", false, "#00ff00")];
        let a = shared(&rules);
        let b = shared(&rules);
        assert!(Arc::ptr_eq(&a, &b));
        let mut cache = RowCache::default();
        let s1 = cache.get(&a, "all ok");
        let s2 = cache.get(&a, "all ok");
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(s1.len(), 1);
        assert_eq!(
            parse_color("#0a0B0c"),
            Some(Rgb {
                r: 10,
                g: 11,
                b: 12
            })
        );
        assert_eq!(parse_color("0a0b0c"), None);
    }
}
