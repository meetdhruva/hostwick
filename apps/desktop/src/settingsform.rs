//! Preferences dialog, in sections. Saving persists to the database (and
//! records the shareable part for settings sync), updates the global, and
//! tells the app so open terminals pick up the change.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, LogMode, ProfileStore as _, SecretString};

use crate::backend::Backend;
use crate::editors::{RuleEditor, SnippetEditor};
use crate::logging::LEVELS;
use crate::settings::{RightClick, Settings, SyncMode, ThemeChoice, apply_theme};
use crate::vault::{SYNC_PASSPHRASE_ID, assign_id, describe, profile_ids};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Terminal,
    Snippets,
    Connections,
    Files,
    Logging,
    Passwords,
    Sync,
    Diagnostics,
}

impl Section {
    const ALL: [Section; 9] = [
        Section::Appearance,
        Section::Terminal,
        Section::Snippets,
        Section::Connections,
        Section::Files,
        Section::Logging,
        Section::Passwords,
        Section::Sync,
        Section::Diagnostics,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Terminal => "Terminal",
            Section::Snippets => "Snippets",
            Section::Connections => "Connections",
            Section::Files => "Files",
            Section::Logging => "Session logging",
            Section::Passwords => "Saved passwords",
            Section::Sync => "Settings sync",
            Section::Diagnostics => "Diagnostics",
        }
    }
}

/// What the dialog tells the app.
pub enum SettingsEvent {
    Saved,
    SyncNow(SyncMode),
    /// Saved hosts changed (a saved password was removed).
    ProfilesChanged,
}

type OnEvent = Rc<dyn Fn(SettingsEvent, &mut Window, &mut App)>;

pub struct SettingsForm {
    backend: Arc<Backend>,
    profiles: Vec<HostProfile>,
    section: Section,
    base: Settings,
    font: Entity<InputState>,
    size: Entity<InputState>,
    scroll: Entity<InputState>,
    logdir: Entity<InputState>,
    editor: Entity<InputState>,
    auto_lock: Entity<InputState>,
    sync_folder: Entity<InputState>,
    sync_pass: Entity<InputState>,
    theme: ThemeChoice,
    right: RightClick,
    mode: LogMode,
    level: usize,
    copy_sel: bool,
    log_all: bool,
    mouse: bool,
    button_bar: bool,
    auto_reconnect: bool,
    restore_tabs: bool,
    sync_auto: bool,
    rules: Entity<RuleEditor>,
    snippets: Entity<SnippetEditor>,
    /// Saved credentials: (id, description); None while loading.
    creds: Option<Vec<(String, String)>>,
    error: Option<String>,
    on_event: OnEvent,
}

impl SettingsForm {
    fn new(
        backend: Arc<Backend>,
        profiles: Vec<HostProfile>,
        section: Section,
        on_event: OnEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let s = Settings::get(cx).clone();
        let mk = |v: String, ph: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(v))
        };
        let default_dir = s.session_log_dir(&backend.paths).display().to_string();
        let folder = backend
            .store
            .setting(crate::app::sync_folder_key())
            .ok()
            .flatten()
            .unwrap_or_default();
        let has_pass = backend.vault.is_unlocked() && backend.vault.has(SYNC_PASSPHRASE_ID);
        let mut form = Self {
            font: mk(s.font_family.clone(), "empty = system monospace".into(), window, cx),
            size: mk(s.font_size.to_string(), "14".into(), window, cx),
            scroll: mk(s.scrollback_lines.to_string(), "10000".into(), window, cx),
            logdir: mk(s.session_log_dir.clone(), default_dir, window, cx),
            editor: mk(
                s.external_editor.clone(),
                "empty = the system's default app (e.g. code -w {file})".into(),
                window,
                cx,
            ),
            auto_lock: mk(
                s.vault_auto_lock_minutes.to_string(),
                "0 = keep unlocked until Hostwick quits".into(),
                window,
                cx,
            ),
            sync_folder: mk(folder, "OneDrive, Dropbox, iCloud Drive or git folder".into(), window, cx),
            sync_pass: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(if has_pass {
                        "Saved in your vault — type to change"
                    } else {
                        "Same passphrase on every computer"
                    })
            }),
            theme: s.theme,
            right: s.right_click,
            mode: s.session_log_mode,
            level: LEVELS.iter().position(|l| *l == s.log_level).unwrap_or(2),
            copy_sel: s.copy_on_select,
            log_all: s.log_all_sessions,
            mouse: s.mouse_reporting,
            button_bar: s.show_button_bar,
            auto_reconnect: s.auto_reconnect,
            restore_tabs: s.restore_tabs,
            sync_auto: s.sync_auto,
            rules: cx.new(|cx| RuleEditor::new(&s.highlight_rules, window, cx)),
            snippets: cx.new(|cx| {
                SnippetEditor::new(
                    &s.snippets,
                    "No snippets yet. Snippets appear as buttons above every terminal and under right-click → Snippets.",
                    window,
                    cx,
                )
            }),
            base: s,
            backend,
            profiles,
            section,
            creds: None,
            error: None,
            on_event,
        };
        form.load_creds(window, cx);
        form
    }

    /// List saved credentials off the UI thread (re-reading the vault may
    /// run Argon2).
    fn load_creds(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.backend.vault.is_unlocked() {
            self.creds = Some(Vec::new());
            return;
        }
        self.creds = None;
        let vault = self.backend.vault.clone();
        let job = self.backend.rt.spawn_blocking(move || vault.ids());
        cx.spawn_in(window, async move |this, cx| {
            let ids = job.await.unwrap_or_default();
            let _ = this.update(cx, |f, cx| {
                let mut rows: Vec<(String, String)> = ids
                    .into_iter()
                    .map(|id| {
                        let d = describe(&id, &f.profiles);
                        (id, d)
                    })
                    .collect();
                rows.sort_by_key(|a| a.1.to_lowercase());
                f.creds = Some(rows);
                cx.notify();
            });
        })
        .detach();
    }

    fn forget(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let id2 = id.clone();
        let job = self.backend.rt.spawn_blocking(move || {
            backend.vault.remove(&id2)?;
            // Hosts stop referring to it (they will prompt again).
            let mut changed = 0;
            for mut p in backend.store.list().map_err(|e| e.to_string())? {
                let mut touched = false;
                for (kind, pid) in profile_ids(&p) {
                    if pid == id2 {
                        touched |= assign_id(&mut p, &kind, None);
                    }
                }
                if touched {
                    backend.store.upsert(&p).map_err(|e| e.to_string())?;
                    changed += 1;
                }
            }
            Ok::<_, String>(changed)
        });
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |f, window, cx| {
                match res {
                    Ok(Ok(changed)) => {
                        if changed > 0 {
                            f.profiles = f.backend.store.list().unwrap_or_default();
                            (f.on_event)(SettingsEvent::ProfilesChanged, window, cx);
                        }
                        f.error = None;
                    }
                    Ok(Err(e)) => f.error = Some(format!("Could not remove it: {e}")),
                    Err(e) => f.error = Some(format!("internal error: {e}")),
                }
                f.load_creds(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn unlock_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let create = !self.backend.vault.exists();
        let this = cx.entity().downgrade();
        crate::vaultui::open_unlock(
            self.backend.clone(),
            create,
            String::new(),
            window,
            cx,
            move |_, window, cx| {
                let _ = this.update(cx, |f, cx| {
                    f.load_creds(window, cx);
                    cx.notify();
                });
            },
        );
    }

    /// Validate and build the new settings.
    fn collect(&self, cx: &App) -> Result<Settings, String> {
        let v = |i: &Entity<InputState>| i.read(cx).value().trim().to_owned();
        let font_size = v(&self.size)
            .parse::<f32>()
            .ok()
            .filter(|s| (6.0..=48.0).contains(s))
            .ok_or("Terminal → Font size must be a number from 6 to 48")?;
        let scrollback = v(&self.scroll)
            .parse::<usize>()
            .ok()
            .filter(|s| (100..=1_000_000).contains(s))
            .ok_or("Terminal → Scrollback must be a whole number from 100 to 1000000")?;
        let auto_lock = v(&self.auto_lock)
            .parse::<u32>()
            .ok()
            .filter(|m| *m <= 24 * 60)
            .ok_or("Saved passwords → Lock after must be minutes (0 to 1440)")?;
        if let Some(p) = self.rules.read(cx).problem(cx) {
            return Err(format!("Terminal → {p}"));
        }
        let folder = v(&self.sync_folder);
        if !folder.is_empty() && !std::path::Path::new(&folder).is_dir() {
            return Err(format!(
                "Settings sync → the folder {folder} does not exist"
            ));
        }
        Ok(Settings {
            font_family: v(&self.font),
            font_size,
            theme: self.theme,
            scrollback_lines: scrollback,
            right_click: self.right,
            copy_on_select: self.copy_sel,
            log_level: LEVELS.get(self.level).copied().unwrap_or("info").to_owned(),
            session_log_dir: v(&self.logdir),
            log_all_sessions: self.log_all,
            session_log_mode: self.mode,
            highlight_rules: self.rules.read(cx).values(cx),
            snippets: self.snippets.read(cx).values(cx),
            auto_reconnect: self.auto_reconnect,
            restore_tabs: self.restore_tabs,
            external_editor: v(&self.editor),
            mouse_reporting: self.mouse,
            show_button_bar: self.button_bar,
            vault_auto_lock_minutes: auto_lock,
            sync_auto: self.sync_auto,
            ..self.base.clone()
        }
        .sanitized())
    }

    /// Store the sync folder and (in the vault) a newly typed passphrase.
    /// Calls `then` once done; asks to unlock the vault if needed.
    fn store_sync_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let folder = self.sync_folder.read(cx).value().trim().to_owned();
        if !folder.is_empty() && !std::path::Path::new(&folder).is_dir() {
            self.error = Some(format!("The folder {folder} does not exist."));
            cx.notify();
            return;
        }
        let _ = self
            .backend
            .store
            .set_setting(crate::app::sync_folder_key(), &folder);
        let pass = self.sync_pass.read(cx).value().to_string();
        if pass.is_empty() {
            then(self, window, cx);
            return;
        }
        if pass.chars().count() < 8 {
            self.error = Some("Use a sync passphrase of at least 8 characters.".into());
            cx.notify();
            return;
        }
        let save = move |f: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            match f
                .backend
                .vault
                .set(SYNC_PASSPHRASE_ID, SecretString::from(pass))
            {
                Ok(()) => {
                    f.sync_pass.update(cx, |s, cx| {
                        s.set_value("", window, cx);
                        s.set_placeholder("Saved in your vault — type to change", window, cx);
                    });
                    then(f, window, cx);
                }
                Err(e) => f.error = Some(format!("Could not save the passphrase: {e}")),
            }
            cx.notify();
        };
        if self.backend.vault.is_unlocked() {
            save(self, window, cx);
            return;
        }
        let this = cx.entity().downgrade();
        crate::vaultui::open_unlock(
            self.backend.clone(),
            !self.backend.vault.exists(),
            "The sync passphrase is kept in your password vault.".into(),
            window,
            cx,
            move |ok, window, cx| {
                if ok {
                    let _ = this.update(cx, |f, cx| save(f, window, cx));
                }
            },
        );
    }

    fn sync_now(&mut self, mode: SyncMode, window: &mut Window, cx: &mut Context<Self>) {
        self.store_sync_settings(window, cx, move |f, window, cx| {
            (f.on_event)(SettingsEvent::SyncNow(mode), window, cx);
            f.error = None;
            cx.notify();
        });
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new = match self.collect(cx) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        if let Err(e) = new.save(&self.backend.store) {
            self.error = Some(format!("Cannot save settings: {e}"));
            cx.notify();
            return;
        }
        if new.log_level != self.base.log_level {
            crate::logging::set_level(&new.log_level);
        }
        if new.theme != self.base.theme {
            apply_theme(new.theme, cx);
        }
        self.base = new.clone();
        cx.set_global(new);
        self.store_sync_settings(window, cx, |f, window, cx| {
            (f.on_event)(SettingsEvent::Saved, window, cx);
            window.defer(cx, |window, cx| {
                window.close_dialog(cx);
                window.refresh();
            });
        });
    }

    // ---- rendering helpers ------------------------------------------------------

    fn row(label: &'static str, el: impl IntoElement) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .text_size(px(13.0))
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(el))
    }

    fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
        div()
            .pl(px(158.0))
            .text_size(px(11.0))
            .whitespace_normal()
            .text_color(cx.theme().muted_foreground)
            .child(text.into())
    }

    fn heading(text: &'static str) -> Div {
        div()
            .pt(px(6.0))
            .text_size(px(15.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
    }

    fn check(
        id: &'static str,
        label: &'static str,
        value: bool,
        cx: &mut Context<Self>,
        set: fn(&mut Self, bool),
    ) -> Div {
        div()
            .pl(px(158.0))
            .child(
                Checkbox::new(id)
                    .label(label)
                    .checked(value)
                    .on_click(cx.listener(move |f, v: &bool, _, cx| {
                        set(f, *v);
                        cx.notify();
                    })),
            )
    }

    fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        current: T,
        options: &[(T, &'static str)],
        cx: &mut Context<Self>,
        set: fn(&mut Self, T),
    ) -> Div {
        let mut row = div().flex().gap(px(4.0)).flex_wrap();
        for (i, (value, label)) in options.iter().copied().enumerate() {
            row = row.child(
                Button::new((id, i))
                    .xsmall()
                    .label(label)
                    .when(current == value, |b| b.primary())
                    .on_click(cx.listener(move |f, _, _, cx| {
                        set(f, value);
                        cx.notify();
                    })),
            );
        }
        row
    }

    fn browse(
        id: &'static str,
        input: &Entity<InputState>,
        directories: bool,
        prompt: &'static str,
    ) -> Button {
        let input = input.clone();
        Button::new(id)
            .small()
            .label("Browse…")
            .on_click(move |_, window, cx| {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: !directories,
                    directories,
                    multiple: false,
                    prompt: Some(prompt.into()),
                });
                let input = input.clone();
                window
                    .spawn(cx, async move |cx| {
                        let Ok(Ok(Some(paths))) = picked.await else {
                            return;
                        };
                        if let Some(p) = paths.into_iter().next() {
                            let text = p.display().to_string();
                            let _ = cx.update(|window, cx| {
                                input.update(cx, |s, cx| s.set_value(text, window, cx))
                            });
                        }
                    })
                    .detach();
            })
    }

    fn render_section(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let body = div().flex().flex_col().gap(px(8.0));
        match self.section {
            Section::Appearance => body
                .child(Self::heading("Appearance"))
                .child(Self::row(
                    "Theme",
                    Self::choice(
                        "theme",
                        self.theme,
                        &[(ThemeChoice::Dark, "Dark"), (ThemeChoice::Light, "Light")],
                        cx,
                        |f, v| f.theme = v,
                    ),
                ))
                .child(Self::row("Terminal font", Input::new(&self.font).small()))
                .child(Self::row("Font size", Input::new(&self.size).small()))
                .child(Self::hint("Font changes apply to open terminals when you save.", cx)),
            Section::Terminal => body
                .child(Self::heading("Terminal"))
                .child(Self::row("Scrollback lines", Input::new(&self.scroll).small()))
                .child(Self::row(
                    "Right-click",
                    Self::choice(
                        "rc",
                        self.right,
                        &[
                            (RightClick::Menu, "Show menu"),
                            (RightClick::Paste, "Paste (PuTTY)"),
                        ],
                        cx,
                        |f, v| f.right = v,
                    ),
                ))
                .child(Self::check("copy-sel", "Copy selected text automatically", self.copy_sel, cx, |f, v| f.copy_sel = v))
                .child(Self::check("mouse", "Send mouse clicks and scrolling to programs (htop, mc, vim)", self.mouse, cx, |f, v| f.mouse = v))
                .child(Self::hint("Hold Shift to select text while a program uses the mouse.", cx))
                .child(Self::check("bar", "Show the snippet button bar above terminals", self.button_bar, cx, |f, v| f.button_bar = v))
                .child(
                    div()
                        .pt(px(6.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(13.0))
                        .child("Keyword highlighting"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(muted)
                        .child("Colours matching text on screen. Earlier rules win where they overlap."),
                )
                .child(self.rules.clone()),
            Section::Snippets => body
                .child(Self::heading("Snippets"))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(muted)
                        .whitespace_normal()
                        .child("Available in every terminal. Hosts can add their own in Edit host → Terminal."),
                )
                .child(self.snippets.clone()),
            Section::Connections => body
                .child(Self::heading("Connections"))
                .child(Self::check("auto-rc", "Reconnect automatically when a connection drops", self.auto_reconnect, cx, |f, v| f.auto_reconnect = v))
                .child(Self::hint("SSH and telnet: after 1, 2, 5, 10, then every 30 seconds, keeping the scrollback. Not after you type exit, nor after a login or host-key failure.", cx))
                .child(Self::check("restore", "Reopen my tabs when Hostwick starts", self.restore_tabs, cx, |f, v| f.restore_tabs = v))
                .child(Self::hint("Including split panes. Tabs connect when you first switch to them.", cx)),
            Section::Files => body
                .child(Self::heading("Files"))
                .child(Self::row(
                    "External editor",
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(div().flex_1().child(Input::new(&self.editor).small()))
                        .child(Self::browse("browse-editor", &self.editor, false, "Editor program")),
                ))
                .child(Self::hint("Used by Edit (F4). {file} is replaced by the file's path, otherwise the path is added at the end. Use the editor's wait flag (code -w, subl -w) if it has one.", cx)),
            Section::Logging => body
                .child(Self::heading("Session logging"))
                .child(Self::check("log-all", "Log every session to a file", self.log_all, cx, |f, v| f.log_all = v))
                .child(Self::row("Log folder", Input::new(&self.logdir).small()))
                .child(Self::row(
                    "Log content",
                    Self::choice(
                        "lm",
                        self.mode,
                        &[
                            (LogMode::Printable, "Printable text"),
                            (LogMode::Raw, "Raw (with escape codes)"),
                        ],
                        cx,
                        |f, v| f.mode = v,
                    ),
                ))
                .child(Self::hint("A host's own session-log setting (Edit host → Terminal) overrides these.", cx)),
            Section::Passwords => self.render_passwords(body, cx),
            Section::Sync => self.render_sync(body, cx),
            Section::Diagnostics => {
                let dir = self.backend.paths.logs.clone();
                let mut levels = div().flex().gap(px(4.0));
                for (i, l) in LEVELS.iter().enumerate() {
                    levels = levels.child(
                        Button::new(("lvl", i))
                            .xsmall()
                            .label(*l)
                            .when(self.level == i, |b| b.primary())
                            .on_click(cx.listener(move |f, _, _, cx| {
                                f.level = i;
                                cx.notify();
                            })),
                    );
                }
                body.child(Self::heading("Diagnostics"))
                    .child(Self::row("App log level", levels))
                    .child(
                        div()
                            .pl(px(158.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(Button::new("open-logs").xsmall().label("Open log folder").on_click(
                                move |_, _, cx| {
                                    let _ = std::fs::create_dir_all(&dir);
                                    cx.open_with_system(&dir);
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(muted)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.backend.paths.logs.display().to_string()),
                            ),
                    )
                    .child(Self::hint(format!("Data folder: {}", self.backend.paths.data_dir.display()), cx))
            }
        }
    }

    fn render_passwords(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let vault = &self.backend.vault;
        let (exists, unlocked) = (vault.exists(), vault.is_unlocked());
        let status = match (exists, unlocked) {
            (false, _) => {
                "No vault yet. Tick “Remember” when Hostwick asks for a password, or create one now."
            }
            (true, false) => "Locked. Saved passwords are used after you unlock them.",
            (true, true) => "Unlocked for this session.",
        };
        let mut actions = div().flex().gap(px(6.0));
        if !unlocked {
            actions = actions.child(
                Button::new("vault-unlock")
                    .small()
                    .primary()
                    .label(if exists {
                        "Unlock…"
                    } else {
                        "Create vault…"
                    })
                    .on_click(cx.listener(|f, _, w, cx| f.unlock_vault(w, cx))),
            );
        } else {
            actions = actions
                .child(
                    Button::new("vault-lock")
                        .small()
                        .label("Lock now")
                        .on_click(cx.listener(|f, _, w, cx| {
                            f.backend.vault.lock();
                            f.load_creds(w, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("vault-change")
                        .small()
                        .label("Change master password…")
                        .on_click(cx.listener(|f, _, w, cx| {
                            crate::vaultui::open_change(f.backend.clone(), w, cx, |_, _, _| {});
                        })),
                );
        }
        let list: AnyElement = match (&self.creds, unlocked) {
            (_, false) => div().into_any_element(),
            (None, true) => div()
                .text_size(px(12.0))
                .text_color(theme.muted_foreground)
                .child("Loading…")
                .into_any_element(),
            (Some(rows), true) if rows.is_empty() => div()
                .text_size(px(12.0))
                .text_color(theme.muted_foreground)
                .child("Nothing saved yet.")
                .into_any_element(),
            (Some(rows), true) => div()
                .id("creds")
                .flex()
                .flex_col()
                .border_1()
                .border_color(theme.border)
                .rounded(px(4.0))
                .children(rows.iter().enumerate().map(|(i, (id, desc))| {
                    let id = id.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(3.0))
                        .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(desc.clone()),
                        )
                        .child(
                            Button::new(("forget", i))
                                .xsmall()
                                .ghost()
                                .label("Remove")
                                .on_click(
                                    cx.listener(move |f, _, w, cx| f.forget(id.clone(), w, cx)),
                                ),
                        )
                }))
                .into_any_element(),
        };
        body.child(Self::heading("Saved passwords"))
            .child(
                div()
                    .text_size(px(13.0))
                    .whitespace_normal()
                    .child(status),
            )
            .child(actions)
            .child(Self::row("Lock after (min)", Input::new(&self.auto_lock).small()))
            .child(Self::hint("Idle minutes before saved passwords are locked again; 0 keeps them unlocked until Hostwick quits.", cx))
            .child(list)
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(format!(
                        "Encrypted with Argon2id + XChaCha20-Poly1305 in {} — the same vault as `hostwick vault`. Never synced.",
                        vault.path().display()
                    )),
            )
    }

    fn render_sync(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let last = self
            .backend
            .store
            .setting(crate::app::sync_last_key())
            .ok()
            .flatten()
            .unwrap_or_else(|| "Never synced on this computer.".into());
        body.child(Self::heading("Settings sync"))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child("Hosts and settings go into one encrypted file in a folder your cloud drive or git keeps in step. Saved passwords never leave this computer."),
            )
            .child(Self::row(
                "Folder",
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.sync_folder).small()))
                    .child(Self::browse("browse-sync", &self.sync_folder, true, "Sync folder")),
            ))
            .child(Self::row("Passphrase", Input::new(&self.sync_pass).small().mask_toggle()))
            .child(Self::hint("Kept in your password vault. Every computer needs the same one.", cx))
            .child(Self::check("sync-auto", "Sync at startup, after changes and every 5 minutes", self.sync_auto, cx, |f, v| f.sync_auto = v))
            .child(
                div()
                    .pl(px(158.0))
                    .flex()
                    .gap(px(6.0))
                    .child(Button::new("sync-now").small().primary().label("Sync now").on_click(
                        cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Sync, w, cx)),
                    ))
                    .child(
                        Button::new("sync-push")
                            .small()
                            .label("Replace sync file")
                            .tooltip("Make the sync file match this computer (also changes its passphrase)")
                            .on_click(cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Push, w, cx))),
                    )
                    .child(
                        Button::new("sync-pull")
                            .small()
                            .label("Replace this computer's")
                            .tooltip("Make this computer's hosts and settings match the sync file")
                            .on_click(cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Pull, w, cx))),
                    ),
            )
            .child(Self::row(
                "Last sync",
                div()
                    .text_size(px(12.0))
                    .whitespace_normal()
                    .text_color(theme.muted_foreground)
                    .child(last),
            ))
    }
}

impl Render for SettingsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let current = self.section;
        let nav = div()
            .w(px(170.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .pr(px(10.0))
            .border_r_1()
            .border_color(theme.border)
            .children(Section::ALL.iter().enumerate().map(|(i, s)| {
                let s = *s;
                div()
                    .id(("sec", i))
                    .px(px(10.0))
                    .py(px(5.0))
                    .rounded(px(4.0))
                    .text_size(px(13.0))
                    .cursor_pointer()
                    .when(s == current, |d| {
                        d.bg(theme.accent).text_color(theme.accent_foreground)
                    })
                    .when(s != current, |d| d.hover(|h| h.bg(theme.muted)))
                    .child(s.label())
                    .on_click(cx.listener(move |f, _, w, cx| {
                        f.section = s;
                        if s == Section::Passwords {
                            f.load_creds(w, cx);
                        }
                        cx.notify();
                    }))
            }));
        let content = self.render_section(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div().flex().h(px(500.0)).child(nav).child(
                    div()
                        .id("settings-content")
                        .flex_1()
                        .min_w_0()
                        .pl(px(14.0))
                        .pr(px(6.0))
                        .overflow_y_scroll()
                        .child(content),
                ),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }
}

pub fn open(
    backend: Arc<Backend>,
    profiles: Vec<HostProfile>,
    section: Option<Section>,
    window: &mut Window,
    cx: &mut App,
    on_event: impl Fn(SettingsEvent, &mut Window, &mut App) + 'static,
) {
    let on_event: OnEvent = Rc::new(on_event);
    let form = cx.new(|cx| {
        SettingsForm::new(
            backend,
            profiles,
            section.unwrap_or(Section::Appearance),
            on_event,
            window,
            cx,
        )
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let f1 = form.clone();
        dialog
            .title("Settings")
            .w(px(900.0))
            .margin_top(px(40.0))
            .child(form.clone())
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save").primary().label("Save").on_click(
                            move |_, window, cx| f1.update(cx, |f, cx| f.save(window, cx)),
                        ),
                    ),
            )
    });
}
