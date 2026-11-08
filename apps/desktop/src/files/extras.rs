//! The file browser's extras: a clickable path bar (and Ctrl+L to type a
//! path), bookmarks, type-to-filter, and the quick preview on Space.

use std::sync::Arc;

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, ProfileStore as _};
use hostwick_sftp::{Entry, EntryKind, RemoteFs};

use super::crumbs::{self, Crumb};
use super::preview::{self, ImageKind, Kind};
use super::{FilesView, Side};
use crate::util::{human_size, local_time, mode_string};
use crate::zoom::px;

/// What a file browser tells the app.
pub enum FilesEvent {
    /// A host's bookmarks changed (saved with the host, so it syncs).
    BookmarksChanged,
}

impl EventEmitter<FilesEvent> for FilesView {}

/// Most bookmarks kept per host.
const MAX_MARKS: usize = 60;

// ---- bookmarks ------------------------------------------------------------------

fn local_marks_key() -> &'static str {
    "files.bookmarks.local"
}

/// Where an unsaved (quick-connect) host keeps its bookmarks.
fn unsaved_marks_key(p: &HostProfile) -> String {
    format!(
        "files.bookmarks.host.{}@{}:{}",
        p.user_or_local(),
        p.host,
        p.port
    )
    .to_lowercase()
}

fn read_list(backend: &crate::backend::Backend, key: &str) -> Vec<String> {
    backend
        .store
        .setting(key)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
        .unwrap_or_default()
}

/// The bookmarks of a side when a browser opens.
pub(super) fn load_marks(
    backend: &crate::backend::Backend,
    profile: &HostProfile,
    side: Side,
) -> Vec<String> {
    match side {
        Side::Local => read_list(backend, local_marks_key()),
        Side::Remote => match backend.store.get(profile.id) {
            Ok(saved) => saved.bookmarks,
            Err(_) => read_list(backend, &unsaved_marks_key(profile)),
        },
    }
}

/// Add or remove `path` (the star button); the list stays short.
pub(super) fn toggle_in(list: &mut Vec<String>, path: &str) {
    if let Some(i) = list.iter().position(|p| p == path) {
        list.remove(i);
    } else if !path.is_empty() {
        list.push(path.to_owned());
        let extra = list.len().saturating_sub(MAX_MARKS);
        list.drain(..extra);
    }
}

impl FilesView {
    pub(super) fn marks(&self, side: Side) -> &Vec<String> {
        match side {
            Side::Local => &self.marks_local,
            Side::Remote => &self.marks_remote,
        }
    }

    fn marks_mut(&mut self, side: Side) -> &mut Vec<String> {
        match side {
            Side::Local => &mut self.marks_local,
            Side::Remote => &mut self.marks_remote,
        }
    }

    /// Star: bookmark the folder shown, or remove its bookmark.
    pub(super) fn toggle_bookmark(&mut self, side: Side, cx: &mut Context<Self>) {
        let cwd = self.pane(side).cwd.clone();
        toggle_in(self.marks_mut(side), &cwd);
        self.save_marks(side, cx);
    }

    pub(super) fn remove_bookmark(&mut self, side: Side, path: &str, cx: &mut Context<Self>) {
        self.marks_mut(side).retain(|p| p != path);
        self.save_marks(side, cx);
    }

    fn save_marks(&mut self, side: Side, cx: &mut Context<Self>) {
        let list = self.marks(side).clone();
        match side {
            Side::Local => self.save_list(local_marks_key(), &list),
            Side::Remote => {
                // A saved host keeps them in its profile (and settings sync
                // carries them); an unsaved one only on this computer.
                let saved = self.backend.store.get(self.profile.id).ok();
                match saved {
                    Some(mut p) => {
                        p.bookmarks = list.clone();
                        if let Err(e) = self.backend.store.upsert(&p) {
                            tracing::warn!(error = %e, "could not save bookmarks");
                        }
                        self.profile.bookmarks = list;
                        cx.emit(FilesEvent::BookmarksChanged);
                    }
                    None => self.save_list(&unsaved_marks_key(&self.profile), &list),
                }
            }
        }
        cx.notify();
    }

    fn save_list(&self, key: &str, list: &[String]) {
        let json = serde_json::to_string(list).unwrap_or_default();
        if let Err(e) = self.backend.store.set_setting(key, &json) {
            tracing::warn!(error = %e, "could not save bookmarks");
        }
    }

    // ---- path bar -------------------------------------------------------------

    /// Click on the empty part of the path bar, or Ctrl+L: edit the path as
    /// text.
    pub(super) fn edit_path(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(side, window, cx);
        let pane = self.pane_mut(side);
        pane.editing_path = true;
        let cwd = pane.cwd.clone();
        let input = pane.path_input.clone();
        input.update(cx, |s, cx| {
            s.set_value(cwd, window, cx);
            s.focus(window, cx);
            // Typing replaces the old path.
            s.select_all(window, cx);
        });
        cx.notify();
    }

    /// Back to crumbs, with the keyboard back in the list.
    pub(super) fn stop_editing_path(
        &mut self,
        side: Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane_mut(side);
        if std::mem::take(&mut pane.editing_path) {
            pane.focus.clone().focus(window, cx);
            cx.notify();
        }
    }

    /// The row of buttons and crumbs under a pane's title.
    pub(super) fn render_path_bar(
        &self,
        side: Side,
        title: SharedString,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let pane = self.pane(side);
        let marks = self.marks(side).clone();
        let marked = marks.contains(&pane.cwd);
        let panes = if self.show_local { 2.0 } else { 1.0 };
        let width = f32::from(window.viewport_size().width);
        // Room for crumbs, in characters, from the pane's width.
        let chars = (((width - 320.0) / panes - 190.0) / 7.5).max(8.0) as usize;
        let all = crumbs::crumbs(&pane.cwd);
        let (hidden, shown) = crumbs::split_overflow(&all, chars);
        let hidden: Vec<Crumb> = hidden.to_vec();
        let mut crumb_row = div()
            .flex()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .text_size(px(12.0));
        if !hidden.is_empty() {
            let view = cx.entity().downgrade();
            let items = hidden.clone();
            crumb_row = crumb_row
                .child(
                    Button::new(SharedString::from(format!("{side:?}-crumb-more")))
                        .xsmall()
                        .ghost()
                        .label("…")
                        .tooltip(t!("files-crumbs-more"))
                        .accessibility_label(t!("files-crumbs-more"))
                        .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                            items.iter().fold(menu, |menu, c| {
                                let (view, path) = (view.clone(), c.path.clone());
                                menu.item(PopupMenuItem::new(c.path.clone()).on_click(
                                    move |_, _, cx| {
                                        let _ = view
                                            .update(cx, |v, cx| v.navigate(side, path.clone(), cx));
                                    },
                                ))
                            })
                        }),
                )
                .child(div().text_color(theme.muted_foreground).child("›"));
        }
        let last = shown.len().saturating_sub(1);
        for (i, c) in shown.iter().enumerate() {
            crumb_row = crumb_row
                .child(
                    div()
                        .id(SharedString::from(format!("{side:?}-crumb-{i}")))
                        .role(Role::Button)
                        .aria_label(t!("files-crumb-go", folder = c.path.clone()))
                        .flex_none()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .when(i == last, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .on_click(cx.listener({
                            let path = c.path.clone();
                            move |v, _, _, cx| {
                                cx.stop_propagation();
                                v.navigate(side, path.clone(), cx);
                            }
                        }))
                        .child(c.label.clone()),
                )
                .when(i != last, |d| {
                    d.child(div().text_color(theme.muted_foreground).child("›"))
                });
        }

        let editing = pane.editing_path;
        let view = cx.entity().downgrade();
        let marks_for_menu = marks.clone();
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
            .child(if editing {
                div()
                    .flex_1()
                    .min_w_0()
                    .capture_key_down(cx.listener(move |v, ev: &KeyDownEvent, window, cx| {
                        if ev.keystroke.key == "escape" {
                            v.stop_editing_path(side, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        Input::new(&pane.path_input)
                            .xsmall()
                            .aria_label(t!("files-path-label", side = title.clone())),
                    )
                    .into_any_element()
            } else {
                div()
                    .id(SharedString::from(format!("{side:?}-path")))
                    .role(Role::Navigation)
                    .aria_label(t!("files-path-label", side = title.clone()))
                    .flex_1()
                    .min_w_0()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(theme.border)
                    .cursor_text()
                    .tooltip({
                        let tip = t!("files-path-click-to-edit");
                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |v, _, window, cx| v.edit_path(side, window, cx)))
                    .child(crumb_row)
                    .child(div().flex_1().h_full())
                    .into_any_element()
            })
            .child(
                Button::new(SharedString::from(format!("{side:?}-star")))
                    .xsmall()
                    .ghost()
                    .icon(if marked {
                        IconName::StarFill
                    } else {
                        IconName::Star
                    })
                    .tooltip(if marked {
                        t!("files-bookmark-remove")
                    } else {
                        t!("files-bookmark-add")
                    })
                    .accessibility_label(if marked {
                        t!("files-bookmark-remove")
                    } else {
                        t!("files-bookmark-add")
                    })
                    .on_click(cx.listener(move |v, _, _, cx| v.toggle_bookmark(side, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("{side:?}-marks")))
                    .xsmall()
                    .ghost()
                    .icon(IconName::ChevronDown)
                    .tooltip(t!("files-bookmarks"))
                    .accessibility_label(t!("files-bookmarks"))
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                        if marks_for_menu.is_empty() {
                            return menu.item(
                                PopupMenuItem::new(t!("files-bookmarks-empty")).disabled(true),
                            );
                        }
                        let go_view = view.clone();
                        let rm_view = view.clone();
                        let to_remove = marks_for_menu.clone();
                        let menu = marks_for_menu.iter().fold(menu, |menu, path| {
                            let (view, path) = (go_view.clone(), path.clone());
                            menu.item(PopupMenuItem::new(path.clone()).on_click(move |_, _, cx| {
                                let _ = view.update(cx, |v, cx| v.navigate(side, path.clone(), cx));
                            }))
                        });
                        menu.separator().submenu(
                            t!("files-bookmark-remove-menu"),
                            window,
                            cx,
                            move |sub, _, _| {
                                to_remove.iter().fold(sub, |sub, path| {
                                    let (view, path) = (rm_view.clone(), path.clone());
                                    sub.item(PopupMenuItem::new(path.clone()).on_click(
                                        move |_, _, cx| {
                                            let _ = view.update(cx, |v, cx| {
                                                v.remove_bookmark(side, &path, cx)
                                            });
                                        },
                                    ))
                                })
                            },
                        )
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("{side:?}-mkdir")))
                    .xsmall()
                    .ghost()
                    .icon(IconName::Plus)
                    .tooltip(t!("files-mkdir-tooltip"))
                    .accessibility_label(t!("files-mkdir-tooltip"))
                    .on_click(cx.listener(move |v, _, w, cx| v.new_folder(side, w, cx))),
            )
            .into_any_element()
    }

    // ---- filter ---------------------------------------------------------------

    /// Show only entries matching `text` (typing in the list, or in the
    /// pane's filter field when `from_input`).
    pub(super) fn set_filter(
        &mut self,
        side: Side,
        text: String,
        from_input: bool,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane_mut(side);
        if pane.filter == text {
            return;
        }
        pane.filter = text;
        pane.filter_dirty = !from_input;
        pane.refilter();
        if let Some(c) = pane.sel.cursor {
            pane.scroll.scroll_to_item(c, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    pub(super) fn focus_filter(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(side, window, cx);
        let input = self.pane(side).filter_input.clone();
        input.update(cx, |s, cx| s.focus(window, cx));
    }

    // ---- preview --------------------------------------------------------------

    /// Space: look at the file under the cursor.
    pub(super) fn open_preview(&mut self, side: Side, cx: &mut Context<Self>) {
        let Some(entry) = self.pane(side).current().cloned() else {
            return;
        };
        self.preview_gen += 1;
        let generation = self.preview_gen;
        self.preview_line = 0;
        if entry.is_dir() {
            self.preview = Some(Preview {
                side,
                entry,
                body: Body::Folder,
                generation,
            });
            self.preview_task = None;
            cx.notify();
            return;
        }
        let fs = self.pane(side).fs.clone();
        let (path, size) = (entry.path.clone(), entry.size);
        self.preview = Some(Preview {
            side,
            entry,
            body: Body::Loading,
            generation,
        });
        let job = self.backend.rt.spawn(load_preview(fs, path, size));
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |v, cx| v.preview_loaded(generation, result, cx));
        }));
        cx.notify();
    }

    fn preview_loaded(
        &mut self,
        generation: u64,
        result: Result<Result<Loaded, hostwick_sftp::FsError>, tokio::task::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.preview.as_mut().filter(|p| p.generation == generation) else {
            return;
        };
        p.body = match result {
            Ok(Ok(Loaded::Text {
                lines,
                more,
                encoding,
                cut,
            })) => Body::Text {
                lines: lines.into_iter().map(SharedString::from).collect(),
                more,
                encoding,
                cut,
            },
            Ok(Ok(Loaded::Image(bytes, kind))) => {
                Body::Image(Arc::new(Image::from_bytes(image_format(kind), bytes)))
            }
            Ok(Ok(Loaded::ImageRefused(size))) => Body::ImageRefused(size),
            Ok(Ok(Loaded::Binary)) => Body::Binary,
            Ok(Ok(Loaded::TooLarge)) => Body::TooLarge,
            Ok(Err(e)) => Body::Error(e.to_string()),
            Err(e) => Body::Error(t!("error-internal", error = e.to_string()).into()),
        };
        cx.notify();
    }

    pub(super) fn close_preview(&mut self, cx: &mut Context<Self>) {
        if self.preview.take().is_some() {
            self.preview_task = None;
            cx.notify();
        }
    }

    /// The cursor moved while the preview is open: show the new file.
    pub(super) fn follow_preview(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.preview else {
            return;
        };
        let side = p.side;
        let same = self
            .pane(side)
            .current()
            .is_some_and(|e| e.path == p.entry.path);
        if !same {
            self.open_preview(side, cx);
        }
    }

    /// Keys while the preview is open. Up/down are left to the list (they
    /// pick the next file); these close it or scroll the text.
    pub(super) fn preview_key(&mut self, key: &str, m: Modifiers, cx: &mut Context<Self>) -> bool {
        if m.control || m.alt || m.platform {
            return false;
        }
        match key {
            "escape" | "space" => {
                self.close_preview(cx);
                true
            }
            "pageup" | "pagedown" | "home" | "end" => {
                let lines = match self.preview.as_ref().map(|p| &p.body) {
                    Some(Body::Text { lines, .. }) => lines.len(),
                    _ => return true,
                };
                let page = 28usize;
                self.preview_line = match key {
                    "pageup" => self.preview_line.saturating_sub(page),
                    "pagedown" => (self.preview_line + page).min(lines.saturating_sub(1)),
                    "home" => 0,
                    _ => lines.saturating_sub(1),
                };
                self.preview_scroll
                    .scroll_to_item(self.preview_line, ScrollStrategy::Top);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    pub(super) fn render_preview(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let p = self.preview.as_ref()?;
        let e = &p.entry;
        let modified = e.modified.map(local_time).unwrap_or_default();
        let mode = e.permissions.map(mode_string).unwrap_or_default();
        let facts = [
            (!e.is_dir()).then(|| human_size(e.size)),
            Some(modified).filter(|m| !m.is_empty()),
            Some(mode).filter(|m| !m.is_empty()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let note = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .px(px(16.0))
                .text_color(theme.muted_foreground)
                .child(crate::a11y::text(text))
        };
        let body = match &p.body {
            Body::Loading => note(t!("loading")).into_any_element(),
            Body::Folder => note(t!("preview-folder")).into_any_element(),
            Body::Binary => note(t!("preview-no-preview"))
                .child(div().text_size(px(12.0)).child(e.path.clone()))
                .into_any_element(),
            Body::TooLarge => note(t!(
                "preview-too-large",
                limit = human_size(preview::IMAGE_LIMIT)
            ))
            .into_any_element(),
            Body::ImageRefused(Some((w, h))) => {
                note(t!("preview-image-too-big", width = *w, height = *h)).into_any_element()
            }
            Body::ImageRefused(None) => note(t!("preview-image-unknown")).into_any_element(),
            Body::Error(err) => note(t!("preview-error", error = err.clone()))
                .text_color(theme.danger)
                .into_any_element(),
            Body::Image(image) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(
                    img(image.clone())
                        .object_fit(ObjectFit::Contain)
                        .max_w_full()
                        .max_h_full(),
                )
                .into_any_element(),
            Body::Text {
                lines,
                more,
                encoding,
                cut,
            } => {
                let shown = lines.clone();
                let count = shown.len();
                let truncated = *cut || *more;
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(if count == 0 {
                        note(t!("preview-empty")).into_any_element()
                    } else {
                        uniform_list(
                            "preview-lines",
                            count,
                            move |range: std::ops::Range<usize>, _, _| {
                                range
                                    .map(|i| {
                                        div()
                                            .h(px(18.0))
                                            .flex()
                                            .gap(px(10.0))
                                            .child(
                                                div()
                                                    .w(px(44.0))
                                                    .flex_none()
                                                    .text_right()
                                                    .opacity(0.5)
                                                    .child((i + 1).to_string()),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .child(shown[i].clone()),
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            },
                        )
                        .track_scroll(&self.preview_scroll)
                        .size_full()
                        .font_family(crate::terminal::monospace_family())
                        .text_size(px(12.5))
                        .into_any_element()
                    }))
                    .child(
                        div()
                            .flex_none()
                            .px(px(10.0))
                            .py(px(3.0))
                            .border_t_1()
                            .border_color(theme.border)
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground)
                            .child(crate::a11y::text(if truncated {
                                t!(
                                    "preview-text-cut",
                                    encoding = *encoding,
                                    limit = human_size(preview::TEXT_LIMIT as u64)
                                )
                            } else {
                                t!("preview-text-info", encoding = *encoding)
                            })),
                    )
                    .into_any_element()
            }
        };
        let name = e.name.clone();
        let icon = match e.kind {
            EntryKind::Dir => IconName::Folder,
            _ => match &p.body {
                Body::Image(_) => IconName::Frame,
                Body::Text { .. } => IconName::FileText,
                _ => IconName::File,
            },
        };
        Some(
            div()
                .id("preview")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.background.opacity(0.55))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|v, _, _, cx| v.close_preview(cx)),
                )
                .child(
                    div()
                        .id("preview-card")
                        .role(Role::Dialog)
                        .aria_label(t!("preview-label", name = name.clone()))
                        .w(relative(0.84))
                        .h(relative(0.84))
                        .flex()
                        .flex_col()
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .overflow_hidden()
                        // Clicks inside do not close it.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .px(px(12.0))
                                .h(px(38.0))
                                .border_b_1()
                                .border_color(theme.border)
                                .child(icon)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.0))
                                        .text_color(theme.muted_foreground)
                                        .child(facts),
                                )
                                .child(
                                    Button::new("preview-close")
                                        .xsmall()
                                        .ghost()
                                        .icon(IconName::Close)
                                        .tooltip(t!("preview-close"))
                                        .accessibility_label(t!("preview-close"))
                                        .on_click(cx.listener(|v, _, _, cx| v.close_preview(cx))),
                                ),
                        )
                        .child(div().flex_1().min_h_0().child(body))
                        .child(
                            div()
                                .flex_none()
                                .px(px(12.0))
                                .py(px(4.0))
                                .border_t_1()
                                .border_color(theme.border)
                                .text_size(px(11.0))
                                .text_color(theme.muted_foreground)
                                .child(crate::a11y::text(t!("preview-hint"))),
                        ),
                )
                .into_any_element(),
        )
    }
}

// ---- loading ------------------------------------------------------------------------

/// What a preview shows.
pub(super) struct Preview {
    side: Side,
    entry: Entry,
    body: Body,
    generation: u64,
}

enum Body {
    Loading,
    Folder,
    Text {
        lines: Vec<SharedString>,
        more: bool,
        encoding: &'static str,
        cut: bool,
    },
    Image(Arc<Image>),
    /// An image whose declared size (or kind of content) is not decoded:
    /// its size when known.
    ImageRefused(Option<(u64, u64)>),
    Binary,
    TooLarge,
    Error(String),
}

/// A file read on the Tokio runtime, ready to show.
enum Loaded {
    Text {
        lines: Vec<String>,
        more: bool,
        encoding: &'static str,
        cut: bool,
    },
    Image(Vec<u8>, ImageKind),
    ImageRefused(Option<(u64, u64)>),
    Binary,
    TooLarge,
}

/// Hand the bytes to the decoder only when the size they declare is
/// reasonable.
fn checked_image(bytes: Vec<u8>, kind: ImageKind) -> Loaded {
    match preview::check_image(&bytes, kind) {
        preview::ImageCheck::Ok => Loaded::Image(bytes, kind),
        preview::ImageCheck::TooBig(w, h) => Loaded::ImageRefused(Some((w, h))),
        preview::ImageCheck::Unknown => Loaded::ImageRefused(None),
    }
}

fn image_format(kind: ImageKind) -> ImageFormat {
    match kind {
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Jpeg => ImageFormat::Jpeg,
        ImageKind::Gif => ImageFormat::Gif,
        ImageKind::Webp => ImageFormat::Webp,
        ImageKind::Svg => ImageFormat::Svg,
    }
}

/// Read the start of a file (any filesystem: local or remote) and decide
/// what it is. Only what is shown is read: 256 KB of text, or the whole
/// of an image up to 20 MB.
async fn load_preview(
    fs: Arc<dyn RemoteFs>,
    path: String,
    size: u64,
) -> Result<Loaded, hostwick_sftp::FsError> {
    let want = if size == 0 {
        preview::TEXT_LIMIT
    } else {
        usize::try_from(size)
            .unwrap_or(usize::MAX)
            .min(preview::TEXT_LIMIT)
    };
    let head = fs.read_range(&path, 0, want).await?;
    Ok(match preview::classify(&head) {
        Kind::Image(kind) => {
            if size > preview::IMAGE_LIMIT {
                Loaded::TooLarge
            } else if (head.len() as u64) >= size {
                checked_image(head, kind)
            } else {
                // The declared size is in the first bytes: look before
                // reading (and decoding) the rest.
                if let preview::ImageCheck::TooBig(w, h) = preview::check_image(&head, kind) {
                    return Ok(Loaded::ImageRefused(Some((w, h))));
                }
                let whole = usize::try_from(size).unwrap_or(0);
                checked_image(fs.read_range(&path, 0, whole).await?, kind)
            }
        }
        Kind::Text => {
            let cut = size > head.len() as u64;
            let decoded = preview::decode_text(&head, cut);
            let (lines, more) = preview::display_lines(&decoded.text);
            Loaded::Text {
                lines,
                more,
                encoding: decoded.encoding.label(),
                cut,
            }
        }
        Kind::Binary => Loaded::Binary,
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_MARKS, toggle_in};

    #[test]
    fn the_star_adds_then_removes() {
        let mut l = Vec::new();
        toggle_in(&mut l, "/var/www");
        toggle_in(&mut l, "/etc");
        assert_eq!(l, ["/var/www", "/etc"]);
        toggle_in(&mut l, "/var/www");
        assert_eq!(l, ["/etc"]);
        // Nothing to bookmark before the first listing.
        toggle_in(&mut l, "");
        assert_eq!(l, ["/etc"]);
    }

    #[test]
    fn the_list_stays_short() {
        let mut l = Vec::new();
        for i in 0..MAX_MARKS + 5 {
            toggle_in(&mut l, &format!("/d{i}"));
        }
        assert_eq!(l.len(), MAX_MARKS);
        // The newest are kept.
        assert_eq!(l.last().map(String::as_str), Some("/d64"));
        assert_eq!(l.first().map(String::as_str), Some("/d5"));
    }
}
