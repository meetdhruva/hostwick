//! App preferences, persisted in the profile database (`settings` table) and
//! exposed to every view as a GPUI global.

use gpui_kit::{App, Global};
use hostwick_core::LogMode;
use hostwick_store::SqliteStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RightClick {
    /// Context menu (Copy, Paste, Clear, Log…).
    Menu,
    /// PuTTY behaviour: right-click pastes.
    Paste,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Empty = OS default monospace font.
    pub font_family: String,
    pub font_size: f32,
    pub theme: ThemeChoice,
    pub scrollback_lines: usize,
    pub right_click: RightClick,
    /// PuTTY habit: selecting text copies it.
    pub copy_on_select: bool,
    /// Diagnostic log level for the app log file: error/warn/info/debug/trace.
    pub log_level: String,
    /// Folder for session logs with relative paths; empty = app log folder.
    pub session_log_dir: String,
    /// Log every session (unless the host has its own setting).
    pub log_all_sessions: bool,
    pub session_log_mode: LogMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 14.0,
            theme: ThemeChoice::Dark,
            scrollback_lines: 10_000,
            right_click: RightClick::Menu,
            copy_on_select: true,
            log_level: "info".into(),
            session_log_dir: String::new(),
            log_all_sessions: false,
            session_log_mode: LogMode::Printable,
        }
    }
}

impl Global for Settings {}

const KEY: &str = "desktop.settings";

impl Settings {
    pub fn load(store: &SqliteStore) -> Self {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Self>(&json).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn save(&self, store: &SqliteStore) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        store.set_setting(KEY, &json).map_err(|e| e.to_string())
    }

    /// Clamp values a hand-edited database might contain.
    pub fn sanitized(mut self) -> Self {
        self.font_size = self.font_size.clamp(6.0, 48.0);
        self.scrollback_lines = self.scrollback_lines.clamp(100, 1_000_000);
        if !matches!(
            self.log_level.as_str(),
            "error" | "warn" | "info" | "debug" | "trace"
        ) {
            self.log_level = "info".into();
        }
        self
    }

    pub fn session_log_dir(&self, paths: &hostwick_store::AppPaths) -> std::path::PathBuf {
        if self.session_log_dir.trim().is_empty() {
            paths.logs.join("sessions")
        } else {
            std::path::PathBuf::from(self.session_log_dir.trim())
        }
    }

    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }
}

pub fn apply_theme(theme: ThemeChoice, cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let mode = match theme {
        ThemeChoice::Dark => ThemeMode::Dark,
        ThemeChoice::Light => ThemeMode::Light,
    };
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_old_or_bad_data() {
        let store = SqliteStore::in_memory().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Settings::load(&store), Settings::default());
        let s = Settings {
            font_size: 18.0,
            right_click: RightClick::Paste,
            ..Default::default()
        };
        s.save(&store).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Settings::load(&store), s);
        // Missing fields fall back to defaults; out-of-range values are clamped.
        store
            .set_setting(KEY, r#"{"font_size": 400, "log_level": "loud"}"#)
            .unwrap_or_else(|e| panic!("{e}"));
        let l = Settings::load(&store);
        assert_eq!(
            (l.font_size, l.log_level.as_str(), l.copy_on_select),
            (48.0, "info", true)
        );
    }
}
