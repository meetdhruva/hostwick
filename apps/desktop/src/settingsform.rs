//! Preferences dialog, in sections. Saving persists to the database (and
//! records the shareable part for settings sync), updates the global, and
//! tells the app so open terminals pick up the change.

use std::rc::Rc;
use std::sync::Arc;

use crate::zoom::px;
use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
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
    pub(crate) const ALL: [Section; 9] = [
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

    pub(crate) fn label(self) -> SharedString {
        match self {
            Section::Appearance => t!("settings-section-appearance"),
            Section::Terminal => t!("settings-section-terminal"),
            Section::Snippets => t!("settings-section-snippets"),
            Section::Connections => t!("settings-section-connections"),
            Section::Files => t!("settings-section-files"),
            Section::Logging => t!("settings-section-logging"),
            Section::Passwords => t!("settings-section-passwords"),
            Section::Sync => t!("settings-section-sync"),
            Section::Diagnostics => t!("settings-section-diagnostics"),
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
    ui_scale: u32,
    scroll: Entity<InputState>,
    logdir: Entity<InputState>,
    editor: Entity<InputState>,
    auto_lock: Entity<InputState>,
    sync_folder: Entity<InputState>,
    sync_pass: Entity<InputState>,
    theme: ThemeChoice,
    /// `Settings.language`: "" follows the OS.
    language: String,
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
        let mk = |v: String, ph: SharedString, window: &mut Window, cx: &mut Context<Self>| {
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
            font: mk(
                s.font_family.clone(),
                t!("settings-font-placeholder"),
                window,
                cx,
            ),
            size: mk(s.font_size.to_string(), "14".into(), window, cx),
            ui_scale: s.ui_scale,
            scroll: mk(s.scrollback_lines.to_string(), "10000".into(), window, cx),
            logdir: mk(s.session_log_dir.clone(), default_dir.into(), window, cx),
            editor: mk(
                s.external_editor.clone(),
                t!("settings-editor-placeholder"),
                window,
                cx,
            ),
            auto_lock: mk(
                s.vault_auto_lock_minutes.to_string(),
                t!("settings-auto-lock-placeholder"),
                window,
                cx,
            ),
            sync_folder: mk(folder, t!("settings-sync-folder-placeholder"), window, cx),
            sync_pass: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(if has_pass {
                        t!("settings-sync-pass-saved")
                    } else {
                        t!("settings-sync-pass-placeholder")
                    })
            }),
            theme: s.theme,
            language: s.language.clone(),
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
                SnippetEditor::new(&s.snippets, t!("settings-snippets-empty"), window, cx)
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
                    Ok(Err(e)) => f.error = Some(t!("settings-remove-failed", error = e).into()),
                    Err(e) => f.error = Some(t!("error-internal", error = e.to_string()).into()),
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
            .ok_or_else(|| t!("settings-error-font-size").to_string())?;
        let scrollback = v(&self.scroll)
            .parse::<usize>()
            .ok()
            .filter(|s| (100..=1_000_000).contains(s))
            .ok_or_else(|| t!("settings-error-scrollback").to_string())?;
        let auto_lock = v(&self.auto_lock)
            .parse::<u32>()
            .ok()
            .filter(|m| *m <= 24 * 60)
            .ok_or_else(|| t!("settings-error-auto-lock").to_string())?;
        if let Some(p) = self.rules.read(cx).problem(cx) {
            return Err(t!("settings-error-rules", error = p).into());
        }
        let folder = v(&self.sync_folder);
        if !folder.is_empty() && !std::path::Path::new(&folder).is_dir() {
            return Err(t!("settings-error-sync-folder", folder = folder).into());
        }
        Ok(Settings {
            font_family: v(&self.font),
            font_size,
            ui_scale: self.ui_scale,
            theme: self.theme,
            language: self.language.clone(),
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
            self.error = Some(t!("settings-folder-missing", folder = folder).into());
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
            self.error = Some(t!("settings-sync-pass-short").into());
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
                        s.set_placeholder(t!("settings-sync-pass-saved"), window, cx);
                    });
                    then(f, window, cx);
                }
                Err(e) => f.error = Some(t!("settings-sync-pass-save-failed", error = e).into()),
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
            t!("vault-reason-sync-passphrase").into(),
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
        let mut new = match self.collect(cx) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        // Changed elsewhere while this dialog was open (sidebar folders,
        // the welcome screen's keys): keep the current ones.
        let current = Settings::get(cx);
        new.folders = current.folders.clone();
        new.default_keys = current.default_keys.clone();
        if let Err(e) = new.save(&self.backend.store) {
            self.error = Some(t!("settings-save-failed", error = e).into());
            cx.notify();
            return;
        }
        if new.log_level != self.base.log_level {
            crate::logging::set_level(&new.log_level);
        }
        if new.theme != self.base.theme {
            apply_theme(new.theme, cx);
        }
        if new.ui_scale != self.base.ui_scale {
            crate::zoom::set_ui_scale(new.ui_scale);
        }
        if new.language != self.base.language {
            crate::i18n::set_language(&new.language);
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

    fn row(label: impl Into<SharedString>, el: impl IntoElement) -> Div {
        let label: SharedString = label.into();
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .text_size(px(13.0))
                    .child(label.clone()),
            )
            // Named after the label, so a screen reader says what a field,
            // or a row of choices, is for.
            .child(
                div()
                    .id(ElementId::Name(label.clone()))
                    .role(Role::Group)
                    .aria_label(label)
                    .flex_1()
                    .min_w_0()
                    .child(el),
            )
    }

    fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
        div()
            .pl(px(158.0))
            .text_size(px(11.0))
            .whitespace_normal()
            .text_color(cx.theme().muted_foreground)
            .child(crate::a11y::text(text))
    }

    fn heading(text: SharedString) -> Div {
        div()
            .pt(px(6.0))
            .text_size(px(15.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
    }

    fn check(
        id: &'static str,
        label: SharedString,
        value: bool,
        cx: &mut Context<Self>,
        set: fn(&mut Self, bool),
    ) -> Div {
        div().pl(px(158.0)).child(
            crate::a11y::checkbox(id, label, false, cx)
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
        options: &[(T, SharedString)],
        cx: &mut Context<Self>,
        set: fn(&mut Self, T),
    ) -> Div {
        let mut row = div().flex().gap(px(4.0)).flex_wrap();
        for (i, (value, label)) in options.iter().cloned().enumerate() {
            row = row.child(
                Button::new((id, i))
                    .xsmall()
                    .label(label)
                    .toggled(current == value)
                    .when(current == value, |b| b.primary())
                    .on_click(cx.listener(move |f, _, _, cx| {
                        set(f, value);
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// Language picker: "System (…)" and every language in its own script.
    fn language_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let system_code = crate::i18n::resolve("");
        let name_of = |code: &str| {
            crate::i18n::LANGUAGES
                .iter()
                .find(|l| l.code == code)
                .map_or("English", |l| l.native_name)
        };
        let system = t!("settings-language-system", language = name_of(system_code));
        let current: SharedString = if self.language.is_empty() {
            system.clone()
        } else {
            name_of(&self.language).into()
        };
        let this = cx.entity().downgrade();
        let selected = self.language.clone();
        Button::new("language")
            .small()
            .label(current)
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                let pick = |code: &'static str| {
                    let this = this.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = this.update(cx, |f, cx| {
                            f.language = code.to_owned();
                            cx.notify();
                        });
                    }
                };
                let mut menu = menu.item(
                    PopupMenuItem::new(system.clone())
                        .checked(selected.is_empty())
                        .on_click(pick("")),
                );
                menu = menu.separator();
                for l in crate::i18n::LANGUAGES {
                    menu = menu.item(
                        PopupMenuItem::new(l.native_name)
                            .checked(selected == l.code)
                            .on_click(pick(l.code)),
                    );
                }
                menu
            })
    }

    fn browse(
        id: &'static str,
        input: &Entity<InputState>,
        directories: bool,
        prompt: SharedString,
    ) -> Button {
        let input = input.clone();
        Button::new(id)
            .small()
            .label(t!("button-browse"))
            .on_click(move |_, window, cx| {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: !directories,
                    directories,
                    multiple: false,
                    prompt: Some(prompt.clone()),
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
                .child(Self::heading(t!("settings-section-appearance")))
                .child(Self::row(t!("settings-language"), self.language_picker(cx)))
                .child(Self::hint(t!("settings-language-hint"), cx))
                .child(Self::row(
                    t!("settings-theme"),
                    Self::choice(
                        "theme",
                        self.theme,
                        &[
                            (ThemeChoice::Dark, t!("settings-theme-dark")),
                            (ThemeChoice::Light, t!("settings-theme-light")),
                        ],
                        cx,
                        |f, v| f.theme = v,
                    ),
                ))
                .child(Self::row(
                    t!("settings-ui-scale"),
                    Self::choice(
                        "ui-scale",
                        self.ui_scale,
                        &(crate::zoom::MIN_UI_SCALE..=crate::zoom::MAX_UI_SCALE)
                            .step_by(crate::zoom::UI_SCALE_STEP as usize)
                            .map(|p| (p, SharedString::from(format!("{p}%"))))
                            .collect::<Vec<_>>(),
                        cx,
                        |f, v| f.ui_scale = v,
                    ),
                ))
                .child(Self::hint(t!("settings-ui-scale-hint"), cx))
                .child(Self::row(
                    t!("settings-terminal-font"),
                    Input::new(&self.font)
                        .small()
                        .aria_label(t!("settings-terminal-font")),
                ))
                .child(Self::row(
                    t!("settings-font-size"),
                    Input::new(&self.size)
                        .small()
                        .aria_label(t!("settings-font-size")),
                ))
                .child(Self::hint(t!("settings-font-hint"), cx)),
            Section::Terminal => body
                .child(Self::heading(t!("settings-section-terminal")))
                .child(Self::row(
                    t!("settings-scrollback"),
                    Input::new(&self.scroll)
                        .small()
                        .aria_label(t!("settings-scrollback")),
                ))
                .child(Self::row(
                    t!("settings-right-click"),
                    Self::choice(
                        "rc",
                        self.right,
                        &[
                            (RightClick::Menu, t!("settings-right-click-menu")),
                            (RightClick::Paste, t!("settings-right-click-paste")),
                        ],
                        cx,
                        |f, v| f.right = v,
                    ),
                ))
                .child(Self::check(
                    "copy-sel",
                    t!("settings-copy-on-select"),
                    self.copy_sel,
                    cx,
                    |f, v| f.copy_sel = v,
                ))
                .child(Self::check(
                    "mouse",
                    t!("settings-mouse"),
                    self.mouse,
                    cx,
                    |f, v| f.mouse = v,
                ))
                .child(Self::hint(t!("settings-mouse-hint"), cx))
                .child(Self::check(
                    "bar",
                    t!("settings-button-bar"),
                    self.button_bar,
                    cx,
                    |f, v| f.button_bar = v,
                ))
                .child(
                    div()
                        .pt(px(6.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(13.0))
                        .child(t!("settings-highlighting")),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(muted)
                        .child(t!("settings-highlighting-hint")),
                )
                .child(self.rules.clone()),
            Section::Snippets => body
                .child(Self::heading(t!("settings-section-snippets")))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(muted)
                        .whitespace_normal()
                        .child(t!("settings-snippets-hint")),
                )
                .child(self.snippets.clone()),
            Section::Connections => body
                .child(Self::heading(t!("settings-section-connections")))
                .child(Self::check(
                    "auto-rc",
                    t!("settings-auto-reconnect"),
                    self.auto_reconnect,
                    cx,
                    |f, v| f.auto_reconnect = v,
                ))
                .child(Self::hint(t!("settings-auto-reconnect-hint"), cx))
                .child(Self::check(
                    "restore",
                    t!("settings-restore-tabs"),
                    self.restore_tabs,
                    cx,
                    |f, v| f.restore_tabs = v,
                ))
                .child(Self::hint(t!("settings-restore-tabs-hint"), cx)),
            Section::Files => body
                .child(Self::heading(t!("settings-section-files")))
                .child(Self::row(
                    t!("settings-editor"),
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            div().flex_1().child(
                                Input::new(&self.editor)
                                    .small()
                                    .aria_label(t!("settings-editor")),
                            ),
                        )
                        .child(Self::browse(
                            "browse-editor",
                            &self.editor,
                            false,
                            t!("settings-editor-pick"),
                        )),
                ))
                .child(Self::hint(t!("settings-editor-hint"), cx)),
            Section::Logging => body
                .child(Self::heading(t!("settings-section-logging")))
                .child(Self::check(
                    "log-all",
                    t!("settings-log-all"),
                    self.log_all,
                    cx,
                    |f, v| f.log_all = v,
                ))
                .child(Self::row(
                    t!("settings-log-folder"),
                    Input::new(&self.logdir)
                        .small()
                        .aria_label(t!("settings-log-folder")),
                ))
                .child(Self::row(
                    t!("settings-log-content"),
                    Self::choice(
                        "lm",
                        self.mode,
                        &[
                            (LogMode::Printable, t!("settings-log-printable")),
                            (LogMode::Raw, t!("settings-log-raw")),
                        ],
                        cx,
                        |f, v| f.mode = v,
                    ),
                ))
                .child(Self::hint(t!("settings-log-hint"), cx)),
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
                body.child(Self::heading(t!("settings-section-diagnostics")))
                    .child(Self::row(t!("settings-log-level"), levels))
                    .child(
                        div()
                            .pl(px(158.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                Button::new("open-logs")
                                    .xsmall()
                                    .label(t!("settings-open-log-folder"))
                                    .on_click(move |_, _, cx| {
                                        let _ = std::fs::create_dir_all(&dir);
                                        cx.open_with_system(&dir);
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(muted)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.backend.paths.logs.display().to_string()),
                            ),
                    )
                    .child(Self::hint(
                        t!(
                            "settings-data-folder",
                            path = self.backend.paths.data_dir.display().to_string()
                        ),
                        cx,
                    ))
            }
        }
    }

    fn render_passwords(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let vault = &self.backend.vault;
        let (exists, unlocked) = (vault.exists(), vault.is_unlocked());
        let status = match (exists, unlocked) {
            (false, _) => t!("settings-vault-none"),
            (true, false) => t!("settings-vault-locked"),
            (true, true) => t!("settings-vault-unlocked"),
        };
        let mut actions = div().flex().gap(px(6.0));
        if !unlocked {
            actions = actions.child(
                Button::new("vault-unlock")
                    .small()
                    .primary()
                    .label(if exists {
                        t!("settings-vault-unlock")
                    } else {
                        t!("settings-vault-create")
                    })
                    .on_click(cx.listener(|f, _, w, cx| f.unlock_vault(w, cx))),
            );
        } else {
            actions = actions
                .child(
                    Button::new("vault-lock")
                        .small()
                        .label(t!("settings-vault-lock"))
                        .on_click(cx.listener(|f, _, w, cx| {
                            f.backend.vault.lock();
                            f.load_creds(w, cx);
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("vault-change")
                        .small()
                        .label(t!("settings-vault-change"))
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
                .child(t!("loading"))
                .into_any_element(),
            (Some(rows), true) if rows.is_empty() => div()
                .text_size(px(12.0))
                .text_color(theme.muted_foreground)
                .child(t!("settings-vault-empty"))
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
                                .label(t!("settings-vault-remove"))
                                .on_click(
                                    cx.listener(move |f, _, w, cx| f.forget(id.clone(), w, cx)),
                                ),
                        )
                }))
                .into_any_element(),
        };
        body.child(Self::heading(t!("settings-section-passwords")))
            .child(
                div()
                    .text_size(px(13.0))
                    .whitespace_normal()
                    .child(crate::a11y::text(status)),
            )
            .child(actions)
            .child(Self::row(
                t!("settings-auto-lock"),
                Input::new(&self.auto_lock)
                    .small()
                    .aria_label(t!("settings-auto-lock")),
            ))
            .child(Self::hint(t!("settings-auto-lock-hint"), cx))
            .child(list)
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(t!(
                        "settings-vault-file",
                        path = vault.path().display().to_string()
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
            .unwrap_or_else(|| t!("settings-sync-never").into());
        body.child(Self::heading(t!("settings-section-sync")))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(t!("settings-sync-intro")),
            )
            .child(Self::row(
                t!("settings-sync-folder"),
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.sync_folder)
                                .small()
                                .aria_label(t!("settings-sync-folder")),
                        ),
                    )
                    .child(Self::browse(
                        "browse-sync",
                        &self.sync_folder,
                        true,
                        t!("settings-sync-folder-pick"),
                    )),
            ))
            .child(Self::row(
                t!("settings-sync-passphrase"),
                Input::new(&self.sync_pass)
                    .small()
                    .aria_label(t!("settings-sync-passphrase"))
                    .mask_toggle(),
            ))
            .child(Self::hint(t!("settings-sync-passphrase-hint"), cx))
            .child(Self::check(
                "sync-auto",
                t!("settings-sync-auto"),
                self.sync_auto,
                cx,
                |f, v| f.sync_auto = v,
            ))
            .child(
                div()
                    .pl(px(158.0))
                    .flex()
                    .gap(px(6.0))
                    .child(
                        Button::new("sync-now")
                            .small()
                            .primary()
                            .label(t!("settings-sync-now"))
                            .on_click(cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Sync, w, cx))),
                    )
                    .child(
                        Button::new("sync-push")
                            .small()
                            .label(t!("settings-sync-push"))
                            .tooltip(t!("settings-sync-push-tooltip"))
                            .on_click(cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Push, w, cx))),
                    )
                    .child(
                        Button::new("sync-pull")
                            .small()
                            .label(t!("settings-sync-pull"))
                            .tooltip(t!("settings-sync-pull-tooltip"))
                            .on_click(cx.listener(|f, _, w, cx| f.sync_now(SyncMode::Pull, w, cx))),
                    ),
            )
            .child(Self::row(
                t!("settings-sync-last"),
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
        // Sections: buttons, so Tab reaches them and Enter/Space opens one.
        let nav = div()
            .id("settings-sections")
            .role(Role::Toolbar)
            .aria_label(t!("a11y-sections"))
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
                crate::a11y::section_button(("sec", i), s.label(), s == current, cx).on_click(
                    cx.listener(move |f, _, w, cx| {
                        f.section = s;
                        if s == Section::Passwords {
                            f.load_creds(w, cx);
                        }
                        cx.notify();
                    }),
                )
            }));
        let content = self.render_section(cx);
        div()
            .id("settings-form")
            .role(Role::Group)
            .aria_label(t!("settings-title"))
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
                        .id("settings-error")
                        .role(Role::Alert)
                        .aria_label(e.clone())
                        .aria_value(e.clone())
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
        let (f1, f2) = (form.clone(), form.clone());
        dialog
            .title(crate::a11y::text(t!("settings-title")))
            .w(px(900.0))
            .margin_top(px(40.0))
            .child(form.clone())
            // Enter (outside multi-line fields) saves.
            .on_ok(move |_, window, cx| {
                f2.update(cx, |f, cx| f.save(window, cx));
                false
            })
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label(t!("button-cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save")
                            .primary()
                            .label(t!("button-save"))
                            .on_click(move |_, window, cx| {
                                f1.update(cx, |f, cx| f.save(window, cx))
                            }),
                    ),
            )
    });
}
