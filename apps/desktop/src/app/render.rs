//! Drawing the root view: sidebar, tab bar, pane trees, transfer queue.

use std::rc::Rc;

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::Protocol;
use hostwick_transfer::TransferState;

use super::{HostwickApp, Pane, PaneBody, SidebarRow, TabKind, logo};
use crate::layout::Node;
use crate::util::{human_size, keys, protocol_name};

/// Divider between split panes, in pixels (easy to grab).
const DIVIDER: f32 = 5.0;
/// Smallest pane a divider can be dragged to, in pixels.
const MIN_PANE: f32 = 120.0;

/// What is being dragged when a divider moves.
#[derive(Clone)]
pub(super) struct DraggedDivider {
    split: u64,
}

/// Dragging a divider shows nothing extra: the panes resize live.
struct DividerGhost;

impl Render for DividerGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Short protocol badge for a host row.
fn badge(p: &Protocol) -> &'static str {
    match p {
        Protocol::Ssh => "SSH",
        Protocol::Telnet => "TEL",
        Protocol::Serial(_) => "COM",
    }
}

impl HostwickApp {
    pub(super) fn sidebar_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let needle = self.search.read(cx).value().to_lowercase();
        let mut rows = Vec::new();
        let mut last_folder: Option<&str> = None;
        for (i, p) in self.profiles.iter().enumerate() {
            if !needle.is_empty() {
                let hay = format!(
                    "{} {} {} {} {}",
                    p.path(),
                    p.host,
                    p.user.as_deref().unwrap_or(""),
                    p.protocol.label(),
                    protocol_name(&p.protocol)
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

    fn render_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows = Rc::new(self.sidebar_rows(cx));
        let count = rows.len();
        let has_sel = self.selected.is_some();
        let list_focused = self.list_focus.is_focused(window);
        let anchor = self.host_anchor.clone();
        anchor.clear();
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
                                        .tooltip(t!("sidebar-vault-unlocked-tooltip"))
                                        .accessibility_label(t!("sidebar-vault-lock"))
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
                                    .tooltip(t!(
                                        "sidebar-settings-tooltip",
                                        keys = keys("Ctrl+,", "⌘,")
                                    ))
                                    .accessibility_label(t!("settings-title"))
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.open_settings(None, w, cx)),
                                    ),
                            ),
                    )
                    .child(
                        Input::new(&self.quick)
                            .small()
                            .aria_label(t!("sidebar-quick-connect")),
                    )
                    .child(
                        Input::new(&self.search)
                            .small()
                            .cleanable(true)
                            .aria_label(t!("sidebar-search-placeholder")),
                    ),
            )
            .child(
                div()
                    .id("hosts")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .track_focus(&self.list_focus)
                    .role(Role::ListBox)
                    .aria_label(t!("sidebar-hosts-label"))
                    .aria_description(t!("sidebar-hosts-keys"))
                    .on_key_down(cx.listener(Self::on_list_key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, window, cx| {
                            app.list_focus.focus(window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        uniform_list(
                            "profiles",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let theme = cx.theme().clone();
                                let anchor = anchor.clone();
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
                                            .child(
                                                div()
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .child(f.clone()),
                                            )
                                            .into_any_element(),
                                        SidebarRow::Profile(i) => {
                                            let p = &this.profiles[*i];
                                            let selected = this.selected == Some(p.id);
                                            let id = p.id;
                                            let indent =
                                                if p.folder.is_empty() { 10.0 } else { 22.0 };
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
                                            let tip = format!(
                                                "{} — {} {address}",
                                                p.path(),
                                                protocol_name(&p.protocol)
                                            );
                                            let spoken = format!(
                                                "{}, {}, {address}",
                                                p.path(),
                                                protocol_name(&p.protocol)
                                            );
                                            div()
                                                .id(("profile", ix))
                                                .role(Role::ListBoxOption)
                                                .aria_label(spoken)
                                                .aria_selected(selected)
                                                .when(selected, |d| {
                                                    d.aria_active_descendant()
                                                        .relative()
                                                        .child(anchor.track())
                                                })
                                                .when(selected && list_focused, |d| {
                                                    d.border_2().border_color(theme.ring)
                                                })
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
                                                            app.open_tab(
                                                                p,
                                                                TabKind::Terminal,
                                                                window,
                                                                cx,
                                                            );
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
                                                .context_menu(Self::host_menu(
                                                    cx.entity().downgrade(),
                                                    id,
                                                ))
                                                .into_any_element()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.list_scroll)
                        .flex_1(),
                    ),
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
                                    .label(t!("sidebar-open-terminal"))
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        app.open_selected(TabKind::Terminal, w, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-files")
                                    .small()
                                    .label(t!("sidebar-open-files"))
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
                                    .tooltip(t!("sidebar-edit-host"))
                                    .accessibility_label(t!("sidebar-edit-host"))
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
                                    .tooltip(t!("sidebar-delete-host"))
                                    .accessibility_label(t!("sidebar-delete-host"))
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
                            .child(
                                Button::new("new-host")
                                    .small()
                                    .label(t!("sidebar-new-host"))
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.host_dialog(None, w, cx)),
                                    ),
                            )
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

    pub(super) fn render_tabs(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active = self.active;
        let bar_focused = self.tabs_focus.is_focused(window);
        let anchor = self.tab_anchor.clone();
        anchor.clear();
        let count = self.tabs.len();
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
                    .track_focus(&self.tabs_focus)
                    .role(Role::TabList)
                    .aria_label(t!("tabs-label"))
                    .aria_description(t!("tabs-keys"))
                    .on_key_down(cx.listener(Self::on_tabs_key))
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
                                let name = t.profile().map(|p| p.name.clone()).unwrap_or_default();
                                let title = t!("tab-files-title", name = name);
                                if linked {
                                    format!("{title} ⇄").into()
                                } else {
                                    title
                                }
                            }
                            (_, 1) => base,
                            (_, n) => format!("{base} +{}", n - 1).into(),
                        };
                        let pending = panes.iter().all(|p| matches!(p.body, PaneBody::Pending));
                        let tab_id = t.id;
                        let state = if pending {
                            t!("tab-state-not-connected")
                        } else if alive {
                            t!("tab-state-connected")
                        } else {
                            t!("tab-state-disconnected")
                        };
                        let close_label = t!("tab-close", title = title.clone());
                        let spoken = t!(
                            "tab-spoken",
                            title = title.clone(),
                            state = state,
                            index = ix + 1,
                            count = count
                        );
                        div()
                            .id(("tab", t.id))
                            .role(Role::Tab)
                            .aria_label(spoken)
                            .aria_selected(is_active)
                            .when(is_active, |d| {
                                d.aria_active_descendant().relative().child(anchor.track())
                            })
                            .when(is_active && bar_focused, |d| {
                                d.border_2().border_color(theme.ring)
                            })
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
                                    .tab_stop(false)
                                    .accessibility_label(close_label)
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
                            t!("broadcast-on")
                        } else {
                            t!("broadcast")
                        })
                        .when(self.broadcast, |b| b.warning())
                        .tooltip(t!("broadcast-tooltip", keys = keys("Ctrl+Shift+B", "⌘B")))
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
        let line = |items: &[(&str, SharedString)]| {
            items
                .iter()
                .map(|(k, what)| format!("{k} {what}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let shortcuts = [
            line(&[
                (keys("Ctrl+Shift+N", "⌘N"), t!("shortcut-new-host")),
                (keys("Ctrl+Shift+T", "⌘T"), t!("shortcut-duplicate-tab")),
                (keys("Ctrl+Shift+W", "⌘W"), t!("shortcut-close-pane")),
                (keys("Ctrl+Shift+R", "⌘R"), t!("shortcut-reconnect")),
                (keys("Ctrl+Tab", "⌃Tab"), t!("shortcut-next-tab")),
            ]),
            line(&[
                (keys("Ctrl+Shift+D", "⌘D"), t!("shortcut-split-right")),
                (keys("Ctrl+Shift+E", "⌘⇧D"), t!("shortcut-split-down")),
                (keys("Alt+←→↑↓", "⌥←→↑↓"), t!("shortcut-move-between-panes")),
                (keys("Ctrl+Shift+B", "⌘B"), t!("shortcut-broadcast")),
                (keys("Ctrl+Shift+F", "⌘F"), t!("shortcut-find")),
                (keys("Ctrl+,", "⌘,"), t!("shortcut-settings")),
            ]),
        ];
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(24.0))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .text_size(px(18.0))
                    .child(crate::a11y::text(t!("empty-title"))),
            )
            .child(
                div()
                    .text_center()
                    .child(crate::a11y::text(t!("empty-hint-connect"))),
            )
            .child(
                div()
                    .text_center()
                    .child(crate::a11y::text(t!("empty-hint-menus"))),
            )
            .child(
                div()
                    .text_center()
                    .child(crate::a11y::text(t!("empty-hint-import"))),
            )
            .child(
                div()
                    .pt(px(12.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(12.0))
                    .children(
                        shortcuts
                            .into_iter()
                            .map(|s| div().text_center().child(crate::a11y::text(s))),
                    ),
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
                .child(crate::a11y::text(t!(
                    "pane-connecting",
                    host = &pane.profile.host
                )))
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
                        .id("connect-error")
                        .role(Role::Alert)
                        .aria_label(err.clone())
                        .aria_value(err.clone())
                        .text_color(theme.danger)
                        .max_w(px(640.0))
                        .whitespace_normal()
                        .child(err.clone()),
                )
                .child(
                    Button::new(("retry", pane_id))
                        .primary()
                        .label(t!("pane-retry"))
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
            Node::Split {
                id,
                vertical,
                ratio,
                a,
                b,
            } => {
                let first = self.render_node(a, focused, multi, cx);
                let second = self.render_node(b, focused, multi, cx);
                self.render_split(*id, *vertical, *ratio, first, second, cx)
            }
        }
    }

    /// Two panes with a draggable divider between them. The divider keeps
    /// both panes at least `MIN_PANE` big and can also be moved by screen
    /// readers (increment/decrement) and with Alt+Shift+arrows.
    fn render_split(
        &self,
        id: u64,
        vertical: bool,
        ratio: f32,
        first: AnyElement,
        second: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let r = crate::layout::clamp_ratio(ratio);
        let (inc, dec) = (cx.entity().downgrade(), cx.entity().downgrade());
        let pane = |child: AnyElement, share: f32| {
            div()
                .relative()
                .overflow_hidden()
                .flex_basis(relative(share))
                .flex_grow(1.0)
                .flex_shrink(1.0)
                .min_w_0()
                .min_h_0()
                .child(child)
        };
        let divider = div()
            .id(("divider", id))
            .role(Role::Splitter)
            .aria_label(t!("pane-divider"))
            .aria_orientation(if vertical {
                gpui_kit::accesskit::Orientation::Horizontal
            } else {
                gpui_kit::accesskit::Orientation::Vertical
            })
            .aria_numeric_value(f64::from((r * 100.0).round()))
            .aria_min_numeric_value(5.0)
            .aria_max_numeric_value(95.0)
            .aria_numeric_value_step(5.0)
            .on_a11y_action(gpui_kit::accesskit::Action::Increment, move |_, _, cx| {
                let _ = inc.update(cx, |app, cx| app.nudge_split(id, 0.05, cx));
            })
            .on_a11y_action(gpui_kit::accesskit::Action::Decrement, move |_, _, cx| {
                let _ = dec.update(cx, |app, cx| app.nudge_split(id, -0.05, cx));
            })
            .flex_none()
            .bg(theme.border)
            .hover(|s| s.bg(theme.primary))
            .map(|d| {
                if vertical {
                    d.h(px(DIVIDER)).w_full().cursor_row_resize()
                } else {
                    d.w(px(DIVIDER)).h_full().cursor_col_resize()
                }
            })
            .on_drag(DraggedDivider { split: id }, |_, _, _, cx| {
                cx.new(|_| DividerGhost)
            });
        div()
            .id(("split", id))
            .size_full()
            .flex()
            .when(vertical, |d| d.flex_col())
            .on_drag_move(
                cx.listener(move |app, ev: &DragMoveEvent<DraggedDivider>, _, cx| {
                    if ev.drag(cx).split != id {
                        return;
                    }
                    let b = ev.bounds;
                    let (pos, start, len) = if vertical {
                        (ev.event.position.y, b.origin.y, b.size.height)
                    } else {
                        (ev.event.position.x, b.origin.x, b.size.width)
                    };
                    let len = f32::from(len);
                    if len <= 0.0 {
                        return;
                    }
                    // Keep both panes usable; a very small split stays even.
                    let min = (MIN_PANE / len).min(0.5);
                    let r = (f32::from(pos - start) / len).clamp(min, 1.0 - min);
                    app.set_split_ratio(id, r, cx);
                }),
            )
            .on_drop(cx.listener(|app, _: &DraggedDivider, _, cx| app.persist_tabs(cx)))
            .child(pane(first, r))
            .child(divider)
            .child(pane(second, 1.0 - r))
            .into_any_element()
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
                    t!(
                        "transfer-progress",
                        done = human_size(t.done),
                        total = human_size(t.total),
                        rate = human_size(t.rate as u64)
                    )
                    .to_string(),
                    theme.primary,
                ),
                Some(TransferState::Done { .. }) => (
                    t!("transfer-done", size = human_size(t.total)).to_string(),
                    theme.success,
                ),
                Some(TransferState::Skipped) => {
                    (t!("transfer-skipped").to_string(), theme.muted_foreground)
                }
                Some(TransferState::Cancelled) => {
                    (t!("transfer-cancelled").to_string(), theme.muted_foreground)
                }
                Some(TransferState::Failed { error }) => (
                    t!("transfer-failed", error = error.to_string()).to_string(),
                    theme.danger,
                ),
                Some(_) => (String::new(), theme.muted_foreground),
            };
            let id = t.id;
            let running = t.state.is_none();
            div()
                .id(SharedString::from(format!("transfer-row-{}", id.0)))
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
                        .child(crate::a11y::text(t.label.clone())),
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
                        .child(crate::a11y::text(status)),
                )
                .when(running, |d| {
                    d.child(
                        Button::new(SharedString::from(format!("cancel-{}", id.0)))
                            .xsmall()
                            .ghost()
                            .label(t!("transfer-cancel"))
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
                        .child(t!("transfers-title", count = active))
                        .child(div().flex_1())
                        .child(
                            Button::new("clear-transfers")
                                .xsmall()
                                .ghost()
                                .label(t!("transfers-clear"))
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toasts = crate::notify::layer(window, cx);
        let sidebar = self.render_sidebar(window, cx);
        let tabs = self.render_tabs(window, cx);
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
            .children(toasts)
    }
}
