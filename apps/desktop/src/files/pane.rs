//! One side of the commander: listing, selection, sorting, rows, drag and
//! drop.

use std::sync::Arc;

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_sftp::{Entry, EntryKind, RemoteFs};

use super::FilesView;
use super::ops::{Selection, SortKey, sort_entries};
use crate::util::{human_size, local_time, mode_string};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Local,
    Remote,
}

impl Side {
    pub fn other(self) -> Self {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }
}

pub struct Pane {
    pub fs: Arc<dyn RemoteFs>,
    pub cwd: String,
    pub entries: Vec<Entry>,
    pub sort: SortKey,
    pub ascending: bool,
    pub sel: Selection,
    pub loading: bool,
    pub error: Option<String>,
    pub path_input: Entity<InputState>,
    pub path_dirty: bool,
    pub focus: FocusHandle,
    pub scroll: UniformListScrollHandle,
    pub generation: u64,
    pub load: Option<Task<()>>,
}

impl Pane {
    pub fn new(
        side: Side,
        fs: Arc<dyn RemoteFs>,
        window: &mut Window,
        cx: &mut Context<FilesView>,
    ) -> (Self, Subscription) {
        let path_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("files-path-placeholder")));
        let sub = cx.subscribe_in(
            &path_input,
            window,
            move |this: &mut FilesView, input, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let path = input.read(cx).value().trim().to_string();
                    if !path.is_empty() {
                        this.navigate(side, path, cx);
                    }
                }
            },
        );
        let pane = Self {
            fs,
            cwd: String::new(),
            entries: Vec::new(),
            sort: SortKey::Name,
            ascending: true,
            sel: Selection::default(),
            loading: false,
            error: None,
            path_input,
            path_dirty: false,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            generation: 0,
            load: None,
        };
        (pane, sub)
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// Sort, keeping the cursor and anchor on the same entries.
    pub fn resort(&mut self) {
        let name_at =
            |ix: Option<usize>, e: &[Entry]| ix.and_then(|i| e.get(i)).map(|e| e.name.clone());
        let cursor = name_at(self.sel.cursor, &self.entries);
        let anchor = name_at(self.sel.anchor, &self.entries);
        sort_entries(&mut self.entries, self.sort, self.ascending);
        self.relocate(cursor, anchor);
    }

    /// Put the cursor/anchor back on the entries with these names (after a
    /// sort or a reload changed the order).
    pub fn relocate(&mut self, cursor: Option<String>, anchor: Option<String>) {
        let find =
            |n: Option<String>, e: &[Entry]| n.and_then(|n| e.iter().position(|x| x.name == n));
        if let Some(c) = find(cursor, &self.entries) {
            self.sel.cursor = Some(c);
        }
        if let Some(a) = find(anchor, &self.entries) {
            self.sel.anchor = Some(a);
        }
    }

    /// The selection in listing order; the cursor row when nothing is
    /// selected.
    pub fn targets(&self) -> Vec<Entry> {
        let sel: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| self.sel.contains(&e.name))
            .cloned()
            .collect();
        if !sel.is_empty() {
            return sel;
        }
        self.sel
            .cursor
            .and_then(|c| self.entries.get(c))
            .cloned()
            .into_iter()
            .collect()
    }

    pub fn current(&self) -> Option<&Entry> {
        self.sel.cursor.and_then(|c| self.entries.get(c))
    }

    pub fn selected_bytes(&self) -> (usize, u64) {
        self.entries
            .iter()
            .filter(|e| self.sel.contains(&e.name))
            .fold((0, 0), |(n, b), e| {
                (n + 1, b + if e.is_dir() { 0 } else { e.size })
            })
    }
}

/// What is being dragged: rows of one pane. Resolved to the selection at
/// drop time (dragging a selected row drags the whole selection).
#[derive(Clone)]
pub struct DragRows {
    pub view: EntityId,
    pub side: Side,
    pub name: String,
    pub label: String,
}

/// The floating label under the pointer while dragging.
pub struct DragPreview(pub String);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .text_size(px(13.0))
            .shadow_md()
            .child(self.0.clone())
    }
}

impl FilesView {
    pub(super) fn render_pane(
        &mut self,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let entity_id = cx.entity().entity_id();
        let active = self.active == side;
        let elevated = self.elevated && side == Side::Remote;
        let pane = self.pane_mut(side);
        if std::mem::take(&mut pane.path_dirty) {
            let cwd = pane.cwd.clone();
            pane.path_input
                .update(cx, |s, cx| s.set_value(cwd, window, cx));
        }
        if active {
            self.menu_anchor.clear();
        }
        let pane = self.pane(side);
        let count = pane.entries.len();
        let (nsel, sel_bytes) = pane.selected_bytes();
        let title = match side {
            Side::Local => t!("files-this-computer").to_string(),
            Side::Remote => format!("{} — {}", self.profile.name, pane.fs.protocol()),
        };
        let header_cell = |key: SortKey, label: SharedString, w: Option<f32>, right: bool| {
            let arrow = if pane.sort == key {
                if pane.ascending { " ▲" } else { " ▼" }
            } else {
                ""
            };
            div()
                .id(SharedString::from(format!("{side:?}-h-{key:?}")))
                .when_some(w, |d, w| d.w(px(w)).flex_none())
                .when(w.is_none(), |d| d.flex_1())
                .when(right, |d| d.text_right())
                .cursor_pointer()
                .hover(|s| s.text_color(theme.foreground))
                .child(format!("{label}{arrow}"))
                .on_click(cx.listener(move |v, _, _, cx| {
                    let p = v.pane_mut(side);
                    if p.sort == key {
                        p.ascending = !p.ascending;
                    } else {
                        p.sort = key;
                        p.ascending = true;
                    }
                    p.resort();
                    cx.notify();
                }))
        };
        let status: SharedString = if pane.loading {
            t!("loading")
        } else if nsel > 0 {
            t!(
                "files-selected-of",
                selected = nsel,
                count = count,
                size = human_size(sel_bytes)
            )
        } else {
            t!("files-item-count", count = count)
        };

        let list = uniform_list(
            SharedString::from(format!("entries-{side:?}")),
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|ix| this.render_row(side, ix, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&pane.scroll)
        .flex_1();

        div()
            .flex_1()
            .min_w(px(240.0))
            .h_full()
            .flex()
            .flex_col()
            .when(side == Side::Local, |d| {
                d.border_r_1().border_color(theme.border)
            })
            // Title: which side, protocol, elevation.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .h(px(26.0))
                    .text_size(px(12.0))
                    .when(active, |d| d.bg(theme.list_active.opacity(0.35)))
                    .text_color(if active {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(match side {
                        Side::Local => IconName::HardDrive,
                        Side::Remote => IconName::Globe,
                    })
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(title.clone()),
                    )
                    .when(elevated, |d| {
                        d.child(
                            div()
                                .px(px(6.0))
                                .rounded(px(4.0))
                                .bg(theme.danger)
                                .text_color(theme.danger_foreground)
                                .child(format!(
                                    "{}{}",
                                    t!("files-root-badge"),
                                    if self.profile.sudo.user.trim().is_empty() {
                                        String::new()
                                    } else {
                                        format!(" ({})", self.profile.sudo.user.trim())
                                    }
                                )),
                        )
                    }),
            )
            // Path bar.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(6.0))
                    .py(px(4.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Button::new(SharedString::from(format!("{side:?}-up")))
                            .xsmall()
                            .ghost()
                            .icon(IconName::ArrowUp)
                            .tooltip(t!("files-up-tooltip"))
                            .accessibility_label(t!("files-up-tooltip"))
                            .on_click(cx.listener(move |v, _, _, cx| v.up(side, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{side:?}-refresh")))
                            .xsmall()
                            .ghost()
                            .icon(IconName::RefreshCw)
                            .tooltip(t!("files-refresh-tooltip"))
                            .accessibility_label(t!("files-refresh-tooltip"))
                            .on_click(cx.listener(move |v, _, _, cx| v.refresh(side, cx))),
                    )
                    .child(
                        div().flex_1().child(
                            Input::new(&pane.path_input)
                                .xsmall()
                                .aria_label(t!("files-path-label", side = title.clone())),
                        ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{side:?}-mkdir")))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Plus)
                            .tooltip(t!("files-mkdir-tooltip"))
                            .accessibility_label(t!("files-mkdir-tooltip"))
                            .on_click(cx.listener(move |v, _, w, cx| v.new_folder(side, w, cx))),
                    ),
            )
            .when_some(pane.error.clone(), |d, err| {
                d.child(
                    div()
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(px(12.0))
                        .text_color(theme.danger)
                        .child(err),
                )
            })
            // Column headers (click to sort).
            .child(
                div()
                    .flex()
                    .px(px(8.0))
                    .gap(px(8.0))
                    .h(px(22.0))
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().w(px(18.0)).flex_none())
                    .child(header_cell(
                        SortKey::Name,
                        t!("files-col-name"),
                        None,
                        false,
                    ))
                    .child(header_cell(
                        SortKey::Size,
                        t!("files-col-size"),
                        Some(76.0),
                        true,
                    ))
                    .child(header_cell(
                        SortKey::Modified,
                        t!("files-col-modified"),
                        Some(124.0),
                        false,
                    ))
                    .child(header_cell(
                        SortKey::Permissions,
                        t!("files-col-mode"),
                        Some(80.0),
                        false,
                    )),
            )
            // The list: focus target for keys, drop target for rows and OS files.
            .child(
                div()
                    .id(SharedString::from(format!("{side:?}-list")))
                    .role(Role::Grid)
                    .aria_label(t!(
                        "files-list-label",
                        side = title.clone(),
                        dir = pane.cwd.clone()
                    ))
                    .aria_description(t!("files-list-keys"))
                    .aria_row_count(count)
                    .aria_column_count(4)
                    .flex_1()
                    .flex()
                    .flex_col()
                    .track_focus(&pane.focus)
                    .key_context("FilesPane")
                    .on_action(cx.listener(move |v, _: &super::SwitchPane, window, cx| {
                        if v.show_local {
                            v.activate(side.other(), window, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |v, _, window, cx| v.activate(side, window, cx)),
                    )
                    .drag_over::<DragRows>(move |style, d, _, cx| {
                        if d.view == entity_id && d.side != side {
                            style.bg(cx.theme().drop_target)
                        } else {
                            style
                        }
                    })
                    .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(move |v, d: &DragRows, window, cx| {
                        if d.view == entity_id && d.side != side {
                            let dir = v.pane(side).cwd.clone();
                            v.drop_rows(d.clone(), side, dir, window, cx);
                        }
                    }))
                    .on_drop(cx.listener(move |v, paths: &ExternalPaths, window, cx| {
                        let dir = v.pane(side).cwd.clone();
                        v.drop_external(paths.paths().to_vec(), side, dir, window, cx);
                    }))
                    .child(list),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(3.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(crate::a11y::text(status)),
            )
            .into_any_element()
    }

    pub(super) fn render_row(&self, side: Side, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let pane = self.pane(side);
        let Some(e) = pane.entries.get(ix) else {
            return div().into_any_element();
        };
        let selected = pane.sel.contains(&e.name);
        let is_cursor = pane.sel.cursor == Some(ix) && self.active == side;
        let theme = cx.theme();
        let entity_id = cx.entity().entity_id();
        let (icon, size) = match e.kind {
            EntryKind::Dir => (IconName::Folder, String::new()),
            EntryKind::Symlink => (IconName::ExternalLink, String::new()),
            _ => (IconName::File, human_size(e.size)),
        };
        let is_dir = e.is_dir() || e.kind == EntryKind::Symlink;
        let drag = DragRows {
            view: entity_id,
            side,
            name: e.name.clone(),
            label: if selected && pane.sel.names.len() > 1 {
                t!("files-selected", count = pane.sel.names.len()).to_string()
            } else {
                e.name.clone()
            },
        };
        let kind_text = match e.kind {
            EntryKind::Dir => t!("props-kind-folder"),
            EntryKind::Symlink => t!("files-kind-link"),
            EntryKind::Other => t!("props-kind-special"),
            EntryKind::File => size.clone().into(),
        };
        let modified = e.modified.map(local_time).unwrap_or_default();
        let spoken = format!("{}, {kind_text}, {modified}", e.name);
        let cell = |id: &'static str, label: SharedString| {
            div().id(id).role(Role::GridCell).aria_label(label)
        };
        let row = div()
            .id(SharedString::from(format!("{side:?}-row-{ix}")))
            .role(Role::Row)
            .aria_label(spoken)
            .aria_selected(selected)
            .aria_row_index(ix + 1)
            .when(pane.sel.cursor == Some(ix), |d| d.aria_active_descendant())
            .when(is_cursor, |d| d.relative().child(self.menu_anchor.track()))
            .w_full()
            .flex()
            .items_center()
            .h(px(24.0))
            .px(px(8.0))
            .gap(px(8.0))
            .text_size(px(13.0))
            .when(selected, |d| d.bg(theme.list_active))
            .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
            .when(is_cursor, |d| d.border_1().border_color(theme.ring))
            .when(!is_cursor, |d| {
                d.border_1().border_color(transparent_black())
            })
            .on_click(cx.listener(move |v, ev: &ClickEvent, window, cx| {
                v.activate(side, window, cx);
                let m = ev.modifiers();
                let toggle = m.secondary(cx) || m.control;
                let pane = v.pane_mut(side);
                let names: Vec<String> = pane.entries.iter().map(|e| e.name.clone()).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                pane.sel.click(ix, &refs, toggle, m.shift);
                if ev.click_count() >= 2 && !m.shift && !toggle {
                    v.open_entry(side, ix, cx);
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |v, _, window, cx| {
                    v.activate(side, window, cx);
                    let pane = v.pane_mut(side);
                    let names: Vec<String> = pane.entries.iter().map(|e| e.name.clone()).collect();
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    pane.sel.context(ix, &refs);
                    cx.notify();
                }),
            )
            .on_drag(drag, |d, _, _, cx| cx.new(|_| DragPreview(d.label.clone())))
            // Dropping on a folder row copies (other pane) or moves (same
            // pane) into that folder.
            .when(e.is_dir(), |d| {
                let target = e.path.clone();
                d.drag_over::<DragRows>(move |style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(move |v, d: &DragRows, window, cx| {
                        if d.view == entity_id {
                            v.drop_rows(d.clone(), side, target.clone(), window, cx);
                        }
                    }))
            })
            .child(div().w(px(18.0)).flex_none().child(icon))
            .child(
                cell("name", e.name.clone().into())
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(e.name.clone()),
            )
            .child(
                cell("size", kind_text)
                    .w(px(76.0))
                    .flex_none()
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(size),
            )
            .child(
                cell("modified", modified.clone().into())
                    .w(px(124.0))
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(modified),
            )
            .child({
                let mode = e.permissions.map(mode_string).unwrap_or_default();
                cell("mode", mode.clone().into())
                    .w(px(80.0))
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .font_family(crate::terminal::monospace_family())
                    .child(mode)
            });
        // Local rows can be dragged out to the OS file manager where the
        // platform supports it (macOS, Wayland).
        let row = if side == Side::Local {
            let view = cx.entity().downgrade();
            row.external_drag_payload::<DragRows>(move |d, _, cx| {
                let v = view.upgrade()?;
                let paths = v.read(cx).drag_paths(d);
                (!paths.is_empty()).then(|| ExternalDragPayload::Files(FileDragPaths::new(paths)))
            })
        } else {
            row
        };
        row.context_menu(Self::row_menu(cx.entity().downgrade(), side, is_dir))
            .into_any_element()
    }

    /// Local paths for a drag leaving the window.
    fn drag_paths(&self, d: &DragRows) -> Vec<(std::path::PathBuf, bool)> {
        if d.side != Side::Local {
            return Vec::new();
        }
        self.dragged(d)
            .into_iter()
            .map(|e| (std::path::PathBuf::from(&e.path), e.is_dir()))
            .collect()
    }

    /// The rows a drag stands for: the selection if the dragged row is in
    /// it, else just that row.
    pub(super) fn dragged(&self, d: &DragRows) -> Vec<Entry> {
        let pane = self.pane(d.side);
        if pane.sel.contains(&d.name) {
            pane.entries
                .iter()
                .filter(|e| pane.sel.contains(&e.name))
                .cloned()
                .collect()
        } else {
            pane.entries
                .iter()
                .filter(|e| e.name == d.name)
                .cloned()
                .collect()
        }
    }

    fn row_menu(
        view: WeakEntity<Self>,
        side: Side,
        is_dir: bool,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, cx| {
            let act = |f: fn(&mut Self, Side, &mut Window, &mut Context<Self>)| {
                let view = view.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = view.update(cx, |v, cx| f(v, side, window, cx));
                }
            };
            let two_panes = view.upgrade().is_some_and(|v| v.read(cx).show_local);
            let other = match side {
                Side::Local => t!("files-upload"),
                Side::Remote if two_panes => t!("files-download"),
                Side::Remote => t!("files-download-pick"),
            };
            let key = |label: SharedString, keys: &str| format!("{label}  ({keys})");
            let mut menu = menu;
            if is_dir {
                menu = menu.item(PopupMenuItem::new(t!("files-menu-open")).on_click(act(
                    |v, s, _, cx| {
                        if let Some(ix) = v.pane(s).sel.cursor {
                            v.open_entry(s, ix, cx);
                        }
                    },
                )));
            } else {
                menu = menu
                    .item(
                        PopupMenuItem::new(key(t!("files-menu-edit"), "F4"))
                            .on_click(act(|v, s, _, cx| v.edit_selection(s, cx))),
                    )
                    .item(PopupMenuItem::new(t!("files-menu-open")).on_click(act(
                        |v, s, _, cx| {
                            if let Some(ix) = v.pane(s).sel.cursor {
                                v.open_entry(s, ix, cx);
                            }
                        },
                    )));
            }
            if side == Side::Remote || two_panes {
                menu = menu
                    .item(
                        PopupMenuItem::new(key(other, "F5"))
                            .on_click(act(|v, s, w, cx| v.copy_selection(s, false, w, cx))),
                    )
                    .when(two_panes, |m| {
                        m.item(
                            PopupMenuItem::new(key(t!("files-menu-move"), "F6"))
                                .on_click(act(|v, s, w, cx| v.copy_selection(s, true, w, cx))),
                        )
                    });
            }
            menu.separator()
                .item(
                    PopupMenuItem::new(key(t!("files-menu-rename"), "F2"))
                        .on_click(act(|v, s, w, cx| v.rename_dialog(s, w, cx))),
                )
                .item(
                    PopupMenuItem::new(key(t!("menu-delete"), "Del"))
                        .on_click(act(|v, s, w, cx| v.confirm_delete(s, w, cx))),
                )
                .separator()
                .item(
                    PopupMenuItem::new(key(t!("files-menu-new-folder"), "F7"))
                        .on_click(act(|v, s, w, cx| v.new_folder(s, w, cx))),
                )
                .item(
                    PopupMenuItem::new(t!("files-menu-new-symlink"))
                        .on_click(act(|v, s, w, cx| v.new_symlink(s, w, cx))),
                )
                .item(PopupMenuItem::new(t!("files-menu-copy-path")).on_click(act(
                    |v, s, _, cx| {
                        let paths: Vec<String> =
                            v.pane(s).targets().into_iter().map(|e| e.path).collect();
                        if !paths.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                        }
                    },
                )))
                .item(
                    PopupMenuItem::new(key(t!("files-menu-refresh"), "Ctrl+R"))
                        .on_click(act(|v, s, _, cx| v.refresh(s, cx))),
                )
                .separator()
                .item(
                    PopupMenuItem::new(key(t!("files-menu-properties"), "Alt+Enter"))
                        .on_click(act(|v, s, w, cx| v.properties(s, w, cx))),
                )
        }
    }
}
