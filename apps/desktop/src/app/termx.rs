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
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{Environment, HostProfile};
use hostwick_sftp::{LocalFs, RemoteFs, path_to_slash};
use hostwick_term::TransportKind;
use hostwick_transfer::{ExistingPolicy, Job};

use super::{HostwickApp, PaneBody, Tab, TabKind};
use crate::notify::{Toast, ToastExt as _};
use crate::terminal::{TermExt, TerminalView, links, paste};

/// Per-app state of this feature.
#[derive(Default)]
pub(crate) struct State {
    /// Tabs with a bell or a finished command, not looked at yet.
    bells: HashSet<u64>,
}

/// Marker types for notification ids.
struct FinishedNote;

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
            TermExt::ConfirmPaste { text } => self.confirm_paste(source, text.clone(), window, cx),
            TermExt::DropFiles(paths) => {
                self.drop_upload(source, pane_id, paths.clone(), window, cx)
            }
            TermExt::OpenPath { path, download } => {
                self.open_remote_path(source, pane_id, path.clone(), *download, window, cx)
            }
            TermExt::Connect(text) => self.connect_from_terminal(text.clone(), window, cx),
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
        let time = crate::terminal::format_duration(took);
        let code = exit.map_or_else(|| "?".to_owned(), |c| c.to_string());
        let text: SharedString = match &command {
            Some(c) => t!(
                "tx-finished",
                host = host.clone(),
                command = c.clone(),
                exit = code,
                time = time
            ),
            None => t!(
                "tx-finished-nocmd",
                host = host.clone(),
                exit = code,
                time = time
            ),
        };
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
            let note = Notification::info(text)
                .title(host)
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

    fn confirm_paste(
        &mut self,
        source: &Entity<TerminalView>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let info = source.read(cx).host_info().clone();
        let peers = if self.broadcast {
            self.all_terminals().len().saturating_sub(1)
        } else {
            0
        };
        let view = source.clone();
        let pv = paste::preview(&text, 8, 110);
        let prod = info.env.is_prod();
        let title: SharedString = if text.len() > paste::LARGE && pv.total_lines <= 1 {
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
                            d.child(
                                div()
                                    .px(px(8.0))
                                    .py(px(4.0))
                                    .rounded(px(4.0))
                                    .bg(hsla(0.0, 0.65, 0.40, 1.0))
                                    .text_color(hsla(0.0, 0.0, 1.0, 1.0))
                                    .child(crate::a11y::text(t!(
                                        "tx-paste-prod",
                                        host = info.name.clone()
                                    ))),
                            )
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
        match source.read(cx).cwd() {
            Some(dir) => self.start_upload(profile, paths, dir, window, cx),
            None => self.ask_upload_dir(source, profile, paths, window, cx),
        }
    }

    /// Queue `paths` for upload into `dir` (folders recursively); the
    /// transfer panel shows the progress.
    fn start_upload(
        &mut self,
        profile: HostProfile,
        paths: Vec<PathBuf>,
        dir: String,
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
                    let target = match hostwick_sftp::join_checked(&dir, &name) {
                        Ok(t) => t,
                        Err(e) => {
                            errors.push(format!("{name}: {e}"));
                            continue;
                        }
                    };
                    let src_path = path_to_slash(&p);
                    if p.is_dir() {
                        if let Err(e) = engine
                            .enqueue_tree(
                                local.clone(),
                                &src_path,
                                session.fs.clone(),
                                &target,
                                ExistingPolicy::Overwrite,
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
                            policy: ExistingPolicy::Overwrite,
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

    /// The shell has not reported its folder: ask where to put the files,
    /// and offer to turn on shell integration so it will next time.
    fn ask_upload_dir(
        &mut self,
        source: &Entity<TerminalView>,
        profile: HostProfile,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let guess = links::resolve_path("~/x", None, Some(&profile.user_or_local()))
            .map(|p| p.trim_end_matches("/x").to_owned())
            .unwrap_or_else(|| "/".to_owned());
        let input = cx.new(|cx| InputState::new(window, cx).default_value(guess.clone()));
        input.update(cx, |i, cx| i.focus(window, cx));
        // The real home folder, when the server tells.
        {
            let backend = self.backend.clone();
            let p = profile.clone();
            let job = backend.rt.spawn({
                let backend = backend.clone();
                async move {
                    let s = backend.open_remote(p, false).await.ok()?;
                    s.fs.canonicalize(".").await.ok()
                }
            });
            let (input, guess) = (input.clone(), guess.clone());
            cx.spawn_in(window, async move |_, cx| {
                if let Ok(Some(home)) = job.await {
                    let _ = cx.update(|window, cx| {
                        input.update(cx, |i, cx| {
                            if i.value().as_ref() == guess {
                                i.set_value(home, window, cx);
                            }
                        });
                    });
                }
            })
            .detach();
        }
        let integration_on = profile.shell_integration;
        let me = cx.entity().downgrade();
        let source = source.clone();
        let paths = Arc::new(paths);
        let count = paths.len();
        let back = source.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let (me1, me2) = (me.clone(), me.clone());
            let (input1, input2) = (input.clone(), input.clone());
            let (paths1, paths2) = (paths.clone(), paths.clone());
            let (profile1, profile2) = (profile.clone(), profile.clone());
            let source2 = source.clone();
            let back = back.clone();
            let go = move |window: &mut Window,
                           cx: &mut App,
                           me: &WeakEntity<HostwickApp>,
                           input: &Entity<InputState>,
                           paths: &Arc<Vec<PathBuf>>,
                           profile: &HostProfile| {
                let dir = input.read(cx).value().trim().to_owned();
                if dir.is_empty() {
                    return;
                }
                window.close_dialog(cx);
                let (paths, profile) = ((**paths).clone(), profile.clone());
                let _ = me.update(cx, |app, cx| {
                    app.start_upload(profile, paths, dir, window, cx)
                });
            };
            let go2 = go;
            dialog
                .title(crate::a11y::text(t!("tx-drop-title", count = count)))
                .w(px(560.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .text_size(px(13.0))
                        .child(crate::a11y::text(t!("tx-drop-to")))
                        .child(Input::new(&input))
                        .child(
                            div()
                                .text_color(muted)
                                .whitespace_normal()
                                .child(crate::a11y::text(t!("tx-drop-hint"))),
                        )
                        .when(!integration_on, |d| {
                            let src = source2.clone();
                            d.child(
                                div().flex().child(
                                    Button::new("enable-integration")
                                        .label(t!("tx-enable-now"))
                                        .on_click(move |_, _, cx| {
                                            src.update(cx, |_, cx| {
                                                cx.emit(crate::terminal::TerminalEvent::Ext(
                                                    TermExt::EnableIntegration,
                                                ))
                                            });
                                        }),
                                ),
                            )
                        }),
                )
                .on_ok(move |_, window, cx| {
                    go(window, cx, &me1, &input1, &paths1, &profile1);
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
                            Button::new("upload")
                                .primary()
                                .label(t!("tx-drop-upload"))
                                .on_click(move |_, window, cx| {
                                    go2(window, cx, &me2, &input2, &paths2, &profile2);
                                }),
                        ),
                )
        });
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
