//! First run: a welcome screen in the main area that finds the sessions
//! other SSH tools saved on this computer (OpenSSH config, PuTTY, WinSCP,
//! a Solar-PuTTY export) and the keys ready to use, and imports what the
//! person ticks in one go. Later: Import… → Scan this computer.

use std::path::PathBuf;

use crate::zoom::px;
use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::HostwickApp;
use crate::firstrun::{Scan, ScanEnv, SourceKind};

pub(crate) const ONBOARDED_KEY: &str = "desktop.onboarded";

pub(crate) struct WelcomeView {
    app: WeakEntity<HostwickApp>,
    /// Shown in a dialog ("Scan this computer") rather than the main area.
    in_dialog: bool,
    scan: Option<Scan>,
    /// Keys the agent offers (None while asking; Err when there is none).
    agent: Option<Result<Vec<String>, String>>,
    /// Ticked sources (same order as `scan.sources`).
    checked: Vec<bool>,
    use_keys: bool,
}

impl WelcomeView {
    pub(crate) fn new(
        app: WeakEntity<HostwickApp>,
        in_dialog: bool,
        rt: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Self {
        // Files (and the registry) on a background thread, the agent on the
        // network runtime; either may come back first.
        let files = cx
            .background_executor()
            .spawn(async move { crate::firstrun::scan(&ScanEnv::from_system()) });
        cx.spawn(async move |this, cx| {
            let scan = files.await;
            let _ = this.update(cx, |v, cx| {
                v.checked = vec![true; scan.sources.len()];
                v.use_keys = !scan.key_files.is_empty();
                v.scan = Some(scan);
                cx.notify();
            });
        })
        .detach();
        let agent = rt.spawn(async {
            hostwick_ssh::agent_key_names()
                .await
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let res = agent.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |v, cx| {
                v.agent = Some(res);
                cx.notify();
            });
        })
        .detach();
        Self {
            app,
            in_dialog,
            scan: None,
            agent: None,
            checked: Vec::new(),
            use_keys: false,
        }
    }

    fn source_label(kind: SourceKind, path: Option<&PathBuf>, count: usize) -> SharedString {
        // `~/…` reads better than the full home path.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let path = path
            .map(
                |p| match home.as_ref().and_then(|h| p.strip_prefix(h).ok()) {
                    Some(rest) => format!("~/{}", rest.display()),
                    None => p.display().to_string(),
                },
            )
            .unwrap_or_default();
        match kind {
            SourceKind::SshConfig => t!("welcome-src-ssh", count = count, path = path),
            SourceKind::PuttyRegistry => t!("welcome-src-putty", count = count),
            SourceKind::WinScpRegistry => t!("welcome-src-winscp-registry", count = count),
            SourceKind::WinScpIni => t!("welcome-src-winscp-ini", count = count, path = path),
            SourceKind::SolarPutty => t!("welcome-src-solar", count = count, path = path),
        }
    }

    fn anything_ticked(&self) -> bool {
        self.checked.iter().any(|c| *c)
            || (self.use_keys && self.scan.as_ref().is_some_and(|s| !s.key_files.is_empty()))
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scan) = self.scan.take() else {
            return;
        };
        let mut merged = hostwick_import::ImportResult::default();
        for (s, on) in scan.sources.into_iter().zip(self.checked.iter()) {
            if *on {
                merged.profiles.extend(s.result.profiles);
                merged.warnings.extend(s.result.warnings);
            }
        }
        let keys = if self.use_keys {
            scan.key_files
        } else {
            Vec::new()
        };
        let in_dialog = self.in_dialog;
        if in_dialog {
            window.close_dialog(cx);
        }
        let _ = self.app.update(cx, |a, cx| {
            a.finish_welcome(cx);
            if !keys.is_empty() {
                let list: Vec<String> = keys.iter().map(|k| k.display().to_string()).collect();
                crate::backend::set_default_keys(keys);
                a.update_settings(cx, |s| s.default_keys = list);
            }
            if !merged.profiles.is_empty() || !merged.warnings.is_empty() {
                a.apply_import(Ok(merged), window, cx);
            }
            a.load_ssh_aliases(cx);
        });
    }

    fn skip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.in_dialog {
            window.close_dialog(cx);
        }
        let _ = self.app.update(cx, |a, cx| a.finish_welcome(cx));
    }
}

impl Render for WelcomeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut list = div()
            .id("welcome-sources")
            .role(Role::Group)
            .aria_label(t!("welcome-found"))
            .flex()
            .flex_col()
            .gap(px(8.0));
        let scanning = self.scan.is_none();
        let mut nothing = false;
        if let Some(scan) = &self.scan {
            for (i, s) in scan.sources.iter().enumerate() {
                let label = Self::source_label(s.kind, s.path.as_ref(), s.result.profiles.len());
                let on = self.checked.get(i).copied().unwrap_or(false);
                list = list.child(
                    crate::a11y::checkbox(("welcome-src", i), label, false, cx)
                        .checked(on)
                        .on_click(cx.listener(move |v, c: &bool, _, cx| {
                            if let Some(slot) = v.checked.get_mut(i) {
                                *slot = *c;
                            }
                            cx.notify();
                        })),
                );
            }
            let agent_count = match &self.agent {
                Some(Ok(k)) => k.len(),
                _ => 0,
            };
            if !scan.key_files.is_empty() {
                let names = scan
                    .key_files
                    .iter()
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .collect::<Vec<_>>()
                    .join(", ");
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            crate::a11y::checkbox(
                                "welcome-keys",
                                t!("welcome-keys", count = scan.key_files.len()),
                                false,
                                cx,
                            )
                            .checked(self.use_keys)
                            .on_click(cx.listener(
                                |v, c: &bool, _, cx| {
                                    v.use_keys = *c;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            div()
                                .pl(px(24.0))
                                .text_size(px(12.0))
                                .text_color(theme.muted_foreground)
                                .whitespace_normal()
                                .child(crate::a11y::text(names)),
                        ),
                );
            }
            let agent_line = match &self.agent {
                None => t!("welcome-agent-asking"),
                Some(Ok(_)) if agent_count > 0 => t!("welcome-agent", count = agent_count),
                Some(_) => t!("welcome-agent-none"),
            };
            list = list.child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(crate::a11y::text(agent_line)),
            );
            nothing = scan.sources.is_empty() && scan.key_files.is_empty();
        }
        let can_import = !scanning && self.anything_ticked();
        div()
            .id("welcome")
            .role(Role::Group)
            .aria_label(t!("welcome-title"))
            .when(!self.in_dialog, |d| {
                d.size_full().flex().items_center().justify_center()
            })
            .child(
                div()
                    .w(px(if self.in_dialog { 560.0 } else { 600.0 }))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .when(!self.in_dialog, |d| {
                        d.p(px(28.0))
                            .rounded(px(10.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(img(super::logo()).size(px(36.0)))
                                    .child(
                                        div()
                                            .text_size(px(20.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(crate::a11y::text(t!("welcome-title"))),
                                    ),
                            )
                    })
                    .child(
                        div()
                            .text_size(px(14.0))
                            .whitespace_normal()
                            .child(crate::a11y::text(t!("welcome-intro"))),
                    )
                    .when(scanning, |d| {
                        d.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(crate::a11y::text(t!("welcome-scanning"))),
                        )
                    })
                    .when(!scanning && nothing, |d| {
                        d.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .whitespace_normal()
                                .child(crate::a11y::text(t!("welcome-nothing"))),
                        )
                    })
                    .when(!scanning, |d| d.child(list))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(crate::a11y::text(t!("welcome-note"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(
                                Button::new("welcome-import")
                                    .primary()
                                    .label(t!("welcome-import"))
                                    .disabled(!can_import)
                                    .on_click(cx.listener(|v, _, w, cx| v.import(w, cx))),
                            )
                            .child(
                                Button::new("welcome-skip")
                                    .label(if nothing {
                                        t!("welcome-continue")
                                    } else {
                                        t!("welcome-skip")
                                    })
                                    .on_click(cx.listener(|v, _, w, cx| v.skip(w, cx))),
                            )
                            .when(!self.in_dialog, |d| {
                                d.child(div().flex_1()).child(
                                    Button::new("welcome-new-host")
                                        .ghost()
                                        .label(t!("sidebar-new-host"))
                                        .on_click(cx.listener(|v, _, w, cx| {
                                            let _ = v
                                                .app
                                                .update(cx, |a, cx| a.host_dialog(None, w, cx));
                                        })),
                                )
                            }),
                    ),
            )
    }
}

impl HostwickApp {
    /// First launch: no hosts and never onboarded → the welcome screen.
    pub(super) fn maybe_welcome(&mut self, cx: &mut Context<Self>) {
        let onboarded = self
            .backend
            .store
            .setting(ONBOARDED_KEY)
            .ok()
            .flatten()
            .is_some();
        if onboarded || !self.profiles.is_empty() {
            return;
        }
        let app = cx.entity().downgrade();
        let rt = self.backend.handle();
        self.welcome = Some(cx.new(|cx| WelcomeView::new(app, false, rt, cx)));
    }

    /// The welcome screen is done (imported or skipped).
    pub(crate) fn finish_welcome(&mut self, cx: &mut Context<Self>) {
        self.welcome = None;
        if let Err(e) = self.backend.store.set_setting(ONBOARDED_KEY, "1") {
            tracing::warn!(error = %e, "could not save the onboarding flag");
        }
        cx.notify();
    }

    /// Import… → Scan this computer: the same screen, in a dialog.
    pub(crate) fn scan_computer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let rt = self.backend.handle();
        let view = cx.new(|cx| WelcomeView::new(app, true, rt, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(crate::a11y::text(t!("import-menu-scan")))
                .w(px(620.0))
                .margin_top(px(60.0))
                .child(view.clone())
        });
    }
}
