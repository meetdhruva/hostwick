//! Drawing the root view: sidebar, tab bar, pane trees, transfer queue.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::Protocol;
use hostwick_transfer::TransferState;

use super::{HostwickApp, Pane, PaneBody, SidebarRow, TabKind, logo};
use crate::layout::Node;
use crate::util::human_size;

/// Short protocol badge for a host row.
fn badge(p: &Protocol) -> &'static str {
    match p {
        Protocol::Ssh => "SSH",
        Protocol::Telnet => "TEL",
        Protocol::Serial(_) => "COM",
    }
}

impl HostwickApp {
    fn sidebar_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let needle = self.search.read(cx).value().to_lowercase();
        let mut rows = Vec::new();
        let mut last_folder: Option<&str> = None;
        for (i, p) in self.profiles.iter().enumerate() {
            if !needle.is_empty() {
                let hay = format!(
                    "{} {} {} {}",
                    p.path(),
                    p.host,
                    p.user.as_deref().unwrap_or(""),
                    p.protocol.label()
                )
                .to_lowercase();
                if !needle.split_whitespace().all(|w| hay.contains(w)) {
                    continue;
                }
            }
            if last_folder != Some(p.folder.as_str()) {
                if !p.folder.is_empty() {
                    rows.push(SidebarRow::Folder(p.folder.clone()));
                }
                last_folder = Some(p.folder.as_str());
            }
            rows.push(SidebarRow::Profile(i));
        }
        rows
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows = Rc::new(self.sidebar_rows(cx));
        let count = rows.len();
        let has_sel = self.selected.is_some();
        div()
            .w(px(260.0))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .p(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(img(logo()).size(px(22.0)))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Hostwick"),
                            )
                            .when(self.backend.vault.is_unlocked(), |d| {
                                d.child(
                                    Button::new("vault-lock")
                                        .ghost()
                                        .small()
                                        .icon(gpui_kit::assets::IconName::LockOpen)
                                        .tooltip("Saved passwords are unlocked — click to lock")
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            app.backend.vault.lock();
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(Self::tools_button(cx.entity().downgrade()))
                            .child(
                                Button::new("settings")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Settings)
                                    .tooltip("Settings (Ctrl+,)")
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.open_settings(None, w, cx)),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.quick).small())
                    .child(Input::new(&self.search).small().cleanable(true)),
            )
            .child(
                uniform_list(
                    "profiles",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let theme = cx.theme().clone();
                        range
                            .map(|ix| match &rows[ix] {
                                SidebarRow::Folder(f) => div()
                                    .id(("folder", ix))
                                    .w_full()
                                    .h(px(28.0))
                                    .px(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .text_size(px(12.0))
                                    .text_color(theme.muted_foreground)
                                    .child(IconName::FolderOpen)
                                    .child(div().overflow_hidden().text_ellipsis().child(f.clone()))
                                    .into_any_element(),
                                SidebarRow::Profile(i) => {
                                    let p = &this.profiles[*i];
                                    let selected = this.selected == Some(p.id);
                                    let id = p.id;
                                    let indent = if p.folder.is_empty() { 10.0 } else { 22.0 };
                                    let serial = matches!(p.protocol, Protocol::Serial(_));
                                    let address = match &p.protocol {
                                        Protocol::Serial(s) => format!(
                                            "{} {}",
                                            p.host,
                                            hostwick_term::serial::summary(s)
                                        ),
                                        _ => {
                                            let user = p
                                                .user
                                                .as_ref()
                                                .map(|u| format!("{u}@"))
                                                .unwrap_or_default();
                                            if p.port == p.protocol.default_port() {
                                                format!("{user}{}", p.host)
                                            } else {
                                                format!("{user}{}:{}", p.host, p.port)
                                            }
                                        }
                                    };
                                    let tip =
                                        format!("{} — {} {address}", p.path(), p.protocol.label());
                                    div()
                                        .id(("profile", ix))
                                        .w_full()
                                        .h(px(28.0))
                                        .pl(px(indent))
                                        .pr(px(8.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .text_size(px(13.0))
                                        .when(selected, |d| d.bg(theme.sidebar_accent))
                                        .when(!selected, |d| {
                                            d.hover(|s| s.bg(theme.sidebar_accent))
                                        })
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(tip.clone()).build(window, cx)
                                        })
                                        .on_click(cx.listener(
                                            move |app, ev: &ClickEvent, window, cx| {
                                                app.selected = Some(id);
                                                if ev.click_count() >= 2
                                                    && let Some(p) = app
                                                        .profiles
                                                        .iter()
                                                        .find(|p| p.id == id)
                                                        .cloned()
                                                {
                                                    app.open_tab(p, TabKind::Terminal, window, cx);
                                                }
                                                cx.notify();
                                            },
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |app, _, _, cx| {
                                                app.selected = Some(id);
                                                cx.notify();
                                            }),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .w(px(30.0))
                                                .text_size(px(9.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_center()
                                                .rounded(px(3.0))
                                                .border_1()
                                                .border_color(theme.border)
                                                .text_color(theme.muted_foreground)
                                                .child(badge(&p.protocol)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(40.0))
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(p.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .max_w(px(110.0))
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted_foreground)
                                                .map(|d| {
                                                    // Device paths differ at the end.
                                                    if serial {
                                                        d.text_ellipsis_start()
                                                    } else {
                                                        d.text_ellipsis()
                                                    }
                                                })
                                                .child(p.host.clone()),
                                        )
                                        .context_menu(Self::host_menu(cx.entity().downgrade(), id))
                                        .into_any_element()
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .child(
                div()
                    .p(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                Button::new("open-term")
                                    .small()
                                    .primary()
                                    .label("Terminal")
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        app.open_selected(TabKind::Terminal, w, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-files")
                                    .small()
                                    .label("Files")
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        app.open_selected(TabKind::Files, w, cx)
                                    })),
                            )
                            .child(
                                Button::new("edit-host")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Settings2)
                                    .tooltip("Edit host")
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        let p = app
                                            .selected
                                            .and_then(|id| app.profiles.iter().find(|p| p.id == id))
                                            .cloned();
                                        if p.is_some() {
                                            app.host_dialog(p, w, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("del-host")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip("Delete host")
                                    .disabled(!has_sel)
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.delete_selected(w, cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(Button::new("new-host").small().label("New host…").on_click(
                                cx.listener(|app, _, w, cx| app.host_dialog(None, w, cx)),
                            ))
                            .child(Self::import_button(cx.entity().downgrade())),
                    ),
            )
            .into_any_element()
    }

    fn pane_title(p: &Pane, cx: &App) -> SharedString {
        match &p.body {
            PaneBody::Terminal(v) => {
                let vt = v.read(cx).title.clone();
                if vt.is_empty() {
                    crate::terminal::tab_title(&p.profile).into()
                } else {
                    vt
                }
            }
            _ => crate::terminal::tab_title(&p.profile).into(),
        }
    }

    fn pane_alive(p: &Pane, cx: &App) -> bool {
        match &p.body {
            PaneBody::Terminal(v) => v.read(cx).is_alive(),
            PaneBody::Failed(_) => false,
            _ => true,
        }
    }

    pub(super) fn render_tabs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active = self.active;
        div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("tabs")
                    .flex_1()
                    .flex()
                    .h_full()
                    .overflow_x_scroll()
                    .children(self.tabs.iter().enumerate().map(|(ix, t)| {
                        let is_active = active == Some(ix);
                        let panes = t.panes();
                        let alive = panes.iter().all(|p| Self::pane_alive(p, cx));
                        let base = t
                            .focused_pane()
                            .map(|p| Self::pane_title(p, cx))
                            .unwrap_or_default();
                        let title: SharedString = match (t.kind, panes.len()) {
                            (TabKind::Files, _) => {
                                let linked = panes.iter().any(|p| p.follows.is_some());
                                format!(
                                    "Files: {}{}",
                                    t.profile().map(|p| p.name.as_str()).unwrap_or(""),
                                    if linked { " ⇄" } else { "" }
                                )
                                .into()
                            }
                            (_, 1) => base,
                            (_, n) => format!("{base} +{}", n - 1).into(),
                        };
                        let pending = panes.iter().all(|p| matches!(p.body, PaneBody::Pending));
                        let tab_id = t.id;
                        div()
                            .id(("tab", t.id))
                            .h_full()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .max_w(px(240.0))
                            .text_size(px(13.0))
                            .border_r_1()
                            .border_color(theme.border)
                            .when(is_active, |d| {
                                d.bg(theme.tab_active)
                                    .text_color(theme.tab_active_foreground)
                            })
                            .when(!is_active, |d| d.text_color(theme.muted_foreground))
                            .on_click(cx.listener(move |app, _, window, cx| {
                                if let Some(ix) = app.tabs.iter().position(|t| t.id == tab_id) {
                                    app.activate_tab(ix, window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(7.0))
                                    .rounded_full()
                                    .bg(if pending {
                                        theme.muted_foreground
                                    } else if alive {
                                        theme.success
                                    } else {
                                        theme.danger
                                    }),
                            )
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .context_menu(Self::tab_menu(cx.entity().downgrade(), t.id))
                            .child(
                                Button::new(("close", t.id))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        if let Some(ix) =
                                            app.tabs.iter().position(|t| t.id == tab_id)
                                        {
                                            app.close_tab(ix, cx);
                                        }
                                    })),
                            )
                    })),
            )
            .child(
                div().px(px(8.0)).child(
                    Button::new("broadcast")
                        .small()
                        .label(if self.broadcast {
                            "Broadcast: ON"
                        } else {
                            "Broadcast"
                        })
                        .when(self.broadcast, |b| b.warning())
                        .tooltip("Send keystrokes to every open terminal (Ctrl+Shift+B)")
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.broadcast = !app.broadcast;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_empty(&self, cx: &App) -> AnyElement {
        let theme = cx.theme().clone();
        let shortcuts: &[&str] = if cfg!(target_os = "macos") {
            &[
                "⌘N new host · ⌘T duplicate tab · ⌘W close pane/tab · ⌘R reconnect · ⌃Tab next tab",
                "⌘D split right · ⌘⇧D split down · ⌥←→↑↓ move between panes · ⌘B broadcast · ⌘F find · ⌘, settings",
            ]
        } else {
            &[
                "Ctrl+Shift+N new host · Ctrl+Shift+T duplicate tab · Ctrl+Shift+W close pane/tab · Ctrl+Shift+R reconnect · Ctrl+Tab next tab",
                "Ctrl+Shift+D split right · Ctrl+Shift+E split down · Alt+arrows move between panes · Ctrl+Shift+B broadcast · Ctrl+Shift+F find · Ctrl+, settings",
            ]
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(24.0))
            .text_color(theme.muted_foreground)
            .child(div().text_size(px(18.0)).child("No open sessions"))
            .child("Double-click a host, or type user@host in the quick-connect box.")
            .child("Right-click hosts, tabs, files and terminals for more actions.")
            .child("Import from ~/.ssh/config, PuTTY, WinSCP or Solar-PuTTY with Import… (bottom left).")
            .child(
                div()
                    .pt(px(12.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(12.0))
                    .children(shortcuts.iter().map(|s| div().text_center().child(*s))),
            )
            .into_any_element()
    }

    fn render_pane(
        &self,
        pane: &Pane,
        show_focus: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let pane_id = pane.id;
        let inner = match &pane.body {
            PaneBody::Terminal(v) => v.clone().into_any_element(),
            PaneBody::Files(v) => v.clone().into_any_element(),
            PaneBody::Pending | PaneBody::Connecting => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child(format!("Connecting to {}…", pane.profile.host))
                .into_any_element(),
            PaneBody::Failed(err) => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .px(px(16.0))
                .child(
                    div()
                        .text_color(theme.danger)
                        .max_w(px(640.0))
                        .whitespace_normal()
                        .child(err.clone()),
                )
                .child(
                    Button::new(("retry", pane_id))
                        .primary()
                        .label("Retry")
                        .on_click(cx.listener(move |app, _, w, cx| {
                            app.connect_pane(pane_id, w, cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        };
        div()
            .id(("pane", pane_id))
            .size_full()
            .relative()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(move |app, _, window, cx| {
                app.focus_pane(pane_id, window, cx);
            }))
            .child(inner)
            .when(show_focus, |d| {
                // Focus ring drawn over the pane (not affecting layout).
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(if focused {
                            theme.primary
                        } else {
                            gpui_kit::transparent_black()
                        }),
                )
            })
            .into_any_element()
    }

    fn render_node(
        &self,
        node: &Node<Pane>,
        focused: u64,
        multi: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            Node::Empty => div().into_any_element(),
            Node::Leaf(p) => self.render_pane(p, multi, p.id == focused, cx),
            Node::Split { id, vertical, a, b } => {
                let first = self.render_node(a, focused, multi, cx);
                let second = self.render_node(b, focused, multi, cx);
                let group = if *vertical {
                    v_resizable(("split", *id))
                } else {
                    h_resizable(("split", *id))
                };
                group
                    .child(resizable_panel().child(first))
                    .child(resizable_panel().child(second))
                    .into_any_element()
            }
        }
    }

    pub(super) fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.active.and_then(|i| self.tabs.get(i)) else {
            return self.render_empty(cx);
        };
        let multi = tab.root.count() > 1;
        self.render_node(&tab.root, tab.focused, multi, cx)
    }

    pub(super) fn render_transfers(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.transfers.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let active = self.transfers.iter().filter(|t| t.state.is_none()).count();
        let rows = self.transfers.iter().rev().take(6).map(|t| {
            let pct = if t.total == 0 {
                if t.state.is_some() { 1.0 } else { 0.0 }
            } else {
                t.done as f32 / t.total as f32
            };
            let (status, color) = match &t.state {
                None => (
                    format!(
                        "{} of {} — {}/s",
                        human_size(t.done),
                        human_size(t.total),
                        human_size(t.rate as u64)
                    ),
                    theme.primary,
                ),
                Some(TransferState::Done { .. }) => {
                    (format!("done — {}", human_size(t.total)), theme.success)
                }
                Some(TransferState::Skipped) => ("skipped".to_owned(), theme.muted_foreground),
                Some(TransferState::Cancelled) => ("cancelled".to_owned(), theme.muted_foreground),
                Some(TransferState::Failed { error }) => (format!("failed: {error}"), theme.danger),
                Some(_) => (String::new(), theme.muted_foreground),
            };
            let id = t.id;
            let running = t.state.is_none();
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .text_size(px(12.0))
                .child(
                    div()
                        .w(px(380.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(t.label.clone()),
                )
                .child(
                    div()
                        .w(px(160.0))
                        .h(px(6.0))
                        .rounded(px(3.0))
                        .bg(theme.muted)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(3.0))
                                .bg(color)
                                .w(relative(pct.clamp(0.0, 1.0))),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.muted_foreground)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(status),
                )
                .when(running, |d| {
                    d.child(
                        Button::new(SharedString::from(format!("cancel-{}", id.0)))
                            .xsmall()
                            .ghost()
                            .label("Cancel")
                            .on_click(
                                cx.listener(move |app, _, _, _| app.backend.engine.cancel(id)),
                            ),
                    )
                })
        });
        Some(
            div()
                .flex_none()
                .border_t_1()
                .border_color(theme.border)
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(px(12.0))
                        .child(format!("Transfers — {active} active"))
                        .child(div().flex_1())
                        .child(
                            Button::new("clear-transfers")
                                .xsmall()
                                .ghost()
                                .label("Clear finished")
                                .on_click(cx.listener(|app, _, _, cx| {
                                    app.transfers.retain(|t| t.state.is_none());
                                    app.backend.engine.prune_finished();
                                    cx.notify();
                                })),
                        ),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}

impl Render for HostwickApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sidebar = self.render_sidebar(cx);
        let tabs = self.render_tabs(cx);
        let body = self.render_body(cx);
        let transfers = self.render_transfers(cx);
        div()
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_shortcut))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(tabs)
                    .child(div().flex_1().min_h_0().overflow_hidden().child(body))
                    .children(transfers),
            )
    }
}
