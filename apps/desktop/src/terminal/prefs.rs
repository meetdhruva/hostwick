//! Terminal preferences added in v0.4 (paste protection, finished-command
//! alerts, status bar, clickable output). Kept in one struct so the main
//! settings only carry a single extra field.

use serde::{Deserialize, Serialize};

use super::paste::PasteConfirm;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TermPrefs {
    /// Confirm multi-line pastes (production hosts always ask).
    pub confirm_paste: PasteConfirm,
    /// Notify when a long command finishes in a tab you are not looking at.
    pub notify_finished: bool,
    /// What counts as long, in seconds.
    pub finished_after_secs: u32,
    /// The status bar at the bottom of terminal tabs.
    pub status_bar: bool,
    /// CPU, memory, disk and load in the status bar (runs a small command
    /// on the server every few seconds while the tab is visible).
    pub server_metrics: bool,
    /// Underline and open URLs, paths and addresses in the output.
    pub clickable_output: bool,
}

impl Default for TermPrefs {
    fn default() -> Self {
        Self {
            confirm_paste: PasteConfirm::Always,
            notify_finished: true,
            finished_after_secs: 10,
            status_bar: true,
            server_metrics: false,
            clickable_output: true,
        }
    }
}

impl TermPrefs {
    /// Values in range, whatever a settings file says.
    pub fn clamped(mut self) -> Self {
        self.finished_after_secs = self.finished_after_secs.clamp(1, 86_400);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_files() {
        let d = TermPrefs::default();
        assert_eq!(d.confirm_paste, PasteConfirm::Always);
        assert_eq!(d.finished_after_secs, 10);
        assert!(d.status_bar && d.notify_finished && !d.server_metrics);
        let p: TermPrefs = serde_json::from_str(r#"{"server_metrics": true}"#).unwrap_or_default();
        assert!(p.server_metrics && p.status_bar);
        let bad: TermPrefs =
            serde_json::from_str(r#"{"finished_after_secs": 0}"#).unwrap_or_default();
        assert_eq!(bad.clamped().finished_after_secs, 1);
    }
}
