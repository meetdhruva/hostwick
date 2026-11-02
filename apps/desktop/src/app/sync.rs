//! Settings sync from the desktop: at startup, a few seconds after hosts or
//! settings change, every five minutes, and on demand. Argon2 and file I/O
//! run on a blocking thread.

use std::time::{Duration, Instant};

use crate::notify::{Toast, ToastExt as _};
use gpui_kit::*;
use hostwick_core::SecretString;
use hostwick_store::{SettingsSync, SyncError, SyncReport};

use super::HostwickApp;
use crate::settings::{Settings, SyncMode};
use crate::vault::SYNC_PASSPHRASE_ID;

/// Settings key of the sync folder (shared with `hostwick settings-sync`).
pub const FOLDER_KEY: &str = "sync.folder";
/// Last sync result, for the settings dialog.
pub const LAST_KEY: &str = "desktop.sync.last";

const DEBOUNCE: Duration = Duration::from_secs(5);
const PERIOD: Duration = Duration::from_secs(5 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct SyncState {
    dirty_since: Option<Instant>,
    last_run: Option<Instant>,
    retry_at: Option<Instant>,
    running: bool,
}

impl SyncState {
    pub fn load() -> Self {
        Self::default()
    }
}

/// When the scheduler should sync now.
pub(crate) fn sync_due(s: &SyncState, now: Instant) -> bool {
    if s.running {
        return false;
    }
    s.dirty_since
        .is_some_and(|t| now.duration_since(t) >= DEBOUNCE)
        || s.retry_at.is_some_and(|t| now >= t)
        || s.last_run.is_some_and(|t| now.duration_since(t) >= PERIOD)
}

/// One line describing a finished sync.
pub fn summarize(r: &SyncReport) -> String {
    let here = if r.local_unchanged() {
        t!("sync-no-changes").to_string()
    } else {
        let mut parts = Vec::new();
        if !r.added.is_empty() {
            parts.push(t!("sync-added", count = r.added.len()).to_string());
        }
        if !r.updated.is_empty() {
            parts.push(t!("sync-updated", count = r.updated.len()).to_string());
        }
        if !r.deleted.is_empty() {
            parts.push(t!("sync-removed", count = r.deleted.len()).to_string());
        }
        if r.app_settings.is_some() {
            parts.push(t!("sync-settings-updated").to_string());
        }
        parts.join(", ")
    };
    let mode = match r.mode {
        "push" => t!("sync-mode-push"),
        "pull" => t!("sync-mode-pull"),
        _ => t!("sync-mode-sync"),
    };
    let line = t!(
        "sync-summary",
        mode = mode,
        count = r.profiles,
        changes = here
    );
    if r.uploaded {
        format!("{line}; {}", t!("sync-file-updated"))
    } else {
        line.into()
    }
}

enum Outcome {
    Done(SyncReport),
    /// Nothing to do (no folder, vault locked and not asking).
    Skipped,
    Failed {
        message: String,
        busy: bool,
    },
}

impl HostwickApp {
    fn sync_folder(&self) -> Option<String> {
        self.backend
            .store
            .setting(FOLDER_KEY)
            .ok()
            .flatten()
            .filter(|f| !f.trim().is_empty())
    }

    pub(crate) fn mark_sync_dirty(&mut self) {
        if self.sync_folder().is_some() {
            self.sync.dirty_since = Some(Instant::now());
        }
    }

    pub(super) fn sync_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !Settings::get(cx).sync_auto || self.sync_folder().is_none() {
            return;
        }
        // Only in the background while the vault is open: never prompt.
        if self.backend.vault.is_unlocked() && sync_due(&self.sync, Instant::now()) {
            self.run_sync(SyncMode::Sync, false, window, cx);
        }
    }

    pub(super) fn startup_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !Settings::get(cx).sync_auto || self.sync_folder().is_none() {
            return;
        }
        let backend = self.backend.clone();
        let job = self.backend.rt.spawn(async move {
            backend
                .ensure_vault(t!("vault-reason-sync-startup").into())
                .await
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(true) = job.await {
                let _ = this.update_in(cx, |app, window, cx| {
                    app.run_sync(SyncMode::Sync, false, window, cx)
                });
            }
        })
        .detach();
    }

    /// Sync now. `interactive`: ask for the vault and the passphrase when
    /// needed and report the result; otherwise run quietly.
    pub(crate) fn run_sync(
        &mut self,
        mode: SyncMode,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self.sync_folder() else {
            if interactive {
                window.toast(Toast::warning(t!("sync-no-folder")), cx);
            }
            return;
        };
        if self.sync.running {
            return;
        }
        self.sync.running = true;
        self.sync.dirty_since = None;
        self.sync.retry_at = None;
        let backend = self.backend.clone();
        let job = self.backend.rt.spawn(async move {
            if !backend.vault.is_unlocked()
                && (!interactive || !backend.ensure_vault(t!("vault-reason-sync").into()).await)
            {
                return Outcome::Skipped;
            }
            let mut pass = backend.vault.get(SYNC_PASSPHRASE_ID);
            for attempt in 0..2 {
                let Some(p) = pass.clone() else {
                    if !interactive {
                        return Outcome::Failed {
                            message: t!("sync-no-passphrase").into(),
                            busy: false,
                        };
                    }
                    pass = backend
                        .ask_secret(
                            t!("sync-passphrase-title").into(),
                            t!("sync-passphrase-label").into(),
                        )
                        .await;
                    if pass.is_none() {
                        return Outcome::Skipped;
                    }
                    continue;
                };
                let b = backend.clone();
                let f = folder.clone();
                let p2: SecretString = p.clone();
                let res = backend
                    .rt
                    .spawn_blocking(move || {
                        let s = SettingsSync::new(&b.store, &f, p2);
                        match mode {
                            SyncMode::Sync => s.sync(),
                            SyncMode::Push => s.push(),
                            SyncMode::Pull => s.pull(),
                        }
                    })
                    .await;
                match res {
                    Ok(Ok(report)) => {
                        // Keep a passphrase that worked.
                        if backend
                            .vault
                            .get(SYNC_PASSPHRASE_ID)
                            .is_none_or(|old| !crate::vault::same_secret(&old, &p))
                        {
                            let _ = backend.vault.set(SYNC_PASSPHRASE_ID, p);
                        }
                        return Outcome::Done(report);
                    }
                    Ok(Err(SyncError::WrongPassphrase(_))) if interactive && attempt == 0 => {
                        pass = backend
                            .ask_secret(
                                t!("sync-passphrase-wrong-title").into(),
                                t!("sync-passphrase-label").into(),
                            )
                            .await;
                        if pass.is_none() {
                            return Outcome::Skipped;
                        }
                    }
                    Ok(Err(e)) => {
                        return Outcome::Failed {
                            busy: matches!(e, SyncError::Busy(_)),
                            message: e.to_string(),
                        };
                    }
                    Err(e) => {
                        return Outcome::Failed {
                            message: t!("error-internal", error = e.to_string()).into(),
                            busy: false,
                        };
                    }
                }
            }
            Outcome::Failed {
                message: t!("sync-wrong-passphrase").into(),
                busy: false,
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = job.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.sync.running = false;
                app.sync.last_run = Some(Instant::now());
                let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
                match outcome {
                    Ok(Outcome::Done(report)) => {
                        let text = summarize(&report);
                        tracing::info!("settings sync: {text}");
                        let _ = app
                            .backend
                            .store
                            .set_setting(LAST_KEY, &format!("{stamp} — {text}"));
                        app.apply_sync_report(&report, cx);
                        app.apply_language(window, cx);
                        if interactive || !report.local_unchanged() {
                            window.toast(Toast::success(t!("sync-done", summary = text)), cx);
                        }
                    }
                    Ok(Outcome::Skipped) => {}
                    Ok(Outcome::Failed { message, busy }) => {
                        tracing::warn!("settings sync failed: {message}");
                        let _ = app.backend.store.set_setting(
                            LAST_KEY,
                            &format!(
                                "{stamp} — {}",
                                t!("sync-last-failed", error = message.clone())
                            ),
                        );
                        if busy {
                            app.sync.retry_at = Some(Instant::now() + BUSY_RETRY);
                        }
                        if interactive || !busy {
                            window.toast(Toast::error(t!("sync-failed", error = message)), cx);
                        }
                    }
                    Err(e) => tracing::warn!("settings sync task: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_sync_report(&mut self, report: &SyncReport, cx: &mut Context<Self>) {
        if let Some(json) = &report.app_settings {
            let current = Settings::get(cx).clone();
            let new = Settings::from_synced(json, &current);
            if new != current {
                if let Err(e) = new.save_local(&self.backend.store) {
                    tracing::warn!("cannot save synced settings: {e}");
                }
                if new.theme != current.theme {
                    crate::settings::apply_theme(new.theme, cx);
                }
                if new.log_level != current.log_level {
                    crate::logging::set_level(&new.log_level);
                }
                if new.language != current.language {
                    crate::i18n::set_language(&new.language);
                }
                cx.set_global(new);
                self.apply_settings_to_views(cx);
            }
        }
        if !report.local_unchanged() {
            self.reload_profiles();
            for t in &mut self.tabs {
                for pane in t.root.leaves_mut() {
                    if let Some(p) = self.profiles.iter().find(|p| p.id == pane.profile.id) {
                        pane.profile = p.clone();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SyncState, sync_due};
    use std::time::{Duration, Instant};

    #[test]
    fn debounce_period_and_retry() {
        let now = Instant::now();
        let mut s = SyncState::default();
        assert!(!sync_due(&s, now));
        s.dirty_since = Some(now);
        assert!(!sync_due(&s, now + Duration::from_secs(2)));
        assert!(sync_due(&s, now + Duration::from_secs(6)));
        s.running = true;
        assert!(!sync_due(&s, now + Duration::from_secs(6)));
        let s = SyncState {
            last_run: Some(now),
            ..Default::default()
        };
        assert!(!sync_due(&s, now + Duration::from_secs(60)));
        assert!(sync_due(&s, now + Duration::from_secs(301)));
        let s = SyncState {
            retry_at: Some(now + Duration::from_secs(30)),
            ..Default::default()
        };
        assert!(sync_due(&s, now + Duration::from_secs(31)));
    }
}
