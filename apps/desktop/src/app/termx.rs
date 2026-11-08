//! What terminals ask the app for (v0.4): bell markers and "command
//! finished" alerts, paste confirmation, dropping files to upload, opening
//! paths and hosts found in the output, "Enable now" for shell integration.
//! The terminal view sends [`TermExt`] events; the app owns tabs, hosts,
//! file browsers and the transfer queue, so it does the rest.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{Environment, HostProfile};
use hostwick_sftp::{LocalFs, RemoteFs, path_to_slash};
use hostwick_term::TransportKind;
use hostwick_transfer::{ExistingPolicy, Job};

use super::{HostwickApp, PaneBody, Tab, TabKind, upload};
use crate::notify::{Toast, ToastExt as _};
use crate::settings::Settings;
use crate::terminal::{TermExt, TerminalView, paste};

/// Per-app state of this feature.
#[derive(Default)]
pub(crate) struct State {
    /// Tabs with a bell or a finished command, not looked at yet.
    bells: HashSet<u64>,
}

/// Marker types for notification ids.
struct FinishedNote;

/// What to say when a command finishes: the in-app text, which may name the
/// command (short and cleaned), and the operating-system notification, which
/// never does: the OS keeps notifications in its history and shows them on
/// the lock screen, and commands often hold secrets (`mysql -pSECRET`,
/// `export TOKEN=…`).
pub(crate) fn finished_texts(
    host: &str,
    command: Option<&str>,
    exit: Option<i32>,
    took: Duration,
) -> (SharedString, SharedString) {
    let time = crate::terminal::format_duration(took);
    let code = exit.map_or_else(|| "?".to_owned(), |c| c.to_string());
    let shown = command
        .map(|c| crate::terminal::display_command(c, 80))
        .filter(|c| !c.is_empty());
    let text = match shown {
        Some(c) => t!(
            "tx-finished",
            host = host.to_owned(),
            command = c,
            exit = code.clone(),
            time = time.clone()
        ),
        None => t!(
            "tx-finished-nocmd",
            host = host.to_owned(),
            exit = code.clone(),
            time = time.clone()
        ),
    };
    let os = t!(
        "tx-finished-os",
        host = host.to_owned(),
        exit = code,
        time = time
    );
    (text, os)
}

/// The most telling environment among a tab's panes (production wins).
pub(crate) fn tab_env(tab: &Tab) -> Environment {
    fn rank(e: &Environment) -> u8 {
        match e {
            Environment::Prod => 4,
            Environment::Staging => 3,
            Environment::Custom(_) => 2,
            Environment::Dev => 1,
            Environment::None => 0,
        }
    }
    tab.panes()
        .into_iter()
        .map(|p| &p.profile.environment)
        .max_by_key(|e| rank(e))
        .cloned()
        .unwrap_or_default()
}

impl HostwickApp {
    /// A tab shows a bell marker.
    pub(crate) fn tab_has_bell(&self, tab_id: u64, window: &Window) -> bool {
        self.termx.bells.contains(&tab_id)
            && !(self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id) == Some(tab_id)
                && window.is_window_active())
    }

    /// The person looked at a tab: its markers go.
    pub(crate) fn clear_bell(&mut self, tab_id: u64) {
        self.termx.bells.remove(&tab_id);
    }

    /// The window came to the front: the visible tab has been seen.
    pub(crate) fn window_activated(&mut self, window: &Window, cx: &mut Context<Self>) {
        if window.is_window_active()
            && let Some(id) = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id)
            && self.termx.bells.remove(&id)
        {
            cx.notify();
        }
    }

    /// Is the person looking at this pane right now?
    fn pane_in_view(&self, pane_id: u64, window: &Window) -> bool {
        window.is_window_active()
            && self
                .active
                .and_then(|i| self.tabs.get(i))
                .is_some_and(|t| t.root.find(pane_id).is_some())
    }

    pub(super) fn on_term_ext(
        &mut self,
        source: &Entity<TerminalView>,
        pane_id: Option<u64>,
        ev: &TermExt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            TermExt::Bell => {
                if let Some(id) = pane_id
                    && !self.pane_in_view(id, window)
                    && let Some((ix, _)) = self.locate(id)
                {
                    let tab = self.tabs[ix].id;
                    if self.termx.bells.insert(tab) {
                        cx.notify();
                    }
                }
            }
            TermExt::Finished {
                command,
                exit,
                took,
            } => self.command_finished(source, pane_id, command.clone(), *exit, *took, window, cx),
            TermExt::Paste { text } => self.paste_request(source, text.clone(), window, cx),
            TermExt::DropFiles(paths) => {
                self.drop_upload(source, pane_id, paths.clone(), window, cx)
            }
            TermExt::OpenPath { path, download } => {
                self.open_remote_path(source, pane_id, path.clone(), *download, window, cx)
            }
            TermExt::Connect(text) => self.connect_from_terminal(text.clone(), window, cx),
            TermExt::RevealFile(path) => self.confirm_reveal(source, path.clone(), window, cx),
            TermExt::EnableIntegration => self.enable_integration(source, pane_id, window, cx),
        }
    }

    // ---- finished commands -----------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn command_finished(
        &mut self,
        source: &Entity<TerminalView>,
        pane_id: Option<u64>,
        command: Option<String>,
        exit: Option<i32>,
        took: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = pane_id else {
            return;
        };
        // Looking at it: the prompt coming back is notice enough.
        if self.pane_in_view(pane_id, window) {
            return;
        }
        if let Some((ix, _)) = self.locate(pane_id) {
            let tab = self.tabs[ix].id;
            self.termx.bells.insert(tab);
        }
        let host = source.read(cx).host_info().name.clone();
        let (text, os_text) = finished_texts(&host, command.as_deref(), exit, took);
        let me = cx.entity().downgrade();
        let show = move |window: &mut Window, cx: &mut App| {
            let _ = me.update(cx, |app, cx| app.show_pane(pane_id, window, cx));
        };
        let toast = match exit {
            Some(0) => Toast::success(text.clone()),
            Some(_) => Toast::warning(text.clone()),
            None => Toast::info(text.clone()),
        };
        window.toast(toast.action(t!("tx-finished-show"), show.clone()), cx);
        if !window.is_window_active() {
            let note = Notification::info(os_text)
                .title("Hostwick")
                .system()
                .id1::<FinishedNote>(pane_id)
                .on_click(move |_, window, cx| show(window, cx));
            window.push_notification(note, cx);
        }
        cx.notify();
    }

    /// Bring a pane to the front (clicking a notification).
    pub(crate) fn show_pane(&mut self, pane_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((ix, _)) = self.locate(pane_id) {
            self.activate_tab(ix, window, cx);
            self.focus_pane(pane_id, window, cx);
        }
    }

    // ---- paste -------------------------------------------------------------------

    /// The terminals a paste into `source` would reach besides it: with
    /// broadcast on, every other terminal that takes input.
    fn paste_peers(&self, source: &Entity<TerminalView>, cx: &App) -> Vec<paste::Receiver> {
        if !self.broadcast {
            return Vec::new();
        }
        self.all_terminals()
            .into_iter()
            .filter(|v| v.entity_id() != source.entity_id())
            .filter_map(|v| {
                let v = v.read(cx);
                v.accepts_input().then(|| paste::Receiver {
                    name: v.host_info().name.clone(),
                    prod: v.host_info().env.is_prod(),
                })
            })
            .collect()
    }

    /// Text is being pasted into `source`: ask first when the paste rules
    /// say so for it or, with broadcast on, for any terminal that receives
    /// it too.
    fn paste_request(
        &mut self,
        source: &Entity<TerminalView>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = Settings::get(cx).term.confirm_paste;
        let peers = self.paste_peers(source, cx);
        let source_prod = source.read(cx).host_info().env.is_prod();
        let verdict = paste::verdict(&text, mode, source_prod, &peers);
        if verdict.confirm {
            self.confirm_paste(source, text, peers.len(), verdict, window, cx);
        } else {
            source.update(cx, |v, cx| v.paste_confirmed(text, false, cx));
        }
    }

    fn confirm_paste(
        &mut self,
        source: &Entity<TerminalView>,
        text: String,
        peers: usize,
        verdict: paste::Verdict,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let info = source.read(cx).host_info().clone();
        let view = source.clone();
        let pv = paste::preview(&text, 8, 110);
        let prod = verdict.prod;
        let source_prod = info.env.is_prod();
        let prod_peers: SharedString = verdict.prod_peers.join(", ").into();
        let has_control = paste::has_control(&text);
        let title: SharedString = if has_control {
            t!("tx-paste-title-control", host = info.name.clone())
        } else if text.len() > paste::LARGE && pv.total_lines <= 1 {
            t!(
                "tx-paste-title-large",
                size = crate::terminal::format_bytes(text.len() as u64),
                host = info.name.clone()
            )
        } else {
            t!(
                "tx-paste-title",
                lines = pv.total_lines,
                host = info.name.clone()
            )
        };
        let back = source.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
            let (t1, t2, t3) = (text.clone(), text.clone(), text.clone());
            let back = back.clone();
            let pv = pv.clone();
            dialog
                .title(crate::a11y::text(title.clone()))
                .w(px(640.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .text_size(px(13.0))
                        .when(prod, |d| {
                            let banner = if source_prod {
                                t!("tx-paste-prod", host = info.name.clone())
                            } else {
                                t!("tx-paste-prod-peers", hosts = prod_peers.clone())
                            };
                            d.child(
                                div()
                                    .px(px(8.0))
                                    .py(px(4.0))
                                    .rounded(px(4.0))
                                    .bg(hsla(0.0, 0.65, 0.40, 1.0))
                                    .text_color(hsla(0.0, 0.0, 1.0, 1.0))
                                    .child(crate::a11y::text(banner)),
                            )
                        })
                        .when(prod && source_prod && !prod_peers.is_empty(), |d| {
                            d.child(div().text_color(theme.danger).child(crate::a11y::text(t!(
                                "tx-paste-prod-peers",
                                hosts = prod_peers.clone()
                            ))))
                        })
                        .child(crate::a11y::text(t!(
                            "tx-paste-target",
                            address = info.address(),
                            size = crate::terminal::format_bytes(pv.bytes as u64)
                        )))
                        .when(peers > 0, |d| {
                            d.child(
                                div().text_color(theme.warning).child(crate::a11y::text(t!(
                                    "tx-paste-broadcast",
                                    count = peers
                                ))),
                            )
                        })
                        .child(
                            div()
                                .id("paste-preview")
                                .max_h(px(240.0))
                                .overflow_y_scroll()
                                .p(px(8.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.background)
                                .font_family(crate::terminal::monospace_family())
                                .text_size(px(12.0))
                                .children(pv.lines.iter().map(|l| {
                                    div().whitespace_nowrap().child(if l.is_empty() {
                                        " ".to_owned()
                                    } else {
                                        l.clone()
                                    })
                                }))
                                .when(pv.more_lines > 0, |d| {
                                    d.child(
                                        div()
                                            .pt(px(4.0))
                                            .text_color(theme.muted_foreground)
                                            .child(t!("tx-paste-more", count = pv.more_lines)),
                                    )
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    window.close_dialog(cx);
                    let t = t1.clone();
                    v1.update(cx, |v, cx| v.paste_confirmed(t, false, cx));
                    false
                })
                .on_close(move |_, window, cx| back.focus(window, cx))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label(t!("button-cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("single").label(t!("tx-paste-single")).on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let t = t2.clone();
                                v2.update(cx, |v, cx| v.paste_confirmed(t, true, cx));
                            },
                        ))
                        .child(
                            Button::new("paste")
                                .label(t!("tx-paste-go"))
                                .when(prod, |b| b.danger())
                                .when(!prod, |b| b.primary())
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let t = t3.clone();
                                    v3.update(cx, |v, cx| v.paste_confirmed(t, false, cx));
                                }),
                        ),
                )
        });
    }

    // ---- drop to upload ------------------------------------------------------------

    fn drop_upload(
        &mut self,
        source: &Entity<TerminalView>,
        pane_id: Option<u64>,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, pane)) = pane_id.and_then(|id| self.locate(id)) else {
            return;
        };
        let profile = pane.profile.clone();
        let kind = source.read(cx).transport();
        if kind != TransportKind::Ssh {
            window.toast(
                Toast::info(t!("tx-drop-ssh-only", protocol = kind.label())),
                cx,
            );
            return;
        }
        if paths.is_empty() {
            return;
        }
        // Never straight away: the folder may have been named by text that
        // was printed to the terminal, and the files replace what is there.
        let (cwd, trusted) = {
            let v = source.read(cx);
            (v.cwd(), v.cwd_trusted())
        };
        self.confirm_upload(source, profile, paths, cwd, trusted, window, cx);
    }

    /// Queue `paths` for upload into `dir` (folders recursively); the
    /// transfer panel shows the progress. `existing` says what happens to
    /// files already there.
    pub(super) fn start_upload(
        &mut self,
        profile: HostProfile,
        paths: Vec<PathBuf>,
        dir: String,
        existing: upload::Existing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let backend = self.backend.clone();
        let engine = backend.engine.clone();
        let count = paths.len();
        let shown = dir.clone();
        let job = backend.rt.spawn({
            let backend = backend.clone();
            async move {
                let session = backend.open_remote(profile, false).await?;
                let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
                let mut errors: Vec<String> = Vec::new();
                for p in paths {
                    let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                        continue;
                    };
                    let mut target = match hostwick_sftp::join_checked(&dir, &name) {
                        Ok(t) => t,
                        Err(e) => {
                            errors.push(format!("{name}: {e}"));
                            continue;
                        }
                    };
                    if existing == upload::Existing::Rename {
                        // Keep both: the new item gets `name (1).ext`.
                        match upload::free_remote_name(&session.fs, &dir, &name).await {
                            Ok(n) => match hostwick_sftp::join_checked(&dir, &n) {
                                Ok(t) => target = t,
                                Err(e) => {
                                    errors.push(format!("{name}: {e}"));
                                    continue;
                                }
                            },
                            Err(e) => {
                                errors.push(format!("{name}: {e}"));
                                continue;
                            }
                        }
                    }
                    let policy = existing.policy();
                    let src_path = path_to_slash(&p);
                    if p.is_dir() {
                        if let Err(e) = engine
                            .enqueue_tree(
                                local.clone(),
                                &src_path,
                                session.fs.clone(),
                                &target,
                                policy,
                            )
                            .await
                        {
                            errors.push(format!("{name}: {e}"));
                        }
                    } else {
                        engine.enqueue(Job {
                            src: local.clone(),
                            src_path,
                            dst: session.fs.clone(),
                            dst_path: target,
                            policy,
                        });
                    }
                }
                Ok::<_, anyhow::Error>(errors)
            }
        });
        window.toast(
            Toast::info(t!("tx-drop-started", count = count, folder = shown)),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |_, window, cx| match result {
                Ok(Ok(errors)) if errors.is_empty() => {}
                Ok(Ok(errors)) => window.toast(Toast::error(errors.join("; ")), cx),
                Ok(Err(e)) => window.toast(Toast::error(format!("{e:#}")), cx),
                Err(e) => window.toast(Toast::error(e.to_string()), cx),
            });
        })
        .detach();
    }

    // ---- paths and hosts found in the output --------------------------------------

    fn open_remote_path(
        &mut self,
        source: &Entity<TerminalView>,
        pane_id: Option<u64>,
        path: String,
        download: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, pane)) = pane_id.and_then(|id| self.locate(id)) else {
            return;
        };
        let profile = pane.profile.clone();
        let kind = source.read(cx).transport();
        if kind != TransportKind::Ssh {
            window.toast(
                Toast::info(t!("tx-drop-ssh-only", protocol = kind.label())),
                cx,
            );
            return;
        }
        if download {
            self.download_path(profile, path, window, cx);
            return;
        }
        // An open file browser of this host (preferably the one following
        // this terminal), else a new one.
        let term = pane_id;
        let existing = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TabKind::Files)
            .flat_map(|(ix, t)| t.panes().into_iter().map(move |p| (ix, p)))
            .filter(|(_, p)| p.profile.id == profile.id)
            .filter_map(|(ix, p)| match &p.body {
                PaneBody::Files(v) => Some((ix, p.follows == term, v.clone())),
                _ => None,
            })
            .max_by_key(|(_, follows, _)| *follows);
        match existing {
            Some((ix, _, view)) => {
                view.update(cx, |v, cx| v.reveal_remote(path, cx));
                self.activate_tab(ix, window, cx);
            }
            None => {
                let quick = self
                    .locate(pane_id.unwrap_or(0))
                    .and_then(|(_, p)| p.quick.clone());
                let mut files = self.new_pane(profile, quick);
                files.start_dir = Some(path);
                self.push_tab(files, TabKind::Files, window, cx);
            }
        }
    }

    /// Download a remote file or folder into a folder the person picks.
    fn download_path(
        &mut self,
        profile: HostProfile,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picked = std::env::var_os("HOSTWICK_DOWNLOAD_DIR").map(PathBuf::from);
        let rx = picked.is_none().then(|| {
            cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some(t!("tx-xfer-folder-prompt")),
            })
        });
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let dest = match (picked, rx) {
                (Some(d), _) => d,
                (None, Some(rx)) => match rx.await {
                    Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
                    _ => return,
                },
                _ => return,
            };
            let engine = backend.engine.clone();
            let job = backend.rt.spawn({
                let backend = backend.clone();
                async move {
                    let session = backend.open_remote(profile, false).await?;
                    let fs = session.fs;
                    let abs = fs.canonicalize(&path).await.unwrap_or(path);
                    let entry = fs.stat(&abs).await?;
                    let Some(entry) = entry else {
                        anyhow::bail!("{abs}: {}", t!("tx-open-missing"));
                    };
                    let name = hostwick_term::xfer::safe_file_name(&entry.name)
                        .or_else(|| hostwick_term::xfer::safe_file_name(&abs))
                        .unwrap_or_else(|| "download".to_owned());
                    let target = hostwick_term::xfer::unique_path(&dest, &name);
                    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
                    let dst = path_to_slash(&target);
                    if entry.is_dir() {
                        engine
                            .enqueue_tree(fs, &abs, local, &dst, ExistingPolicy::Overwrite)
                            .await?;
                    } else {
                        engine.enqueue(Job {
                            src: fs,
                            src_path: abs,
                            dst: local,
                            dst_path: dst,
                            policy: ExistingPolicy::Overwrite,
                        });
                    }
                    Ok::<_, anyhow::Error>(())
                }
            });
            let result = job.await;
            let _ = this.update_in(cx, |_, window, cx| match result {
                Ok(Ok(())) => window.toast(Toast::info(t!("tx-download-started")), cx),
                Ok(Err(e)) => window.toast(Toast::error(format!("{e:#}")), cx),
                Err(e) => window.toast(Toast::error(e.to_string()), cx),
            });
        })
        .detach();
    }

    /// "Connect" on an address in the output: quick connect, prefilled.
    fn connect_from_terminal(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let quick = self.quick.clone();
        quick.update(cx, |i, cx| {
            i.set_value(text, window, cx);
            i.focus(window, cx);
        });
        self.quick_open = true;
        cx.notify();
    }

    /// A `file:` link was clicked in the output: ask, then show it in the
    /// operating system's file manager (never open it: that could run it).
    fn confirm_reveal(
        &mut self,
        source: &Entity<TerminalView>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = source.read(cx).focus_handle(cx);
        let shown: SharedString = path.clone().into();
        let go_path = PathBuf::from(&path);
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let (go_path, back) = (go_path.clone(), back.clone());
            let go = {
                let p = go_path.clone();
                move |window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    if p.exists() {
                        cx.reveal_path(&p);
                    } else {
                        window.toast(Toast::info(t!("tx-link-file-missing")), cx);
                    }
                }
            };
            let go_ok = go.clone();
            dialog
                .title(crate::a11y::text(t!("tx-link-file-title")))
                .w(px(520.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .text_size(px(13.0))
                        .child(crate::a11y::text(t!("tx-link-file-body")))
                        .child(
                            div()
                                .p(px(8.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(theme.border)
                                .font_family(crate::terminal::monospace_family())
                                .whitespace_normal()
                                .child(shown.clone()),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    go_ok(window, cx);
                    false
                })
                .on_close(move |_, window, cx| back.focus(window, cx))
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
                            Button::new("show")
                                .primary()
                                .label(t!("tx-link-file-go"))
                                .on_click(move |_, window, cx| go(window, cx)),
                        ),
                )
        });
    }

    // ---- shell integration: "Enable now" -----------------------------------------------

    /// Turn shell integration on for the host (saved hosts keep it) and
    /// set it up in the running session without reconnecting.
    pub(crate) fn enable_integration(
        &mut self,
        source: &Entity<TerminalView>,
        pane_id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = pane_id
            && let Some((_, pane)) = self.locate(id)
            && !pane.profile.shell_integration
        {
            let mut p = pane.profile.clone();
            p.shell_integration = true;
            if self.profiles.iter().any(|x| x.id == p.id) {
                self.save_profiles(vec![p], cx);
            } else if let Some(pane) = self.pane_mut(id) {
                // Not a saved host (quick connect): this session only.
                pane.profile = p;
            }
        }
        let ok = source.update(cx, |v, cx| v.inject_integration(cx));
        if ok {
            window.toast(Toast::success(t!("tx-integration-enabled")), cx);
        } else {
            window.toast(Toast::info(t!("tx-integration-later")), cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::finished_texts;

    #[test]
    fn os_notifications_never_carry_the_command() {
        let secret = "mysql -u root -pHUNTER2 -e 'drop table x'";
        let (app, os) = finished_texts("db1", Some(secret), Some(1), Duration::from_secs(42));
        assert!(app.contains("mysql"), "{app}");
        assert!(
            os.contains("db1") && os.contains('1') && os.contains("42"),
            "{os}"
        );
        for banned in ["mysql", "HUNTER2", "drop table"] {
            assert!(!os.contains(banned), "{banned} leaked into: {os}");
        }
        // Without a command both read sensibly.
        let (app, os) = finished_texts("web", None, None, Duration::from_secs(3));
        assert!(app.contains("web") && os.contains("web"));
    }

    #[test]
    fn the_in_app_text_is_short_and_clean() {
        let long = format!("echo {}", "x".repeat(500));
        let (app, _) = finished_texts("h", Some(&long), Some(0), Duration::from_secs(1));
        assert!(app.chars().count() < 160, "{}", app.chars().count());
        let (app, _) = finished_texts(
            "h",
            Some("a\u{1b}[2Jb\u{202e}c"),
            Some(0),
            Duration::from_secs(1),
        );
        assert!(
            !app.contains('\u{1b}') && !app.contains('\u{202e}'),
            "{app}"
        );
    }
}
