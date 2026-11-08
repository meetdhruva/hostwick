//! The sidebar: quick connect with suggestions, Favourites and Recent
//! sections, the folder tree of saved hosts with multi-selection, inline
//! rename, drag and drop onto folders, and the keys of the host list.

use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use crate::zoom::px;
use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, Input, MoveDown, MoveUp};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, ProfileId, Protocol};
use serde::{Deserialize, Serialize};

use super::palette::{Choice, Mods, address, secondary};
use super::{HostwickApp, TabKind, logo};
use crate::fuzzy::{Hit, Prepared, Query, byte_ranges, match_fields};
use crate::hostops::{self, Recent, TreeRow};
use crate::settings::Settings;
use crate::util::{keys, protocol_name};

/// Height of every row of the host list.
const ROW: f32 = 28.0;
/// Recent connections shown in the sidebar.
const RECENT_SHOWN: usize = 8;
/// Quick-connect suggestions shown.
const QUICK_SHOWN: usize = 8;

/// The sidebar's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sec {
    Favourites,
    Recent,
    Hosts,
}

impl Sec {
    fn label(self) -> SharedString {
        match self {
            Sec::Favourites => t!("sidebar-section-favourites"),
            Sec::Recent => t!("sidebar-section-recent"),
            Sec::Hosts => t!("sidebar-section-hosts"),
        }
    }
}

/// Which section a host row is in (a host can be in all three).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Place {
    Fav,
    Recent,
    Tree,
}

/// A row's identity, for the keyboard cursor and inline rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowId {
    Section(Sec),
    Folder(String),
    Host(ProfileId, Place),
    Quick(String),
}

pub(crate) enum SidebarRow {
    Section {
        sec: Sec,
        count: usize,
        collapsed: bool,
    },
    Folder {
        path: String,
        depth: usize,
        count: usize,
        collapsed: bool,
    },
    Profile {
        ix: usize,
        depth: usize,
        place: Place,
    },
    /// A recent quick connection (not a saved host).
    Quick { text: String },
}

impl SidebarRow {
    fn id(&self, profiles: &[HostProfile]) -> Option<RowId> {
        Some(match self {
            SidebarRow::Section { sec, .. } => RowId::Section(*sec),
            SidebarRow::Folder { path, .. } => RowId::Folder(path.clone()),
            SidebarRow::Profile { ix, place, .. } => RowId::Host(profiles.get(*ix)?.id, *place),
            SidebarRow::Quick { text } => RowId::Quick(text.clone()),
        })
    }
}

/// Saved hosts being dragged onto a folder.
#[derive(Clone)]
pub(crate) struct DraggedHosts {
    ids: Vec<ProfileId>,
    label: SharedString,
}

struct DragGhost(SharedString);

impl Render for DragGhost {
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

/// Collapsed folders and sections, kept per computer.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct SidebarState {
    #[serde(default)]
    pub collapsed: Vec<String>,
    #[serde(default)]
    pub sections: Vec<Sec>,
}

pub(crate) const SIDEBAR_KEY: &str = "desktop.sidebar";

/// A quick-connect suggestion.
#[derive(Clone)]
pub(crate) struct QuickSug {
    choice: Choice,
    label: SharedString,
    detail: SharedString,
    hit: Hit,
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
    // ---- rows -----------------------------------------------------------------

    pub(super) fn sidebar_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let needle = self.search.read(cx).value().to_lowercase();
        let explicit = &Settings::get(cx).folders;
        if !needle.is_empty() {
            // Matches only, in their folders (all open).
            let idx: Vec<usize> = self
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    let hay = format!(
                        "{} {} {} {} {}",
                        p.path(),
                        p.host,
                        p.user.as_deref().unwrap_or(""),
                        p.protocol.label(),
                        protocol_name(&p.protocol)
                    )
                    .to_lowercase();
                    needle.split_whitespace().all(|w| hay.contains(w))
                })
                .map(|(i, _)| i)
                .collect();
            let hosts: Vec<(&str, &str)> = idx
                .iter()
                .map(|i| {
                    (
                        self.profiles[*i].folder.as_str(),
                        self.profiles[*i].name.as_str(),
                    )
                })
                .collect();
            return hostops::tree_rows(&hosts, &[], &HashSet::new())
                .into_iter()
                .map(|r| match r {
                    TreeRow::Folder {
                        path, depth, count, ..
                    } => SidebarRow::Folder {
                        path,
                        depth,
                        count,
                        collapsed: false,
                    },
                    TreeRow::Host { index, depth } => SidebarRow::Profile {
                        ix: idx[index],
                        depth,
                        place: Place::Tree,
                    },
                })
                .collect();
        }
        let mut rows = Vec::new();
        let mut favs: Vec<usize> = (0..self.profiles.len())
            .filter(|i| hostops::is_favourite(&self.profiles[*i]))
            .collect();
        favs.sort_by_key(|i| self.profiles[*i].name.to_lowercase());
        if !favs.is_empty() {
            let collapsed = self.collapsed_secs.contains(&Sec::Favourites);
            rows.push(SidebarRow::Section {
                sec: Sec::Favourites,
                count: favs.len(),
                collapsed,
            });
            if !collapsed {
                rows.extend(favs.into_iter().map(|ix| SidebarRow::Profile {
                    ix,
                    depth: 0,
                    place: Place::Fav,
                }));
            }
        }
        let recents: Vec<SidebarRow> =
            self.recents
                .iter()
                .filter_map(|r| match r {
                    Recent::Host { id } => {
                        self.profiles.iter().position(|p| p.id == *id).map(|ix| {
                            SidebarRow::Profile {
                                ix,
                                depth: 0,
                                place: Place::Recent,
                            }
                        })
                    }
                    Recent::Quick { text } => Some(SidebarRow::Quick { text: text.clone() }),
                })
                .take(RECENT_SHOWN)
                .collect();
        if !recents.is_empty() {
            let collapsed = self.collapsed_secs.contains(&Sec::Recent);
            rows.push(SidebarRow::Section {
                sec: Sec::Recent,
                count: recents.len(),
                collapsed,
            });
            if !collapsed {
                rows.extend(recents);
            }
        }
        let hosts: Vec<(&str, &str)> = self
            .profiles
            .iter()
            .map(|p| (p.folder.as_str(), p.name.as_str()))
            .collect();
        let tree = hostops::tree_rows(&hosts, explicit, &self.collapsed);
        if !tree.is_empty() {
            let collapsed = self.collapsed_secs.contains(&Sec::Hosts);
            rows.push(SidebarRow::Section {
                sec: Sec::Hosts,
                count: self.profiles.len(),
                collapsed,
            });
            if !collapsed {
                rows.extend(tree.into_iter().map(|r| match r {
                    TreeRow::Folder {
                        path,
                        depth,
                        count,
                        collapsed,
                    } => SidebarRow::Folder {
                        path,
                        depth,
                        count,
                        collapsed,
                    },
                    TreeRow::Host { index, depth } => SidebarRow::Profile {
                        ix: index,
                        depth,
                        place: Place::Tree,
                    },
                }));
            }
        }
        rows
    }

    /// Hosts the list shows in the tree, in order (for Shift+click and
    /// Ctrl+A).
    fn tree_order(&self, cx: &App) -> Vec<ProfileId> {
        let mut seen = HashSet::new();
        self.sidebar_rows(cx)
            .into_iter()
            .filter_map(|r| match r {
                SidebarRow::Profile { ix, .. } => self.profiles.get(ix).map(|p| p.id),
                _ => None,
            })
            .filter(|id| seen.insert(*id))
            .collect()
    }

    /// The hosts an action applies to: the multi-selection, else the
    /// selected host.
    pub(crate) fn selection(&self) -> Vec<ProfileId> {
        if self.marked.len() > 1 {
            // In list order.
            self.profiles
                .iter()
                .filter(|p| self.marked.contains(&p.id))
                .map(|p| p.id)
                .collect()
        } else {
            self.selected.into_iter().collect()
        }
    }

    pub(crate) fn set_cursor(&mut self, row: Option<RowId>) {
        self.selected = match &row {
            Some(RowId::Host(id, _)) => Some(*id),
            _ => None,
        };
        self.cursor = row;
    }

    /// Select one saved host (cursor in the tree).
    pub(crate) fn select_host(&mut self, id: ProfileId, cx: &mut Context<Self>) {
        self.set_cursor(Some(RowId::Host(id, Place::Tree)));
        self.marked = BTreeSet::from([id]);
        self.anchor = Some(id);
        cx.notify();
    }

    /// Persist collapsed folders and sections.
    fn save_sidebar_state(&self) {
        let state = SidebarState {
            collapsed: self.collapsed.iter().cloned().collect(),
            sections: self.collapsed_secs.iter().copied().collect(),
        };
        if let Ok(json) = serde_json::to_string(&state)
            && let Err(e) = self.backend.store.set_setting(SIDEBAR_KEY, &json)
        {
            tracing::warn!(error = %e, "could not save the sidebar state");
        }
    }

    fn toggle_row(&mut self, row: &RowId, open: Option<bool>, cx: &mut Context<Self>) {
        match row {
            RowId::Section(s) => {
                let now_open = self.collapsed_secs.contains(s);
                if open.is_none_or(|o| o == now_open) {
                    if now_open {
                        self.collapsed_secs.remove(s);
                    } else {
                        self.collapsed_secs.insert(*s);
                    }
                }
            }
            RowId::Folder(p) => {
                let now_open = self.collapsed.contains(p);
                if open.is_none_or(|o| o == now_open) {
                    if now_open {
                        self.collapsed.remove(p);
                    } else {
                        self.collapsed.insert(p.clone());
                    }
                }
            }
            _ => return,
        }
        self.save_sidebar_state();
        cx.notify();
    }

    // ---- mouse ------------------------------------------------------------------

    fn click_host(
        &mut self,
        id: ProfileId,
        place: Place,
        ev: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = ev.modifiers();
        if secondary(&m) {
            // Ctrl/⌘+click toggles.
            if !self.marked.remove(&id) {
                if self.marked.is_empty()
                    && let Some(s) = self.selected
                {
                    self.marked.insert(s);
                }
                self.marked.insert(id);
            }
            self.cursor = Some(RowId::Host(id, place));
            self.selected = Some(id);
            self.anchor = Some(id);
        } else if m.shift {
            let order = self.tree_order(cx);
            let a = self
                .anchor
                .and_then(|a| order.iter().position(|x| *x == a))
                .unwrap_or(0);
            if let Some(b) = order.iter().position(|x| *x == id) {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.marked = order[lo..=hi].iter().copied().collect();
            }
            self.cursor = Some(RowId::Host(id, place));
            self.selected = Some(id);
        } else {
            self.set_cursor(Some(RowId::Host(id, place)));
            self.marked = BTreeSet::from([id]);
            self.anchor = Some(id);
            if ev.click_count() >= 2
                && let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned()
            {
                self.open_tab(p, TabKind::Terminal, window, cx);
            }
        }
        cx.notify();
    }

    // ---- keyboard ---------------------------------------------------------------

    /// Keys in the host list: arrows/Home/End/PageUp/PageDown move (Shift
    /// extends the selection), Left/Right fold folders, Enter opens a
    /// terminal (Ctrl/⌘ files, Shift a new split, Alt edits), F2 renames,
    /// Delete deletes (after asking), Space marks, Ctrl/⌘+A marks all,
    /// Escape clears the marks, Shift+F10 or the Menu key opens the menu.
    pub(super) fn on_list_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.renaming.is_some() {
            return;
        }
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if crate::a11y::is_menu_key(ks) {
            if self.cursor.is_none() {
                self.move_cursor(0, false, cx);
            }
            self.host_anchor.open(window, cx);
            cx.stop_propagation();
            return;
        }
        let handled = match ks.key.as_str() {
            "a" if secondary(&m) && !m.shift && !m.alt => {
                self.marked = self.tree_order(cx).into_iter().collect();
                cx.notify();
                true
            }
            _ if m.control && !secondary(&m) => false,
            _ if m.platform && !secondary(&m) => false,
            "up" if !m.alt => self.move_cursor(-1, m.shift, cx),
            "down" if !m.alt => self.move_cursor(1, m.shift, cx),
            "pageup" => self.move_cursor(-10, m.shift, cx),
            "pagedown" => self.move_cursor(10, m.shift, cx),
            "home" => self.move_cursor(isize::MIN, m.shift, cx),
            "end" => self.move_cursor(isize::MAX, m.shift, cx),
            "left" | "right" if !m.alt && !m.shift => {
                let open = ks.key == "right";
                match self.cursor.clone() {
                    Some(r @ (RowId::Section(_) | RowId::Folder(_))) => {
                        let is_open = match &r {
                            RowId::Section(s) => !self.collapsed_secs.contains(s),
                            RowId::Folder(p) => !self.collapsed.contains(p),
                            _ => false,
                        };
                        if !open && !is_open {
                            // Already closed: go to the parent folder.
                            if let RowId::Folder(p) = &r {
                                let up = hostops::parent(p);
                                self.set_cursor(Some(if up.is_empty() {
                                    RowId::Section(Sec::Hosts)
                                } else {
                                    RowId::Folder(up.to_owned())
                                }));
                                self.scroll_to_cursor(cx);
                            }
                        } else {
                            self.toggle_row(&r, Some(open), cx);
                        }
                    }
                    Some(RowId::Host(id, Place::Tree)) if !open => {
                        let folder = self
                            .profiles
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| p.folder.clone())
                            .unwrap_or_default();
                        if !folder.is_empty() {
                            self.set_cursor(Some(RowId::Folder(folder)));
                            self.scroll_to_cursor(cx);
                        }
                    }
                    _ => {}
                }
                cx.notify();
                true
            }
            "enter" => {
                let mods = Mods::from_modifiers(&m);
                self.list_enter(mods, window, cx);
                true
            }
            "space" if !m.shift && !m.alt => {
                if let Some(id) = self.selected {
                    if self.marked.len() <= 1 {
                        self.marked = BTreeSet::from([id]);
                    } else if !self.marked.remove(&id) {
                        self.marked.insert(id);
                    }
                    cx.notify();
                }
                true
            }
            "escape" if self.marked.len() > 1 => {
                self.marked = self.selected.into_iter().collect();
                cx.notify();
                true
            }
            "f2" => {
                self.start_rename(window, cx);
                true
            }
            "delete" => {
                self.delete_at_cursor(window, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    fn list_enter(&mut self, mods: Mods, window: &mut Window, cx: &mut Context<Self>) {
        match self.cursor.clone() {
            Some(r @ (RowId::Section(_) | RowId::Folder(_))) => self.toggle_row(&r, None, cx),
            Some(RowId::Quick(text)) => {
                self.run_choice(Choice::Quick(text), mods, window, cx);
            }
            Some(RowId::Host(id, _)) => {
                let sel = self.selection();
                if sel.len() > 1 && sel.contains(&id) {
                    match mods {
                        Mods::Alt => self.bulk_edit(sel, window, cx),
                        Mods::Secondary => {
                            self.open_group(sel, super::hosts::GroupOpen::Files, window, cx)
                        }
                        _ => self.open_group(sel, super::hosts::GroupOpen::Tabs, window, cx),
                    }
                } else {
                    self.note_use(&format!("host:{id}"));
                    self.run_choice(Choice::Host(id), mods, window, cx);
                }
            }
            None => {}
        }
    }

    /// Move the cursor by `delta` rows (clamped; `isize::MIN`/`MAX` for
    /// first/last); with `extend`, the marked range follows.
    pub(super) fn move_cursor(
        &mut self,
        delta: isize,
        extend: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let rows = self.sidebar_rows(cx);
        if rows.is_empty() {
            return true;
        }
        let ids: Vec<Option<RowId>> = rows.iter().map(|r| r.id(&self.profiles)).collect();
        let cur = self
            .cursor
            .as_ref()
            .and_then(|c| ids.iter().position(|i| i.as_ref() == Some(c)));
        let last = rows.len() as isize - 1;
        let next = match (cur, delta) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => last,
            (None, _) => 0,
            (Some(c), d) => (c as isize).saturating_add(d).clamp(0, last),
        } as usize;
        let row = ids[next].clone();
        self.set_cursor(row);
        match self.selected {
            Some(id) if extend => {
                let order = self.tree_order(cx);
                let a = self.anchor.and_then(|a| order.iter().position(|x| *x == a));
                match (a, order.iter().position(|x| *x == id)) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                        self.marked = order[lo..=hi].iter().copied().collect();
                    }
                    _ => {
                        self.marked.insert(id);
                        self.anchor = Some(id);
                    }
                }
            }
            Some(id) => {
                self.marked = BTreeSet::from([id]);
                self.anchor = Some(id);
            }
            None if !extend => self.marked.clear(),
            None => {}
        }
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    fn scroll_to_cursor(&mut self, cx: &mut Context<Self>) {
        let rows = self.sidebar_rows(cx);
        if let Some(ix) = rows
            .iter()
            .position(|r| r.id(&self.profiles).as_ref() == self.cursor.as_ref())
        {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    fn delete_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.cursor.clone() {
            Some(RowId::Folder(p)) => self.delete_folder(p, window, cx),
            Some(RowId::Host(..)) => {
                let sel = self.selection();
                if sel.len() > 1 {
                    self.delete_hosts(sel, window, cx);
                } else if let Some(id) = sel.first() {
                    self.delete_profile(*id, window, cx);
                }
            }
            Some(RowId::Quick(text)) => {
                self.recents
                    .retain(|r| *r != Recent::Quick { text: text.clone() });
                self.save_recents();
                self.set_cursor(None);
                cx.notify();
            }
            _ => {}
        }
    }

    // ---- inline rename ----------------------------------------------------------

    /// F2: rename the host or folder under the cursor in place.
    pub(crate) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (row, text) = match self.cursor.clone() {
            Some(r @ RowId::Host(id, _)) => {
                let Some(p) = self.profiles.iter().find(|p| p.id == id) else {
                    return;
                };
                (r, p.name.clone())
            }
            Some(RowId::Folder(path)) => {
                let name = hostops::leaf(&path).to_owned();
                (RowId::Folder(path), name)
            }
            _ => return,
        };
        self.renaming = Some(row);
        self.rename_input.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.select_all(window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    /// Enter in the rename field (or it lost focus): save.
    pub(super) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.renaming.take() else {
            return;
        };
        let text = self.rename_input.read(cx).value().trim().to_owned();
        match row {
            RowId::Host(id, place) => {
                if let Some(mut p) = self.profiles.iter().find(|p| p.id == id).cloned()
                    && !text.is_empty()
                    && text != p.name
                {
                    if text.contains('/') {
                        crate::notify::ToastExt::toast(
                            window,
                            crate::notify::Toast::error(t!("rename-no-slash")),
                            cx,
                        );
                    } else {
                        p.name = text;
                        self.save_profiles(vec![p], cx);
                    }
                }
                self.cursor = Some(RowId::Host(id, place));
            }
            RowId::Folder(path) if !text.is_empty() && text != hostops::leaf(&path) => {
                if hostops::valid_folder_name(&text) {
                    let to = hostops::join(hostops::parent(&path), &text);
                    self.move_folder(&path, &to, cx);
                    self.cursor = Some(RowId::Folder(to));
                } else {
                    crate::notify::ToastExt::toast(
                        window,
                        crate::notify::Toast::error(t!("rename-no-slash")),
                        cx,
                    );
                }
            }
            _ => {}
        }
        self.list_focus.focus(window, cx);
        cx.notify();
    }

    /// The rename field: Enter saves, Escape cancels, the key never reaches
    /// the list.
    fn render_rename(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .capture_action(cx.listener(|app, _: &Enter, window, cx| {
                app.commit_rename(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|app, _: &Escape, window, cx| {
                app.cancel_rename(window, cx);
                cx.stop_propagation();
            }))
            .child(
                Input::new(&self.rename_input)
                    .xsmall()
                    .aria_label(t!("menu-rename")),
            )
    }

    pub(super) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.list_focus.focus(window, cx);
            cx.notify();
        }
    }

    // ---- quick connect ----------------------------------------------------------

    /// Suggestions for the quick-connect text: saved hosts, recent quick
    /// connections and ssh_config aliases (best first), and connecting to
    /// the text as typed.
    pub(super) fn quick_suggestions(&self, text: &str) -> Vec<QuickSug> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let q = Query::new(text);
        let now = crate::fuzzy::now_secs();
        let mut hits: Vec<(i32, QuickSug)> = Vec::new();
        for p in &self.profiles {
            let label: SharedString = p.path().into();
            let detail: SharedString = address(p).into();
            if let Some(h) = match_fields(&q, &Prepared::new(&label), &Prepared::new(&detail)) {
                let s = h.score + self.frecency.boost(&format!("host:{}", p.id), now);
                hits.push((
                    s,
                    QuickSug {
                        choice: Choice::Host(p.id),
                        label,
                        detail,
                        hit: h,
                    },
                ));
            }
        }
        for r in &self.recents {
            if let Recent::Quick { text: t } = r
                && let Some(h) = match_fields(&q, &Prepared::new(t), &Prepared::default())
            {
                let s = h.score + self.frecency.boost(&format!("quick:{t}"), now);
                hits.push((
                    s,
                    QuickSug {
                        choice: Choice::Quick(t.clone()),
                        label: t.clone().into(),
                        detail: t!("palette-quick-detail"),
                        hit: h,
                    },
                ));
            }
        }
        for a in &self.ssh_aliases {
            let label: SharedString = a.name.clone().into();
            let detail: SharedString = address(a).into();
            if let Some(h) = match_fields(&q, &Prepared::new(&label), &Prepared::new(&detail)) {
                let s = h.score + self.frecency.boost(&format!("ssh:{}", a.name), now);
                hits.push((
                    s,
                    QuickSug {
                        choice: Choice::Alias(a.clone()),
                        label,
                        detail: format!("{detail} · ~/.ssh/config").into(),
                        hit: h,
                    },
                ));
            }
        }
        hits.sort_by_key(|h| std::cmp::Reverse(h.0));
        let mut out: Vec<QuickSug> = hits
            .into_iter()
            .map(|(_, s)| s)
            .filter(|s| !matches!(&s.choice, Choice::Quick(t) if t == text))
            .take(QUICK_SHOWN - 1)
            .collect();
        if hostwick_core::Target::parse(text).is_ok() && !text.contains(char::is_whitespace) {
            let typed = QuickSug {
                choice: Choice::Quick(text.to_owned()),
                label: t!("palette-connect-to", target = text),
                detail: SharedString::default(),
                hit: Hit::default(),
            };
            if text.contains(['@', ':', '.']) || out.is_empty() {
                out.insert(0, typed);
            } else {
                out.push(typed);
            }
        }
        out
    }

    fn quick_visible(&self) -> bool {
        self.quick_open && !self.quick_sugs.is_empty()
    }

    /// Enter in quick connect: the highlighted suggestion, else the text.
    pub(super) fn quick_confirm(
        &mut self,
        mods: Mods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.quick.read(cx).value().trim().to_owned();
        let pick = if self.quick_visible() {
            self.quick_sugs
                .get(self.quick_sel)
                .map(|s| s.choice.clone())
        } else {
            None
        };
        let choice = match pick {
            Some(c) => c,
            None if text.is_empty() => return,
            None => Choice::Quick(text),
        };
        self.quick_pick(choice, mods, window, cx);
    }

    fn quick_pick(
        &mut self,
        choice: Choice,
        mods: Mods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = match &choice {
            Choice::Host(id) => Some(format!("host:{id}")),
            Choice::Quick(t) => Some(format!("quick:{t}")),
            Choice::Alias(a) => Some(format!("ssh:{}", a.name)),
            _ => None,
        };
        if let Some(k) = key {
            self.note_use(&k);
        }
        self.quick.update(cx, |s, cx| s.set_value("", window, cx));
        self.quick_sugs.clear();
        self.quick_open = false;
        self.run_choice(choice, mods, window, cx);
        cx.notify();
    }

    fn render_quick(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let visible = self.quick_visible();
        let sugs = self.quick_sugs.clone();
        let sel = self.quick_sel;
        let hl = HighlightStyle {
            color: Some(theme.link),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        div()
            .relative()
            .capture_action(cx.listener(|app, _: &MoveDown, _, cx| {
                if app.quick_visible() {
                    app.quick_sel = (app.quick_sel + 1) % app.quick_sugs.len();
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|app, _: &MoveUp, _, cx| {
                if app.quick_visible() {
                    let n = app.quick_sugs.len();
                    app.quick_sel = (app.quick_sel + n - 1) % n;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|app, a: &Enter, window, cx| {
                let mods = if a.secondary {
                    Mods::Secondary
                } else if a.shift {
                    Mods::Shift
                } else {
                    Mods::Plain
                };
                app.quick_confirm(mods, window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|app, _: &Escape, window, cx| {
                if app.quick_visible() {
                    app.quick_open = false;
                    cx.notify();
                    cx.stop_propagation();
                } else if !app.quick.read(cx).value().is_empty() {
                    app.quick.update(cx, |s, cx| s.set_value("", window, cx));
                    cx.stop_propagation();
                }
            }))
            .capture_key_down(cx.listener(|app, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "enter" && ev.keystroke.modifiers.alt {
                    app.quick_confirm(Mods::Alt, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Input::new(&self.quick)
                    .small()
                    .aria_label(t!("sidebar-quick-connect")),
            )
            .when(visible, |d| {
                d.child(deferred(
                    div()
                        .id("quick-suggestions")
                        .role(Role::ListBox)
                        .aria_label(t!("quick-suggestions"))
                        .absolute()
                        .top(px(30.0))
                        .left_0()
                        .w(px(320.0))
                        .p(px(4.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .children(sugs.into_iter().enumerate().map(|(i, s)| {
                            let selected = i == sel;
                            let label = StyledText::new(s.label.clone()).with_highlights(
                                byte_ranges(&s.label, &s.hit.label)
                                    .into_iter()
                                    .map(|r| (r, hl)),
                            );
                            let detail = StyledText::new(s.detail.clone()).with_highlights(
                                byte_ranges(&s.detail, &s.hit.detail)
                                    .into_iter()
                                    .map(|r| (r, hl)),
                            );
                            let choice = s.choice.clone();
                            div()
                                .id(("quick-sug", i))
                                .role(Role::ListBoxOption)
                                .aria_label(format!("{}, {}", s.label, s.detail))
                                .aria_selected(selected)
                                .when(selected, |d| d.aria_active_descendant())
                                .h(px(26.0))
                                .px(px(6.0))
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .text_size(px(13.0))
                                .cursor_pointer()
                                .when(selected, |d| {
                                    d.bg(theme.accent).text_color(theme.accent_foreground)
                                })
                                .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                                // On press: the field loses focus right after.
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |app, ev: &MouseDownEvent, window, cx| {
                                        let mods = Mods::from_modifiers(&ev.modifiers);
                                        app.quick_pick(choice.clone(), mods, window, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .when(!s.detail.is_empty(), |d| d.max_w(relative(0.65)))
                                        .when(s.detail.is_empty(), |d| d.flex_1().min_w_0())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(11.0))
                                        .text_color(theme.muted_foreground)
                                        .child(detail),
                                )
                        }))
                        .child(
                            div()
                                .px(px(6.0))
                                .pt(px(4.0))
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(crate::a11y::text(t!(
                                    "quick-hint",
                                    files = keys("Ctrl+↵", "⌘↵")
                                ))),
                        ),
                ))
            })
            .into_any_element()
    }

    // ---- drawing ------------------------------------------------------------------

    pub(super) fn render_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows = Rc::new(self.sidebar_rows(cx));
        let count = rows.len();
        let sel = self.selection();
        let has_sel = !sel.is_empty();
        let multi = sel.len() > 1;
        let list_focused = self.list_focus.is_focused(window);
        let anchor = self.host_anchor.clone();
        anchor.clear();
        let quick = self.render_quick(cx);
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
                            .gap(px(2.0))
                            .child(img(logo()).size(px(22.0)).mr(px(4.0)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Hostwick"),
                            )
                            .child(
                                Button::new("palette")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Search)
                                    .tooltip(t!(
                                        "sidebar-palette-tooltip",
                                        keys = crate::shortcuts::Sc::Palette.hint()
                                    ))
                                    .accessibility_label(t!("palette-title"))
                                    .on_click(cx.listener(|app, _, w, cx| app.open_palette(w, cx))),
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
                                Button::new("help")
                                    .ghost()
                                    .small()
                                    .icon(gpui_kit::assets::IconName::CircleQuestionMark)
                                    .tooltip(t!(
                                        "sidebar-help-tooltip",
                                        keys = crate::shortcuts::Sc::Shortcuts.hint()
                                    ))
                                    .accessibility_label(t!("keys-shortcuts"))
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.open_shortcuts(w, cx)),
                                    ),
                            )
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
                    .child(quick)
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
                    .role(Role::Tree)
                    .aria_label(t!("sidebar-hosts-label"))
                    .aria_description(t!("sidebar-hosts-keys"))
                    .on_key_down(cx.listener(Self::on_list_key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, window, cx| {
                            if app.renaming.is_none() {
                                app.list_focus.focus(window, cx);
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        uniform_list(
                            "profiles",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                                range
                                    .map(|ix| {
                                        this.render_row(
                                            &rows[ix],
                                            ix,
                                            list_focused,
                                            &anchor,
                                            window,
                                            cx,
                                        )
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
                    .when(multi, |d| {
                        d.child(div().text_xs().text_color(theme.muted_foreground).child(
                            crate::a11y::text(t!("sidebar-selected-count", count = sel.len())),
                        ))
                    })
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
                                        let sel = app.selection();
                                        app.open_group(sel, super::hosts::GroupOpen::Tabs, w, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-files")
                                    .small()
                                    .label(t!("sidebar-open-files"))
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        let sel = app.selection();
                                        app.open_group(sel, super::hosts::GroupOpen::Files, w, cx)
                                    })),
                            )
                            .child(
                                Button::new("edit-host")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Settings2)
                                    .tooltip(if multi {
                                        t!("menu-bulk-edit", count = sel.len())
                                    } else {
                                        t!("sidebar-edit-host")
                                    })
                                    .accessibility_label(t!("sidebar-edit-host"))
                                    .disabled(!has_sel)
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.edit_selection(w, cx)),
                                    ),
                            )
                            .child(
                                Button::new("del-host")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip(t!("sidebar-delete-host"))
                                    .accessibility_label(t!("sidebar-delete-host"))
                                    .disabled(!has_sel)
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        let sel = app.selection();
                                        if sel.len() > 1 {
                                            app.delete_hosts(sel, w, cx);
                                        } else if let Some(id) = sel.first() {
                                            app.delete_profile(*id, w, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(
                                Button::new("new-host")
                                    .small()
                                    .label(t!("sidebar-new-host"))
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.host_dialog(None, w, cx)),
                                    ),
                            )
                            .child(
                                Button::new("new-folder")
                                    .small()
                                    .ghost()
                                    .icon(gpui_kit::assets::IconName::FolderPlus)
                                    .tooltip(t!("act-new-folder"))
                                    .accessibility_label(t!("act-new-folder"))
                                    .on_click(cx.listener(|app, _, w, cx| {
                                        let parent = app.cursor_folder();
                                        app.new_folder(&parent, w, cx)
                                    })),
                            )
                            .child(Self::import_button(cx.entity().downgrade())),
                    ),
            )
            .into_any_element()
    }

    /// The folder new things go into: the folder under the cursor, or the
    /// selected host's folder.
    pub(crate) fn cursor_folder(&self) -> String {
        match &self.cursor {
            Some(RowId::Folder(p)) => p.clone(),
            Some(RowId::Host(id, Place::Tree)) => self
                .profiles
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.folder.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn render_row(
        &mut self,
        row: &SidebarRow,
        ix: usize,
        list_focused: bool,
        anchor: &crate::a11y::MenuAnchor,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let id = row.id(&self.profiles);
        let is_cursor = id.is_some() && id == self.cursor;
        let base = div()
            .id(("sb-row", ix))
            .w_full()
            .h(px(ROW))
            .flex()
            .items_center()
            .gap(px(6.0))
            .pr(px(8.0))
            .when(is_cursor, |d| d.relative().child(anchor.track()))
            .when(is_cursor && list_focused, |d| {
                d.border_2().border_color(theme.ring)
            });
        let weak = cx.entity().downgrade();
        match row {
            SidebarRow::Section {
                sec,
                count,
                collapsed,
            } => {
                let (sec, collapsed) = (*sec, *collapsed);
                let row_id = RowId::Section(sec);
                let r2 = row_id.clone();
                base.role(Role::TreeItem)
                    .aria_label(format!("{}, {count}", sec.label()))
                    .aria_level(1)
                    .aria_expanded(!collapsed)
                    .aria_selected(is_cursor)
                    .when(is_cursor, |d| d.aria_active_descendant())
                    .pl(px(6.0))
                    .mt(px(2.0))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.sidebar_accent))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.set_cursor(Some(r2.clone()));
                        app.toggle_row(&r2, None, cx);
                    }))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall(),
                    )
                    .child(div().flex_1().child(sec.label()))
                    .child(div().child(count.to_string()))
                    .when(sec == Sec::Hosts, |d| {
                        // Dropping here takes hosts out of their folders.
                        d.drag_over::<DraggedHosts>(|s, _, _, cx| s.bg(cx.theme().drop_target))
                            .on_drop(cx.listener(|app, d: &DraggedHosts, _, cx| {
                                app.move_hosts(&d.ids, "", cx);
                            }))
                    })
                    .context_menu(Self::section_menu(weak, sec))
                    .into_any_element()
            }
            SidebarRow::Folder {
                path,
                depth,
                count,
                collapsed,
            } => {
                let (depth, collapsed, count) = (*depth, *collapsed, *count);
                let row_id = RowId::Folder(path.clone());
                let renaming = self.renaming.as_ref() == Some(&row_id);
                let (p1, p2) = (path.clone(), path.clone());
                let name = hostops::leaf(path).to_owned();
                base.role(Role::TreeItem)
                    .aria_label(t!(
                        "sidebar-folder-spoken",
                        name = name.clone(),
                        count = count
                    ))
                    .aria_level(depth + 2)
                    .aria_expanded(!collapsed)
                    .aria_selected(is_cursor)
                    .when(is_cursor, |d| d.aria_active_descendant())
                    .pl(px(10.0 + 14.0 * depth as f32))
                    .text_size(px(13.0))
                    .when(is_cursor, |d| d.bg(theme.sidebar_accent))
                    .hover(|s| s.bg(theme.sidebar_accent))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        let r = RowId::Folder(p1.clone());
                        app.set_cursor(Some(r.clone()));
                        app.marked.clear();
                        app.toggle_row(&r, None, cx);
                    }))
                    .drag_over::<DraggedHosts>(|s, _, _, cx| {
                        s.bg(cx.theme().drop_target)
                            .border_1()
                            .border_color(cx.theme().drag_border)
                    })
                    .on_drop(cx.listener(move |app, d: &DraggedHosts, _, cx| {
                        app.move_hosts(&d.ids, &p2, cx);
                    }))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall()
                        .text_color(theme.muted_foreground),
                    )
                    .child(
                        Icon::new(if collapsed {
                            IconName::FolderClosed
                        } else {
                            IconName::FolderOpen
                        })
                        .small()
                        .text_color(theme.muted_foreground),
                    )
                    .map(|d| {
                        if renaming {
                            d.child(self.render_rename(cx))
                        } else {
                            d.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground)
                                    .child(count.to_string()),
                            )
                        }
                    })
                    .context_menu(Self::folder_menu(weak, path.clone()))
                    .into_any_element()
            }
            SidebarRow::Quick { text } => {
                let t1 = text.clone();
                base.role(Role::TreeItem)
                    .aria_label(t!("sidebar-quick-spoken", target = text.clone()))
                    .aria_selected(is_cursor)
                    .when(is_cursor, |d| d.aria_active_descendant())
                    .pl(px(10.0))
                    .text_size(px(13.0))
                    .when(is_cursor, |d| d.bg(theme.sidebar_accent))
                    .hover(|s| s.bg(theme.sidebar_accent))
                    .on_click(cx.listener(move |app, ev: &ClickEvent, window, cx| {
                        app.set_cursor(Some(RowId::Quick(t1.clone())));
                        app.marked.clear();
                        if ev.click_count() >= 2 {
                            let mods = Mods::from_modifiers(&ev.modifiers());
                            app.run_choice(Choice::Quick(t1.clone()), mods, window, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::Clock)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(text.clone()),
                    )
                    .into_any_element()
            }
            SidebarRow::Profile {
                ix: pix,
                depth,
                place,
            } => {
                let Some(p) = self.profiles.get(*pix) else {
                    return div().into_any_element();
                };
                let (depth, place) = (*depth, *place);
                let pid = p.id;
                let marked = self.marked.contains(&pid) && self.marked.len() > 1;
                // With several marked, the marks show; else the cursor.
                let selected = if self.marked.len() > 1 {
                    marked
                } else {
                    is_cursor
                };
                let fav = hostops::is_favourite(p);
                let renaming = self.renaming.as_ref() == Some(&RowId::Host(pid, place));
                let serial = matches!(p.protocol, Protocol::Serial(_));
                let addr = address(p);
                let tip = format!("{} — {} {addr}", p.path(), protocol_name(&p.protocol));
                let spoken = format!("{}, {}, {addr}", p.path(), protocol_name(&p.protocol));
                // Dragging a marked host takes the whole selection along.
                let drag_ids: Vec<ProfileId> =
                    if self.marked.contains(&pid) && self.marked.len() > 1 {
                        self.selection()
                    } else {
                        vec![pid]
                    };
                let drag_label: SharedString = if drag_ids.len() > 1 {
                    t!("sidebar-drag-hosts", count = drag_ids.len())
                } else {
                    p.name.clone().into()
                };
                let folder = p.folder.clone();
                base.group("sb-host")
                    .role(Role::TreeItem)
                    .aria_label(spoken)
                    .aria_level(depth + 2)
                    .aria_selected(selected)
                    .when(is_cursor, |d| d.aria_active_descendant())
                    .pl(px(10.0 + 14.0 * depth as f32))
                    .text_size(px(13.0))
                    .when(selected, |d| d.bg(theme.sidebar_accent))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.sidebar_accent)))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |app, ev: &ClickEvent, window, cx| {
                        app.click_host(pid, place, ev, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |app, _, _, cx| {
                            // Right-click inside the selection keeps it.
                            if !(app.marked.contains(&pid) && app.marked.len() > 1) {
                                app.set_cursor(Some(RowId::Host(pid, place)));
                                app.marked = BTreeSet::from([pid]);
                                app.anchor = Some(pid);
                            }
                            cx.notify();
                        }),
                    )
                    .when(!renaming, |d| {
                        d.on_drag(
                            DraggedHosts {
                                ids: drag_ids,
                                label: drag_label,
                            },
                            |d, _, _, cx| cx.new(|_| DragGhost(d.label.clone())),
                        )
                    })
                    .when(place == Place::Tree, |d| {
                        // Dropping on a host puts the dragged hosts beside it.
                        d.drag_over::<DraggedHosts>(|s, _, _, cx| s.bg(cx.theme().drop_target))
                            .on_drop(cx.listener(move |app, d: &DraggedHosts, _, cx| {
                                app.move_hosts(&d.ids, &folder, cx);
                            }))
                    })
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
                    .map(|d| {
                        if renaming {
                            d.child(self.render_rename(cx))
                        } else {
                            d.child(
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
                                    .max_w(px(96.0))
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
                        }
                    })
                    .child(
                        div()
                            .id(("fav", ix))
                            .flex_none()
                            .role(Role::CheckBox)
                            .aria_label(t!("sidebar-favourite-toggle", name = p.name.clone()))
                            .aria_toggled(if fav {
                                gpui_kit::accesskit::Toggled::True
                            } else {
                                gpui_kit::accesskit::Toggled::False
                            })
                            .on_a11y_action(gpui_kit::accesskit::Action::Click, {
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    let _ =
                                        weak.update(cx, |app, cx| app.toggle_favourite(&[pid], cx));
                                }
                            })
                            .cursor_pointer()
                            .rounded(px(3.0))
                            .hover(|s| s.bg(theme.muted))
                            .text_color(if fav {
                                theme.warning
                            } else {
                                theme.muted_foreground
                            })
                            .when(!fav && !selected, |d| {
                                d.opacity(0.0).group_hover("sb-host", |s| s.opacity(1.0))
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.toggle_favourite(&[pid], cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                Icon::new(if fav {
                                    IconName::StarFill
                                } else {
                                    IconName::Star
                                })
                                .xsmall(),
                            ),
                    )
                    .context_menu(Self::host_menu(weak, pid))
                    .into_any_element()
            }
        }
    }
}
