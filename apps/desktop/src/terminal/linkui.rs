//! Clickable output in the terminal view: finding the link under the
//! pointer, underlining it, opening it, and the context-menu entries.
//!
//! Detection runs only for the logical line under the pointer when the
//! pointer moves to another cell, so it costs nothing while the mouse is
//! still. With mouse reporting on (htop, vim) links need Shift, like
//! selection does.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use gpui_kit::*;

use super::ext::Hover;
use super::links::{self, LinkKind};
use super::{TermExt, TerminalEvent, TerminalView};
use crate::settings::Settings;

/// Ctrl (⌘ on macOS) is the "follow link" modifier.
pub fn follow_modifier(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    }
}

impl TerminalView {
    /// Text of the logical line (wrapped rows joined) that contains the
    /// grid line `line`, with the grid position of every character.
    fn logical_line(&self, line: i32) -> (String, Vec<(i32, usize)>) {
        let term = self.shared.term.lock();
        let grid = term.grid();
        let cols = term.columns();
        if cols == 0 {
            return (String::new(), Vec::new());
        }
        let top = -(grid.history_size() as i32);
        let bottom = term.screen_lines() as i32 - 1;
        let line = line.clamp(top, bottom);
        let wraps = |l: i32| {
            grid[Line(l)][Column(cols - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        };
        let mut first = line;
        let mut n = 0;
        while first > top && wraps(first - 1) && n < 6 {
            first -= 1;
            n += 1;
        }
        let mut last = line;
        n = 0;
        while last < bottom && wraps(last) && n < 6 {
            last += 1;
            n += 1;
        }
        let mut text = String::new();
        let mut map = Vec::new();
        for l in first..=last {
            for c in 0..cols {
                let cell = &grid[Line(l)][Column(c)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                text.push(if cell.c == '\0' || cell.c == '\t' {
                    ' '
                } else {
                    cell.c
                });
                map.push((l, c));
            }
        }
        (text, map)
    }

    /// The link at viewport cell (`col`, `row`), if there is one.
    fn link_at_cell(&self, col: usize, row: usize) -> Option<Hover> {
        let offset = self.shared.term.lock().grid().display_offset() as i32;
        let line = row as i32 - offset;
        let (text, map) = self.logical_line(line);
        let idx = map
            .iter()
            .position(|&(l, c)| l == line && c == col)
            .or_else(|| {
                // The right half of a wide character.
                map.iter().position(|&(l, c)| l == line && c + 1 == col)
            })?;
        let found = links::at(&text, idx)?;
        let mut segments: Vec<(i32, std::ops::Range<usize>)> = Vec::new();
        for &(l, c) in &map[found.range.clone()] {
            match segments.last_mut() {
                Some((sl, r)) if *sl == l => r.end = c + 1,
                _ => segments.push((l, c..c + 1)),
            }
        }
        Some(Hover { found, segments })
    }

    /// The pointer moved (or a modifier changed): keep the hover current.
    pub(super) fn update_hover(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let modifier = follow_modifier(&ev.modifiers);
        let mut changed = self.x.link_mod != modifier;
        self.x.link_mod = modifier;
        let allowed = Settings::get(cx).term.clickable_output
            && !self.selecting
            && self.reporting(ev.modifiers.shift, cx).is_none();
        let cell = if allowed {
            self.cell_at(ev.position).map(|(c, r, _)| (c, r))
        } else {
            None
        };
        if cell != self.x.hover_cell || !allowed {
            self.x.hover_cell = cell;
            let hover = cell.and_then(|(c, r)| self.link_at_cell(c, r));
            let same = match (&hover, &self.x.hover) {
                (Some(a), Some(b)) => a.found == b.found && a.segments == b.segments,
                (None, None) => true,
                _ => false,
            };
            if !same {
                self.x.hover = hover;
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn clear_hover(&mut self, cx: &mut Context<Self>) {
        if self.x.hover.is_some() || self.x.hover_cell.is_some() {
            self.x.hover = None;
            self.x.hover_cell = None;
            cx.notify();
        }
    }

    /// A modifier key went down or up with the pointer still.
    pub(super) fn modifiers_changed(&mut self, m: &Modifiers, cx: &mut Context<Self>) {
        let modifier = follow_modifier(m);
        if self.x.link_mod != modifier {
            self.x.link_mod = modifier;
            if self.x.hover.is_some() {
                cx.notify();
            }
        }
    }

    /// The link is shown as active (underline, pointing hand).
    pub(crate) fn link_active(&self) -> bool {
        self.x
            .hover
            .as_ref()
            .is_some_and(|h| h.found.kind == LinkKind::Url || self.x.link_mod)
    }

    /// Absolute remote path for a path link (`~` and `./` resolved).
    pub(super) fn resolve_link_path(&self, raw: &str) -> Option<String> {
        links::resolve_path(raw, self.cwd.as_deref(), Some(&self.x.info.user))
    }

    /// Ctrl/⌘+click on a link. True when it was one.
    pub(super) fn follow_link(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(h) = self.x.hover.clone() else {
            return false;
        };
        match h.found.kind {
            LinkKind::Url => cx.open_url(&links::url_to_open(&h.found.text)),
            LinkKind::Path => {
                let Some(path) = self.resolve_link_path(&h.found.text) else {
                    return false;
                };
                cx.emit(TerminalEvent::Ext(TermExt::OpenPath {
                    path,
                    download: false,
                }));
            }
            // Addresses and host names are only offered in the menu: one
            // careless click should not start a connection.
            LinkKind::Ip | LinkKind::Host => return false,
        }
        true
    }
}

/// Context-menu entries for the link under the pointer, added on top.
pub(super) fn link_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    found: links::Found,
    path: Option<String>,
    is_ssh: bool,
    view: &WeakEntity<TerminalView>,
) -> gpui_kit::component::menu::PopupMenu {
    use gpui_kit::component::menu::PopupMenuItem;
    let text = found.text.clone();
    let copy = |label: SharedString, value: String| {
        PopupMenuItem::new(label).on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
        })
    };
    let ask = |label: SharedString, ev: TermExt| {
        let view = view.clone();
        PopupMenuItem::new(label).on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let ev = ev.clone();
            let _ = view.update(cx, |_, cx| cx.emit(TerminalEvent::Ext(ev)));
        })
    };
    match found.kind {
        LinkKind::Url => {
            let url = links::url_to_open(&text);
            menu.item(
                PopupMenuItem::new(t!("tx-link-open")).on_click(
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.open_url(&url),
                ),
            )
            .item(copy(t!("tx-link-copy"), text))
            .separator()
        }
        LinkKind::Path => {
            let mut menu = menu;
            if is_ssh && let Some(p) = path.clone() {
                menu = menu
                    .item(ask(
                        t!("tx-link-reveal"),
                        TermExt::OpenPath {
                            path: p.clone(),
                            download: false,
                        },
                    ))
                    .item(ask(
                        t!("tx-link-download"),
                        TermExt::OpenPath {
                            path: p,
                            download: true,
                        },
                    ));
            }
            menu.item(copy(t!("tx-link-copy-path"), path.unwrap_or(text)))
                .separator()
        }
        LinkKind::Ip | LinkKind::Host => {
            let target = links::connect_text(&found);
            menu.item(ask(
                t!("tx-link-connect", target = target.clone()),
                TermExt::Connect(target),
            ))
            .item(copy(t!("menu-copy-address"), text))
            .separator()
        }
    }
}
