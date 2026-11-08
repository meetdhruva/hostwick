//! What the tab and window code needs from a terminal: its text for a closed
//! tab to come back with, per-tab zoom, and a note that the view moved to
//! another window.

use super::*;

/// Lines kept from a closed tab's scrollback.
pub const HISTORY_LINES: usize = 3000;

impl TerminalView {
    /// The scrollback and screen as plain text (newest [`HISTORY_LINES`]),
    /// saved when a tab closes so reopening it can show it again.
    pub fn history_text(&self) -> String {
        self.scrollback_text(HISTORY_LINES)
    }

    /// Show `text` (an earlier session's output, dimmed) above everything
    /// else, then the "reconnected" marker line. Call right after the new
    /// session starts; the shell's first output lands below.
    pub fn show_history(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            return;
        }
        {
            let mut term = self.shared.term.lock();
            let mut p: Processor = Processor::new();
            let mut out = String::with_capacity(text.len() + 256);
            out.push_str("\x1b[0;2m");
            for line in text.lines() {
                // Plain text only: no stray escape sequences from the old session.
                out.extend(line.chars().filter(|c| !c.is_control() || *c == '\t'));
                out.push_str("\r\n");
            }
            let stamp = chrono::Local::now().format("%H:%M:%S");
            out.push_str(&format!(
                "\x1b[0;2m── {} ──\x1b[0m\r\n",
                t!("term-reconnected-marker", time = stamp.to_string())
            ));
            p.advance(&mut *term, out.as_bytes());
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
        }
        cx.notify();
    }

    /// Zoom to `pct` percent of the settings font size (clamped).
    pub fn set_zoom(&mut self, pct: u32, cx: &mut Context<Self>) {
        self.zoom = crate::zoom::clamp_zoom(pct);
        self.font_size = px(crate::zoom::zoomed_font(
            Settings::get(cx).font_size,
            self.zoom,
        ));
        cx.notify();
    }

    /// The view now lives in another window: parts that were created for
    /// the old one (the search bar's input) are dropped.
    pub fn window_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.search.take() {
            s.generation.fetch_add(1, Ordering::AcqRel);
        }
        cx.notify();
    }
}
