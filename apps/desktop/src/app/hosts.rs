//! Managing saved hosts with fewer clicks: favourites, recent connections,
//! opening a whole folder (tabs or a grid), bulk delete, folders (new,
//! rename, move, delete) and the theme toggle.

use std::collections::{BTreeSet, HashSet};

use crate::zoom::px;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, ProfileId, ProfileStore as _};

use super::sidebar::{RowId, Sec};
use super::{HostwickApp, Pane, Tab, TabKind};
use crate::hostops::{self, Recent};
use crate::notify::{Toast, ToastExt as _};
use crate::settings::{Settings, ThemeChoice};

/// Opening many hosts at once asks first above this many.
const CONFIRM_ABOVE: usize = 12;

pub(crate) const RECENTS_KEY: &str = "desktop.recents";
pub(crate) const FRECENCY_KEY: &str = "desktop.frecency";

/// How to open a group of hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupOpen {
    /// A terminal tab each.
    Tabs,
    /// A file browser tab each.
    Files,
    /// One tab, terminals tiled.
    Grid,
    /// One tab, terminals tiled, typing goes to all of them.
    GridBroadcast,
}

/// A yes/no dialog; `on_ok` runs when confirmed.
pub(crate) fn confirm(
    window: &mut Window,
    cx: &mut App,
    title: SharedString,
    body: SharedString,
    ok: SharedString,
    danger: bool,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_ok = std::rc::Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, _, _| {
        let (go, enter) = (on_ok.clone(), on_ok.clone());
        dialog
            .title(crate::a11y::text(title.clone()))
            .w(px(480.0))
            .child(
                div()
                    .whitespace_normal()
                    .text_size(px(13.0))
                    .child(crate::a11y::text(body.clone())),
            )
            .on_ok(move |_, window, cx| {
                window.close_dialog(cx);
                enter(window, cx);
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
                        Button::new("ok")
                            .label(ok.clone())
                            .when(danger, |b| b.danger())
                            .when(!danger, |b| b.primary())
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                go(window, cx);
                            }),
                    ),
            )
    });
}

impl HostwickApp {
    // ---- frecency and recents ---------------------------------------------------

    /// Count a use of something (palette ranking).
    pub(crate) fn note_use(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        self.frecency.record(key, crate::fuzzy::now_secs());
        if let Err(e) = self
            .backend
            .store
            .set_setting(FRECENCY_KEY, &self.frecency.to_json())
        {
            tracing::warn!(error = %e, "could not save usage counts");
        }
    }

    /// A connection was opened: it becomes the most recent.
    pub(crate) fn note_recent(&mut self, profile: &HostProfile, quick: Option<&str>) {
        let saved = self.profiles.iter().any(|p| p.id == profile.id);
        let r = match quick {
            Some(q) if !saved => Recent::Quick { text: q.to_owned() },
            _ if saved => Recent::Host { id: profile.id },
            _ => return,
        };
        hostops::push_recent(&mut self.recents, r);
        self.save_recents();
    }

    pub(crate) fn save_recents(&self) {
        if let Err(e) = self
            .backend
            .store
            .set_setting(RECENTS_KEY, &hostops::recents_to_json(&self.recents))
        {
            tracing::warn!(error = %e, "could not save recent connections");
        }
    }

    // ---- saving -------------------------------------------------------------------

    /// Save changed hosts: shown at once, written off the UI thread.
    pub(crate) fn save_profiles(&mut self, list: Vec<HostProfile>, cx: &mut Context<Self>) {
        if list.is_empty() {
            return;
        }
        for p in &list {
            if let Some(slot) = self.profiles.iter_mut().find(|x| x.id == p.id) {
                *slot = p.clone();
            }
        }
        self.refresh_tab_profiles();
        let backend = self.backend.clone();
        let job = self.backend.rt.spawn_blocking(move || {
            list.iter()
                .filter_map(|p| {
                    backend
                        .store
                        .upsert(p)
                        .err()
                        .map(|e| format!("{}: {e}", p.path()))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let errors = job.await.unwrap_or_else(|e| vec![e.to_string()]);
            let _ = this.update(cx, |a, cx| {
                for e in &errors {
                    tracing::warn!("saving host: {e}");
                }
                if !errors.is_empty() {
                    a.pending_error =
                        Some(t!("hosts-save-failed", error = errors.join("; ")).into());
                }
                a.reload_profiles();
                a.refresh_tab_profiles();
                a.mark_sync_dirty();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Open tabs see edited hosts (new connections use the edit).
    pub(crate) fn refresh_tab_profiles(&mut self) {
        for t in &mut self.tabs {
            for pane in t.root.leaves_mut() {
                if let Some(p) = self.profiles.iter().find(|p| p.id == pane.profile.id) {
                    pane.profile = p.clone();
                }
            }
        }
    }

    /// Change the app settings (empty folders, default keys) and save.
    pub(crate) fn update_settings(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Settings),
    ) {
        let mut s = Settings::get(cx).clone();
        f(&mut s);
        if let Err(e) = s.save(&self.backend.store) {
            tracing::warn!("cannot save settings: {e}");
        }
        cx.set_global(s);
        self.mark_sync_dirty();
    }

    // ---- favourites ---------------------------------------------------------------

    /// Star or unstar: all of `ids` become favourites unless all already are.
    pub(crate) fn toggle_favourite(&mut self, ids: &[ProfileId], cx: &mut Context<Self>) {
        let hosts: Vec<&HostProfile> = self
            .profiles
            .iter()
            .filter(|p| ids.contains(&p.id))
            .collect();
        let on = !hosts.iter().all(|p| hostops::is_favourite(p));
        let changed: Vec<HostProfile> = hosts
            .into_iter()
            .cloned()
            .filter_map(|mut p| hostops::set_favourite(&mut p, on).then_some(p))
            .collect();
        self.save_profiles(changed, cx);
    }

    // ---- opening many ---------------------------------------------------------------

    /// Open several hosts at once (asks first above 12).
    pub(crate) fn open_group(
        &mut self,
        ids: Vec<ProfileId>,
        mode: GroupOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profiles: Vec<HostProfile> = ids
            .iter()
            .filter_map(|id| self.profiles.iter().find(|p| p.id == *id).cloned())
            .collect();
        if profiles.is_empty() {
            return;
        }
        if profiles.len() <= CONFIRM_ABOVE {
            self.do_open_group(profiles, mode, window, cx);
            return;
        }
        let n = profiles.len();
        let app = cx.entity().downgrade();
        let profiles = std::rc::Rc::new(profiles);
        confirm(
            window,
            cx,
            t!("group-confirm-title", count = n),
            t!("group-confirm-body", count = n),
            t!("group-confirm-open"),
            false,
            move |window, cx| {
                let list = (*profiles).clone();
                let _ = app.update(cx, |a, cx| a.do_open_group(list, mode, window, cx));
            },
        );
    }

    fn do_open_group(
        &mut self,
        profiles: Vec<HostProfile>,
        mode: GroupOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for p in &profiles {
            self.note_recent(p, None);
        }
        match mode {
            GroupOpen::Tabs | GroupOpen::Files => {
                let kind = if mode == GroupOpen::Files {
                    TabKind::Files
                } else {
                    TabKind::Terminal
                };
                let first = self.tabs.len();
                if profiles.len() == 1 {
                    let p = profiles.into_iter().next();
                    if let Some(p) = p {
                        self.open_tab(p, kind, window, cx);
                    }
                    return;
                }
                // Every connection starts now; questions come one at a time.
                for p in profiles {
                    let pane = self.new_pane(p, None);
                    let pane_id = pane.id;
                    let id = self.next_id();
                    self.tabs.push(Tab {
                        id,
                        kind,
                        root: crate::layout::Node::Leaf(pane),
                        focused: pane_id,
                        zoom: crate::zoom::DEFAULT_ZOOM,
                    });
                    self.connect_pane(pane_id, window, cx);
                }
                self.activate_tab(first, window, cx);
            }
            GroupOpen::Grid | GroupOpen::GridBroadcast => {
                let panes: Vec<Pane> = profiles
                    .into_iter()
                    .map(|p| self.new_pane(p, None))
                    .collect();
                let ids: Vec<u64> = panes.iter().map(|p| p.id).collect();
                let Some(root) = crate::layout::grid_tree(panes, &mut super::tabs::fresh_id) else {
                    return;
                };
                let id = self.next_id();
                self.tabs.push(Tab {
                    id,
                    kind: TabKind::Terminal,
                    root,
                    focused: ids[0],
                    zoom: crate::zoom::DEFAULT_ZOOM,
                });
                self.active = Some(self.tabs.len() - 1);
                for pane in &ids {
                    self.connect_pane(*pane, window, cx);
                }
                if mode == GroupOpen::GridBroadcast {
                    self.broadcast = true;
                    window.toast(
                        Toast::info(t!(
                            "group-broadcast-on",
                            keys = crate::shortcuts::Sc::Broadcast.hint()
                        )),
                        cx,
                    );
                }
                self.persist_tabs(cx);
            }
        }
        cx.notify();
    }

    // ---- deleting -----------------------------------------------------------------

    /// Delete several hosts after one confirmation.
    pub(crate) fn delete_hosts(
        &mut self,
        ids: Vec<ProfileId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names: Vec<String> = self
            .profiles
            .iter()
            .filter(|p| ids.contains(&p.id))
            .map(|p| p.path())
            .collect();
        if names.is_empty() {
            return;
        }
        let shown = names.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        let more = names.len().saturating_sub(8);
        let body = if more > 0 {
            t!("hosts-delete-body-more", names = shown, more = more)
        } else {
            t!("hosts-delete-body", names = shown)
        };
        let app = cx.entity().downgrade();
        let n = names.len();
        confirm(
            window,
            cx,
            t!("hosts-delete-title", count = n),
            body,
            t!("button-delete"),
            true,
            move |_, cx| {
                let ids = ids.clone();
                let _ = app.update(cx, |a, cx| a.do_delete_hosts(ids, cx));
            },
        );
    }

    pub(crate) fn do_delete_hosts(&mut self, ids: Vec<ProfileId>, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let gone: HashSet<ProfileId> = ids.iter().copied().collect();
        self.profiles.retain(|p| !gone.contains(&p.id));
        self.marked.retain(|id| !gone.contains(id));
        if self.selected.is_some_and(|s| gone.contains(&s)) {
            self.set_cursor(None);
        }
        self.recents
            .retain(|r| !matches!(r, Recent::Host { id } if gone.contains(id)));
        self.save_recents();
        for id in &ids {
            self.frecency.forget(&format!("host:{id}"));
        }
        let job = self.backend.rt.spawn_blocking(move || {
            for id in ids {
                if let Err(e) = backend.store.delete(id) {
                    tracing::warn!(error = %e, "deleting host");
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let _ = job.await;
            let _ = this.update(cx, |a, cx| {
                a.reload_profiles();
                a.mark_sync_dirty();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ---- folders --------------------------------------------------------------------

    /// Every folder (with hosts or kept empty).
    fn folders(&self, cx: &App) -> BTreeSet<String> {
        hostops::all_folders(
            self.profiles.iter().map(|p| p.folder.as_str()),
            &Settings::get(cx).folders,
        )
    }

    /// Move hosts into `folder` ("" = no folder).
    pub(crate) fn move_hosts(&mut self, ids: &[ProfileId], folder: &str, cx: &mut Context<Self>) {
        let folder = hostops::normalize(folder);
        let changed: Vec<HostProfile> = self
            .profiles
            .iter()
            .filter(|p| ids.contains(&p.id) && hostops::normalize(&p.folder) != folder)
            .cloned()
            .map(|mut p| {
                p.folder = folder.clone();
                p
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        // Show where they went.
        let mut f = folder.as_str();
        while !f.is_empty() {
            self.collapsed.remove(f);
            f = hostops::parent(f);
        }
        self.collapsed_secs.remove(&Sec::Hosts);
        self.save_profiles(changed, cx);
    }

    /// Rename or move folder `from` to `to` (subfolders follow).
    pub(crate) fn move_folder(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let folders: Vec<&str> = self.profiles.iter().map(|p| p.folder.as_str()).collect();
        let change = hostops::move_folder(&folders, &Settings::get(cx).folders, from, to);
        let changed: Vec<HostProfile> = change
            .hosts
            .iter()
            .filter_map(|(i, f)| {
                self.profiles.get(*i).cloned().map(|mut p| {
                    p.folder = f.clone();
                    p
                })
            })
            .collect();
        self.collapsed = self
            .collapsed
            .iter()
            .map(|c| hostops::remap(c, from, to).unwrap_or_else(|| c.clone()))
            .collect();
        let explicit = change.explicit;
        self.update_settings(cx, |s| s.folders = explicit);
        self.save_profiles(changed, cx);
        cx.notify();
    }

    /// A new folder in `parent`, named at once (inline rename).
    pub(crate) fn new_folder(&mut self, parent: &str, window: &mut Window, cx: &mut Context<Self>) {
        let existing = self.folders(cx);
        let path = hostops::unique_folder(parent, &t!("folder-new-name"), &existing);
        let p2 = path.clone();
        self.update_settings(cx, |s| s.folders.push(p2));
        self.collapsed_secs.remove(&Sec::Hosts);
        let mut f = parent;
        while !f.is_empty() {
            self.collapsed.remove(f);
            f = hostops::parent(f);
        }
        self.set_cursor(Some(RowId::Folder(path)));
        self.marked.clear();
        self.start_rename(window, cx);
        cx.notify();
    }

    /// Delete a folder: its hosts and subfolders move to its parent. Asks
    /// first when there is anything in it.
    pub(crate) fn delete_folder(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inside = self
            .profiles
            .iter()
            .filter(|p| hostops::within(&hostops::normalize(&p.folder), &path))
            .count();
        let app = cx.entity().downgrade();
        let run = move |cx: &mut App, path: &str| {
            let path = path.to_owned();
            let _ = app.update(cx, |a, cx| {
                let folders: Vec<&str> = a.profiles.iter().map(|p| p.folder.as_str()).collect();
                let change = hostops::delete_folder(&folders, &Settings::get(cx).folders, &path);
                let changed: Vec<HostProfile> = change
                    .hosts
                    .iter()
                    .filter_map(|(i, f)| {
                        a.profiles.get(*i).cloned().map(|mut p| {
                            p.folder = f.clone();
                            p
                        })
                    })
                    .collect();
                let explicit = change.explicit;
                a.update_settings(cx, |s| s.folders = explicit);
                a.collapsed.retain(|c| !hostops::within(c, &path));
                let up = hostops::parent(&path).to_owned();
                a.set_cursor(Some(if up.is_empty() {
                    RowId::Section(Sec::Hosts)
                } else {
                    RowId::Folder(up)
                }));
                a.save_profiles(changed, cx);
                cx.notify();
            });
        };
        if inside == 0 {
            run(cx, &path);
            return;
        }
        let parent = hostops::parent(&path);
        let target: SharedString = if parent.is_empty() {
            t!("sidebar-section-hosts")
        } else {
            parent.to_owned().into()
        };
        confirm(
            window,
            cx,
            t!("folder-delete-title"),
            t!(
                "folder-delete-body",
                name = hostops::leaf(&path).to_owned(),
                count = inside,
                target = target
            ),
            t!("folder-delete-ok"),
            true,
            move |_, cx| run(cx, &path),
        );
    }

    // ---- editing ----------------------------------------------------------------------

    /// Edit the selected host, or bulk-edit the multi-selection.
    pub(crate) fn edit_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.selection();
        if sel.len() > 1 {
            self.bulk_edit(sel, window, cx);
        } else if let Some(p) = sel
            .first()
            .and_then(|id| self.profiles.iter().find(|p| p.id == *id))
            .cloned()
        {
            self.host_dialog(Some(p), window, cx);
        }
    }

    /// The host editor for a new host that starts filled in.
    pub(crate) fn host_dialog_new(
        &mut self,
        base: HostProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        crate::hostform::open_new(
            base,
            self.backend.clone(),
            window,
            cx,
            move |p, window, cx| {
                let _ = app.update(cx, |a, cx| a.saved_from_editor(p, window, cx));
            },
        );
    }

    /// The host editor saved `p`.
    pub(crate) fn saved_from_editor(
        &mut self,
        p: HostProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Bookmarks are changed in the file browser, not in this editor: an
        // editor opened before a new bookmark must not undo it.
        let mut p = p;
        if let Ok(current) = self.backend.store.get(p.id) {
            p.bookmarks = current.bookmarks;
        }
        match self.backend.store.upsert(&p) {
            Ok(()) => {
                self.reload_profiles();
                self.select_host(p.id, cx);
                self.refresh_tab_profiles();
                self.mark_sync_dirty();
                // A saved ssh_config alias is no longer a suggestion.
                self.ssh_aliases
                    .retain(|a| !a.name.eq_ignore_ascii_case(&p.name));
            }
            Err(e) => window.toast(Toast::error(e.to_string()), cx),
        }
        cx.notify();
    }

    /// Ctrl+Shift+U (⌘T): the active tab's host again, in a new tab.
    pub(crate) fn duplicate_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = self.active.and_then(|i| self.tabs.get(i))
            && let Some(p) = t.focused_pane()
        {
            let (profile, quick, kind) = (p.profile.clone(), p.quick.clone(), t.kind);
            self.open_tab_quick(profile, quick, kind, window, cx);
        }
    }

    /// Dark ⇄ light, saved like the setting.
    pub(crate) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match Settings::get(cx).theme {
            ThemeChoice::Dark => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
        };
        self.update_settings(cx, |s| s.theme = next);
        crate::settings::apply_theme(next, cx);
        self.apply_settings_to_views(cx);
        window.refresh();
    }

    /// Show a problem that happened in the background.
    pub(super) fn flush_pending_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(e) = self.pending_error.take() {
            window.toast(Toast::error(e), cx);
        }
    }

    /// Hosts in a folder (and below), in tree order.
    pub(crate) fn hosts_in(&self, folder: &str) -> Vec<ProfileId> {
        let mut v: Vec<&HostProfile> = self
            .profiles
            .iter()
            .filter(|p| hostops::within(&hostops::normalize(&p.folder), folder))
            .collect();
        v.sort_by_key(|p| p.path().to_lowercase());
        v.into_iter().map(|p| p.id).collect()
    }

    /// Favourites, by name.
    pub(crate) fn favourite_ids(&self) -> Vec<ProfileId> {
        let mut v: Vec<&HostProfile> = self
            .profiles
            .iter()
            .filter(|p| hostops::is_favourite(p))
            .collect();
        v.sort_by_key(|p| p.name.to_lowercase());
        v.into_iter().map(|p| p.id).collect()
    }

    /// Keep the cursor and marks on hosts that still exist.
    pub(crate) fn prune_selection(&mut self) {
        let live: HashSet<ProfileId> = self.profiles.iter().map(|p| p.id).collect();
        self.marked.retain(|id| live.contains(id));
        if let Some(RowId::Host(id, _)) = &self.cursor
            && !live.contains(id)
        {
            self.set_cursor(None);
        }
        if self.selected.is_some_and(|s| !live.contains(&s)) {
            self.selected = None;
        }
    }
}
