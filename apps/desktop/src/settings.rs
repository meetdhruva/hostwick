//! App preferences, persisted in the profile database (`settings` table) and
//! exposed to every view as a GPUI global.

use gpui_kit::{App, Global};
use hostwick_core::{HighlightRule, LogMode, Snippet};
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
    /// Keyword highlighting rules applied to every terminal.
    pub highlight_rules: Vec<HighlightRule>,
    /// Snippets available in every terminal (hosts can add their own).
    pub snippets: Vec<Snippet>,
    /// Reconnect dropped sessions automatically (with backoff).
    pub auto_reconnect: bool,
    /// Reopen the previous session's tabs at startup.
    pub restore_tabs: bool,
    /// Command for "Edit" in the file browser; empty = system default app.
    /// `{file}` is replaced by the path, otherwise it is appended.
    pub external_editor: String,
    /// Folder for encrypted settings sync (OneDrive, Dropbox, git…); empty = off.
    pub sync_folder: String,
    /// UI language code (`en`, `hi`, …); empty = follow the OS.
    pub language: String,
    /// Send mouse clicks/scrolls to programs that ask (htop, mc, vim).
    /// Hold Shift to select text instead.
    pub mouse_reporting: bool,
    /// Show the snippet button bar above terminals.
    pub show_button_bar: bool,
    /// Lock saved passwords after this many idle minutes (0 = when the app
    /// quits).
    pub vault_auto_lock_minutes: u32,
    /// Sync settings at startup, after changes and every few minutes.
    pub sync_auto: bool,
    /// Host folders kept while empty (folders with hosts need no entry).
    pub folders: Vec<String>,
    /// Key files for quick connect and new hosts (chosen on the welcome
    /// screen); paths, so they stay on this computer.
    pub default_keys: Vec<String>,
    /// Size of the whole interface in percent (90–150), for this screen.
    pub ui_scale: u32,
}

/// Which way a settings sync goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Merge both ways.
    Sync,
    /// Make the sync file match this computer.
    Push,
    /// Make this computer match the sync file.
    Pull,
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
            highlight_rules: HighlightRule::defaults(),
            snippets: Vec::new(),
            auto_reconnect: true,
            restore_tabs: true,
            external_editor: String::new(),
            sync_folder: String::new(),
            language: String::new(),
            mouse_reporting: true,
            show_button_bar: true,
            vault_auto_lock_minutes: 0,
            sync_auto: true,
            folders: Vec::new(),
            default_keys: Vec::new(),
            ui_scale: 100,
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

    /// Save, and record the shareable part for settings sync.
    pub fn save(&self, store: &SqliteStore) -> Result<(), String> {
        self.save_local(store)?;
        store
            .set_app_settings(&self.synced_json())
            .map_err(|e| e.to_string())
    }

    /// Save on this computer only (applying settings received by sync).
    pub fn save_local(&self, store: &SqliteStore) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        store.set_setting(KEY, &json).map_err(|e| e.to_string())
    }

    /// The settings other computers get: everything except what belongs to
    /// this computer (paths, the sync folder, the app log level, default
    /// key files).
    pub fn synced_json(&self) -> String {
        let shared = Self {
            session_log_dir: String::new(),
            external_editor: String::new(),
            sync_folder: String::new(),
            log_level: String::new(),
            default_keys: Vec::new(),
            ui_scale: 100,
            ..self.clone()
        };
        serde_json::to_string(&shared).unwrap_or_default()
    }

    /// Settings received from another computer, keeping this computer's
    /// own paths. Unreadable input leaves `current` as it is.
    pub fn from_synced(json: &str, current: &Self) -> Self {
        match serde_json::from_str::<Self>(json) {
            Ok(s) => Self {
                session_log_dir: current.session_log_dir.clone(),
                external_editor: current.external_editor.clone(),
                sync_folder: current.sync_folder.clone(),
                log_level: current.log_level.clone(),
                default_keys: current.default_keys.clone(),
                ui_scale: current.ui_scale,
                ..s
            }
            .sanitized(),
            Err(e) => {
                tracing::warn!(error = %e, "ignoring unreadable synced settings");
                current.clone()
            }
        }
    }

    /// Clamp values a hand-edited database might contain.
    pub fn sanitized(mut self) -> Self {
        self.font_size = self.font_size.clamp(6.0, 48.0);
        self.ui_scale = crate::zoom::clamp_ui_scale(self.ui_scale);
        self.vault_auto_lock_minutes = self.vault_auto_lock_minutes.min(24 * 60);
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
    if theme == ThemeChoice::Light {
        // The stock light theme's muted text (#737373) falls to about 4.3:1
        // on its grey panels and 3.8:1 on secondary buttons, under the WCAG
        // 4.5:1 for normal text. #595959 keeps 6.5:1 or better on all of them
        // and still reads as secondary. The dark theme's muted text is
        // already 6:1 or better.
        Theme::global_mut(cx).muted_foreground = gpui_kit::rgb(0x595959).into();
    }
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

    #[test]
    fn synced_settings_keep_local_paths() {
        let store = SqliteStore::in_memory().unwrap_or_else(|e| panic!("{e}"));
        let here = Settings {
            session_log_dir: "/home/me/logs".into(),
            external_editor: "code -w".into(),
            log_level: "debug".into(),
            font_size: 12.0,
            ..Default::default()
        };
        here.save(&store).unwrap_or_else(|e| panic!("{e}"));
        let (json, _) = store
            .app_settings()
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_default();
        assert!(!json.contains("/home/me/logs") && !json.contains("code -w"));
        let there = Settings {
            session_log_dir: r"C:\logs".into(),
            log_level: "warn".into(),
            ..Default::default()
        };
        let merged = Settings::from_synced(&json, &there);
        assert_eq!(merged.font_size, 12.0);
        assert_eq!(merged.session_log_dir, r"C:\logs");
        assert_eq!(merged.log_level, "warn");
        assert_eq!(Settings::from_synced("nonsense", &there), there);
    }
}
