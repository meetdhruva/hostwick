//! Scrollback search on Alacritty's regex engine.
//!
//! - Matches on screen are found per frame (only the visible rows).
//! - Next/previous search outward from the current match with a line
//!   budget, so a keypress never scans a huge scrollback on the UI thread.
//! - "3 of 17" comes from a background count that walks the scrollback in
//!   small locked chunks, so neither the parser nor the painter waits on it
//!   for long. Output that arrives meanwhile shifts lines; the count stores
//!   positions relative to the history size it started with and converts.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use alacritty_terminal::Term;
use alacritty_terminal::sync::FairMutex;

/// Most matches counted (the label then says "10000+").
pub const COUNT_CAP: usize = 10_000;
/// Lines searched per lock while counting.
const CHUNK: i32 = 2_000;
/// Lines next/previous scans on the UI thread before falling back to the
/// background count.
const STEP_BUDGET: usize = 20_000;
/// Lines the search-as-you-type scan covers (older matches come from the
/// background count).
const FIRST_BUDGET: usize = 5_000;
/// Most matches highlighted on screen.
const VISIBLE_CAP: usize = 2_000;

/// Turn the search box into a regex for Alacritty: plain text is escaped,
/// and case sensitivity is explicit (Alacritty would otherwise use "smart
/// case").
pub fn pattern(query: &str, case_sensitive: bool, regex: bool) -> String {
    let body = if regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    format!("{}{body}", if case_sensitive { "(?-i)" } else { "(?i)" })
}

/// Compile, with a short error for the search bar.
pub fn compile(query: &str, case_sensitive: bool, regex: bool) -> Result<RegexSearch, String> {
    if regex {
        // Alacritty's builder reports only "error building DFA"; the regex
        // crate says what is wrong.
        if let Err(e) = regex::Regex::new(query) {
            let msg = e.to_string();
            let last = msg.lines().last().unwrap_or("invalid regex").trim();
            return Err(last.trim_start_matches("error: ").to_owned());
        }
    }
    RegexSearch::new(&pattern(query, case_sensitive, regex)).map_err(|e| {
        let s = e.to_string();
        if s.contains("exceeds size limit") || s.contains("too big") {
            "pattern too complex".to_owned()
        } else {
            s
        }
    })
}

/// Background match count for one query.
#[derive(Debug, Clone, Default)]
pub struct Count {
    /// Match start points, in the coordinates of when the count began.
    pub starts: Vec<Point>,
    pub history_at_start: usize,
    pub capped: bool,
}

impl Count {
    /// 1-based position of `m` among the counted matches.
    pub fn index_of(&self, m: &Match, history_now: usize) -> Option<usize> {
        let p = to_count_coords(*m.start(), self.history_at_start, history_now);
        let i = self.starts.partition_point(|s| *s < p);
        (self.starts.get(i) == Some(&p)).then_some(i + 1)
    }
}

/// A point now → the coordinates the count used (lines shift up as history
/// grows).
fn to_count_coords(p: Point, history_then: usize, history_now: usize) -> Point {
    let shift = history_now as i32 - history_then as i32;
    Point::new(p.line + shift, p.column)
}

fn from_count_coords(p: Point, history_then: usize, history_now: usize) -> Point {
    let shift = history_now as i32 - history_then as i32;
    Point::new(p.line - shift, p.column)
}

/// Count every match in the scrollback, chunk by chunk. Stops early (None)
/// when `generation` moves on (the query changed or search closed).
pub fn count<T>(
    term: &FairMutex<Term<T>>,
    mut regex: RegexSearch,
    generation: &Arc<AtomicU64>,
    mine: u64,
) -> Option<Count> {
    let (top, bottom, hist0) = {
        let term = term.lock();
        (
            term.topmost_line(),
            term.bottommost_line(),
            term.history_size(),
        )
    };
    let mut out = Count {
        history_at_start: hist0,
        ..Count::default()
    };
    // In count coordinates.
    let mut line = top.0;
    while line <= bottom.0 {
        if generation.load(Ordering::Acquire) != mine {
            return None;
        }
        let term = term.lock();
        let shift = term.history_size() as i32 - hist0 as i32;
        let last = (line + CHUNK - 1).min(bottom.0);
        // Now coordinates, clamped to what still exists.
        let a = Line(line - shift).max(term.topmost_line());
        let b = Line(last - shift).min(term.bottommost_line());
        if a <= b {
            let start = Point::new(a, Column(0));
            let end = Point::new(b, term.last_column());
            for m in RegexIter::new(start, end, Direction::Right, &*term, &mut regex) {
                out.starts
                    .push(Point::new(m.start().line + shift, m.start().column));
                if out.starts.len() >= COUNT_CAP {
                    out.capped = true;
                    return Some(out);
                }
            }
        }
        drop(term);
        line = last + 1;
        std::thread::yield_now();
    }
    out.starts.sort();
    out.starts.dedup();
    Some(out)
}

/// Matches intersecting the visible rows.
pub fn visible<T>(term: &Term<T>, regex: &mut RegexSearch) -> Vec<Match> {
    let offset = term.grid().display_offset() as i32;
    let rows = term.screen_lines() as i32;
    // One line above, so a match wrapping onto the first row shows.
    let top = Line(-offset - 1).max(term.topmost_line());
    let bottom = Line(-offset + rows - 1).min(term.bottommost_line());
    let start = Point::new(top, Column(0));
    let end = Point::new(bottom, term.last_column());
    RegexIter::new(start, end, Direction::Right, term, regex)
        .take(VISIBLE_CAP)
        .collect()
}

/// The match nearest the bottom of the screen (the newest output), looking
/// upwards: where a new search starts.
pub fn first<T>(term: &Term<T>, regex: &mut RegexSearch) -> Option<Match> {
    let origin = Point::new(term.bottommost_line(), term.last_column());
    term.search_next(
        regex,
        origin,
        Direction::Left,
        Side::Left,
        Some(FIRST_BUDGET),
    )
}

/// Older (`up`) or newer match than `from`; None if nothing within the
/// line budget (the caller then uses the background count).
pub fn step<T>(term: &Term<T>, regex: &mut RegexSearch, from: &Match, up: bool) -> Option<Match> {
    if up {
        let origin = from.start().sub(term, Boundary::Grid, 1);
        if origin == *from.start() {
            return None;
        }
        term.search_next(
            regex,
            origin,
            Direction::Left,
            Side::Left,
            Some(STEP_BUDGET),
        )
        .filter(|m| m.start() < from.start())
    } else {
        let origin = from.end().add(term, Boundary::Grid, 1);
        if origin == *from.end() {
            return None;
        }
        term.search_next(
            regex,
            origin,
            Direction::Right,
            Side::Right,
            Some(STEP_BUDGET),
        )
        .filter(|m| m.start() > from.start())
    }
}

/// Neighbour of `from` from the background count (wrapping around), turned
/// back into a full match at its current position.
pub fn step_counted<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    count: &Count,
    from: Option<&Match>,
    up: bool,
) -> Option<Match> {
    if count.starts.is_empty() {
        return None;
    }
    let hist = term.history_size();
    let target = match from {
        Some(m) => {
            let p = to_count_coords(*m.start(), count.history_at_start, hist);
            let i = count.starts.partition_point(|s| *s < p);
            if up {
                if i == 0 {
                    count.starts.len() - 1
                } else {
                    i - 1
                }
            } else {
                let j = count.starts.partition_point(|s| *s <= p);
                if j >= count.starts.len() { 0 } else { j }
            }
        }
        None => count.starts.len() - 1,
    };
    let start = from_count_coords(count.starts[target], count.history_at_start, hist);
    if start.line < term.topmost_line() || start.line > term.bottommost_line() {
        return None;
    }
    // Re-run the regex from that point to get the match's end.
    let end = Point::new(term.bottommost_line(), term.last_column());
    term.regex_search_right(regex, start, end)
        .filter(|m| *m.start() == start)
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    use super::super::GridSize;
    use super::*;

    /// 200 lines of output on a 10-row screen; every 20th mentions NEEDLE.
    fn term() -> FairMutex<Term<VoidListener>> {
        let size = GridSize { cols: 40, rows: 10 };
        let mut t = Term::new(Config::default(), &size, VoidListener);
        let mut p: Processor = Processor::new();
        for i in 0..200 {
            let line = if i % 20 == 7 {
                format!("line {i} has a needle here\r\n")
            } else {
                format!("line {i}\r\n")
            };
            p.advance(&mut t, line.as_bytes());
        }
        FairMutex::new(t)
    }

    fn text_at<T>(t: &Term<T>, m: &Match) -> String {
        t.bounds_to_string(*m.start(), *m.end())
    }

    #[test]
    fn counts_steps_and_wraps() {
        let t = term();
        let gen_arc = Arc::new(AtomicU64::new(1));
        let re = compile("NEEDLE", false, false).unwrap_or_else(|e| panic!("{e}"));
        let c = count(&t, re.clone(), &gen_arc, 1).unwrap_or_else(|| panic!("cancelled"));
        assert_eq!(c.starts.len(), 10);
        assert!(!c.capped);
        // Case-sensitive: no match.
        let cs = compile("NEEDLE", true, false).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(count(&t, cs, &gen_arc, 1).map(|c| c.starts.len()), Some(0));
        // A stale generation stops the count.
        assert!(count(&t, re.clone(), &gen_arc, 99).is_none());

        let term = t.lock();
        let mut re = re;
        let newest = first(&term, &mut re).unwrap_or_else(|| panic!("no match"));
        assert_eq!(text_at(&term, &newest), "needle");
        let hist = term.history_size();
        assert_eq!(c.index_of(&newest, hist), Some(10));
        let older = step(&term, &mut re, &newest, true).unwrap_or_else(|| panic!("no older"));
        assert_eq!(c.index_of(&older, hist), Some(9));
        let back = step(&term, &mut re, &older, false).unwrap_or_else(|| panic!("no newer"));
        assert_eq!(back, newest);
        // Past the newest: nothing newer within the scan; the counted list wraps.
        assert!(step(&term, &mut re, &newest, false).is_none());
        let wrapped = step_counted(&term, &mut re, &c, Some(&newest), false)
            .unwrap_or_else(|| panic!("no wrap"));
        assert_eq!(c.index_of(&wrapped, hist), Some(1));
        let wrapped_up = step_counted(&term, &mut re, &c, Some(&wrapped), true)
            .unwrap_or_else(|| panic!("no wrap up"));
        assert_eq!(wrapped_up, newest);
    }

    #[test]
    fn visible_matches_only_on_screen() {
        let t = term();
        let mut re = compile(r"line 19\d", false, true).unwrap_or_else(|e| panic!("{e}"));
        let term = t.lock();
        // Screen shows lines 191..199 plus the empty prompt row.
        let v = visible(&term, &mut re);
        assert!(v.len() >= 9 && v.len() <= 10, "{}", v.len());
        assert!(v.iter().all(|m| text_at(&term, m).starts_with("line 19")));
    }

    #[test]
    fn counted_positions_survive_new_output() {
        let t = term();
        let gen_arc = Arc::new(AtomicU64::new(1));
        let mut re = compile("needle", false, false).unwrap_or_else(|e| panic!("{e}"));
        let c = count(&t, re.clone(), &gen_arc, 1).unwrap_or_else(|| panic!("cancelled"));
        {
            let mut term = t.lock();
            let mut p: Processor = Processor::new();
            p.advance(&mut *term, b"more\r\nand more\r\n");
        }
        let term = t.lock();
        let newest = first(&term, &mut re).unwrap_or_else(|| panic!("no match"));
        assert_eq!(c.index_of(&newest, term.history_size()), Some(10));
    }

    #[test]
    fn patterns() {
        assert_eq!(pattern("a.b", false, false), r"(?i)a\.b");
        assert_eq!(pattern("a.b", true, true), "(?-i)a.b");
        assert!(compile("(", false, true).is_err_and(|e| e.contains("unclosed")));
        assert!(compile("(", false, false).is_ok());
    }
}
