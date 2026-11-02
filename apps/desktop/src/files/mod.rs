//! File browser tab, WinSCP "Commander" style: the local computer on the
//! left, the server on the right (the local pane can be hidden).
//!
//! - Copy/move between panes (F5/F6, drag and drop), drop files from the
//!   OS file manager onto either pane, keyboard navigation and
//!   multi-selection, sortable columns.
//! - Edit in an external editor with automatic upload on save ([`edit`]).
//! - Properties: permissions, owner, checksums, folder size, symlinks
//!   ([`props`]).
//! - Reopen as root through sudo, with a password if needed.
//! - Synchronize with a reviewable plan ([`syncdlg`]), and "Keep remote
//!   directory up to date".
//!
//! Every file operation runs on the Tokio runtime; the UI thread only
//! awaits join handles.

mod edit;
mod ops;
mod pane;
mod props;
mod syncdlg;

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedSender;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Selectable as _, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::HostProfile;
use hostwick_sftp::{Entry, LocalFs, RemoteFs, path_to_slash};
use hostwick_transfer::{ExistingPolicy, Job, SyncMode, SyncOptions, TransferState};
use tokio_util::sync::CancellationToken;

use crate::backend::{Backend, FilesSession};
use crate::settings::Settings;
use crate::util::human_size;
use edit::{ConflictChoice, EditEvent, EditSession, EditStatus};
use ops::{editor_command, parent_dir, remove_recursive};
pub use pane::Side;
use pane::{DragRows, Pane};

const SHOW_LOCAL_KEY: &str = "files.show_local";

gpui_kit::actions!(files, [SwitchPane]);

/// Tab switches panes. It needs a binding (the app root binds Tab to
/// focus cycling); a deeper key context wins.
fn bind_keys_once(cx: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        cx.bind_keys([KeyBinding::new("tab", SwitchPane, Some("FilesPane"))]);
    }
}

fn local_dir_key(profile: &HostProfile) -> String {
    format!("files.local_dir.{}", profile.id)
}

fn home_dir() -> String {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| path_to_slash(std::path::Path::new(&h)))
        .unwrap_or_else(|| "/".into())
}

/// A file or folder to copy, from either pane or the OS.
#[derive(Clone)]
struct Item {
    path: String,
    name: String,
    is_dir: bool,
}

impl From<&Entry> for Item {
    fn from(e: &Entry) -> Self {
        Self {
            path: e.path.clone(),
            name: e.name.clone(),
            is_dir: e.is_dir(),
        }
    }
}

pub struct FilesView {
    backend: Arc<Backend>,
    profile: HostProfile,
    local: Pane,
    remote: Pane,
    show_local: bool,
    active: Side,
    /// The remote pane runs through sudo.
    elevated: bool,
    switching_root: bool,
    /// Message above the panes: (is error, text).
    banner: Option<(bool, String)>,
    edits: Vec<EditSession>,
    edit_root: PathBuf,
    next_edit: u64,
    /// Edit ids that were opened for viewing only (no upload).
    open_only: std::collections::HashSet<u64>,
    edit_tx: UnboundedSender<EditEvent>,
    sync: Option<ActiveSync>,
    /// Opened from a terminal: its tab title, and whether the remote pane
    /// follows its current folder.
    link: Option<String>,
    follow: bool,
    focus: FocusHandle,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

struct ActiveSync {
    local: String,
    remote: String,
    cancel: CancellationToken,
    status: String,
    _ui: Task<()>,
}

impl Drop for ActiveSync {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Drop for FilesView {
    fn drop(&mut self) {
        for e in &self.edits {
            e.cancel.cancel();
        }
        // Temporary copies of edited files go with the tab.
        let root = self.edit_root.clone();
        if root.exists()
            && let Err(e) = std::fs::remove_dir_all(&root)
        {
            tracing::warn!(error = %e, dir = %root.display(), "could not remove edit folder");
        }
    }
}

impl Focusable for FilesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FilesView {
    pub fn new(
        session: FilesSession,
        profile: HostProfile,
        backend: Arc<Backend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        bind_keys_once(cx);
        let local_fs: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
        let (local, s1) = Pane::new(Side::Local, local_fs, window, cx);
        let (remote, s2) = Pane::new(Side::Remote, session.fs, window, cx);
        let show_local = backend
            .store
            .setting(SHOW_LOCAL_KEY)
            .ok()
            .flatten()
            .is_none_or(|v| v != "false");
        let start_local = backend
            .store
            .setting(&local_dir_key(&profile))
            .ok()
            .flatten()
            .filter(|d| std::path::Path::new(d).is_dir())
            .unwrap_or_else(home_dir);

        let (edit_tx, mut edit_rx) = futures::channel::mpsc::unbounded::<EditEvent>();
        let edit_task = cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = edit_rx.next().await {
                if this
                    .update_in(cx, |v, window, cx| v.on_edit_event(ev, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        backend.rt.spawn_blocking(edit::sweep_stale);
        let mut v = Self {
            backend,
            profile,
            local,
            remote,
            show_local,
            active: Side::Remote,
            elevated: session.elevated,
            switching_root: false,
            banner: session.notice.map(|n| (true, n)),
            edits: Vec::new(),
            edit_root: edit::new_temp_root(),
            next_edit: 1,
            open_only: Default::default(),
            edit_tx,
            sync: None,
            link: None,
            follow: false,
            focus: cx.focus_handle(),
            _tasks: vec![edit_task],
            _subs: vec![s1, s2],
        };
        v.navigate(Side::Remote, ".".into(), cx);
        v.navigate(Side::Local, start_local, cx);
        v
    }

    // ---- public API for the app ------------------------------------------

    /// Link this browser to a terminal (by its title) so it follows the
    /// terminal's folder; `None` unlinks.
    pub fn set_link(&mut self, terminal: Option<String>, cx: &mut Context<Self>) {
        self.follow = terminal.is_some();
        self.link = terminal;
        cx.notify();
    }

    /// Following the linked terminal's folder now.
    pub fn following(&self) -> bool {
        self.link.is_some() && self.follow
    }

    pub fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        if self.link.is_some() {
            self.follow = !self.follow;
            cx.notify();
        }
    }

    /// Show `path` in the remote pane (e.g. to follow the terminal's
    /// current folder).
    pub fn navigate_remote(&mut self, path: String, cx: &mut Context<Self>) {
        self.navigate(Side::Remote, path, cx);
    }

    /// The folder the remote pane shows (absolute once loaded).
    #[allow(dead_code)] // API for the app (terminal ↔ browser folder sync)
    pub fn remote_cwd(&self) -> &str {
        &self.remote.cwd
    }

    // ---- panes ---------------------------------------------------------

    fn pane(&self, side: Side) -> &Pane {
        match side {
            Side::Local => &self.local,
            Side::Remote => &self.remote,
        }
    }

    fn pane_mut(&mut self, side: Side) -> &mut Pane {
        match side {
            Side::Local => &mut self.local,
            Side::Remote => &mut self.remote,
        }
    }

    fn activate(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        if self.active != side {
            self.active = side;
            cx.notify();
        }
        self.pane(side).focus.clone().focus(window, cx);
    }

    /// Give the keyboard back to the active pane (after a dialog closes).
    fn refocus(&self, window: &mut Window, cx: &mut App) {
        self.pane(self.active).focus.clone().focus(window, cx);
    }

    fn navigate(&mut self, side: Side, path: String, cx: &mut Context<Self>) {
        let pane = self.pane_mut(side);
        pane.loading = true;
        pane.error = None;
        pane.generation += 1;
        let generation = pane.generation;
        let fs = pane.fs.clone();
        let job = self.backend.rt.spawn(async move {
            let abs = fs.canonicalize(&path).await?;
            let list = fs.list_dir(&abs).await?;
            Ok::<_, hostwick_sftp::FsError>((abs, list))
        });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |v, cx| {
                v.loaded(side, generation, result, cx);
            });
        });
        self.pane_mut(side).load = Some(task);
        cx.notify();
    }

    fn loaded(
        &mut self,
        side: Side,
        generation: u64,
        result: Result<
            Result<(String, Vec<Entry>), hostwick_sftp::FsError>,
            tokio::task::JoinError,
        >,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane_mut(side);
        if pane.generation != generation {
            return;
        }
        pane.loading = false;
        match result {
            Ok(Ok((abs, list))) => {
                let same = abs == pane.cwd;
                let name_at = |ix: Option<usize>| {
                    ix.and_then(|i| pane.entries.get(i)).map(|e| e.name.clone())
                };
                let (cursor, anchor) = (name_at(pane.sel.cursor), name_at(pane.sel.anchor));
                let old_ix = pane.sel.cursor;
                pane.entries = list;
                pane.sel.cursor = None;
                pane.sel.anchor = None;
                pane.resort();
                if same {
                    pane.relocate(cursor, anchor);
                    // The cursor's entry is gone (deleted): stay at its place.
                    if pane.sel.cursor.is_none() && !pane.entries.is_empty() {
                        pane.sel.cursor = old_ix.map(|i| i.min(pane.entries.len() - 1));
                    }
                }
                let names: Vec<String> = pane.names().into_iter().map(str::to_owned).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                if same {
                    pane.sel.retain(&refs);
                } else {
                    pane.sel.clear();
                    pane.scroll.scroll_to_item(0, ScrollStrategy::Top);
                }
                pane.cwd = abs;
                pane.path_dirty = true;
                if side == Side::Local && !same {
                    let key = local_dir_key(&self.profile);
                    if let Err(e) = self.backend.store.set_setting(&key, &self.local.cwd) {
                        tracing::debug!(error = %e, "could not remember the local folder");
                    }
                }
            }
            Ok(Err(e)) => pane.error = Some(e.to_string()),
            Err(e) => pane.error = Some(format!("internal error: {e}")),
        }
        cx.notify();
    }

    fn refresh(&mut self, side: Side, cx: &mut Context<Self>) {
        let cwd = self.pane(side).cwd.clone();
        let cwd = if cwd.is_empty() { ".".into() } else { cwd };
        self.navigate(side, cwd, cx);
    }

    fn up(&mut self, side: Side, cx: &mut Context<Self>) {
        if let Some(p) = parent_dir(&self.pane(side).cwd) {
            self.navigate(side, p, cx);
        }
    }

    /// Enter / double-click: folders open; local files open with their
    /// app; remote files are downloaded to the temp folder and opened
    /// (without watching — that is "Edit").
    fn open_entry(&mut self, side: Side, ix: usize, cx: &mut Context<Self>) {
        let Some(e) = self.pane(side).entries.get(ix).cloned() else {
            return;
        };
        if e.is_dir() || e.kind == hostwick_sftp::EntryKind::Symlink {
            // A symlink to a file fails to list and shows the error.
            self.navigate(side, e.path, cx);
            return;
        }
        match side {
            Side::Local => cx.open_with_system(std::path::Path::new(&e.path)),
            Side::Remote => self.start_edit(e, false, cx),
        }
    }

    // ---- keyboard -------------------------------------------------------

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let side = if self.local.focus.is_focused(window) {
            Side::Local
        } else if self.remote.focus.is_focused(window) {
            Side::Remote
        } else {
            return;
        };
        let m = ev.keystroke.modifiers;
        let ctrl = m.control || m.platform;
        let key = ev.keystroke.key.as_str();
        let names: Vec<String> = self
            .pane(side)
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let page = 20;
        let handled = match key {
            "enter" if m.alt => {
                self.properties(side, window, cx);
                true
            }
            "enter" => {
                if let Some(ix) = self.pane(side).sel.cursor {
                    self.open_entry(side, ix, cx);
                }
                true
            }
            "backspace" => {
                self.up(side, cx);
                true
            }
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => {
                let n = refs.len() as isize;
                let delta = match key {
                    "up" => -1,
                    "down" => 1,
                    "pageup" => -page,
                    "pagedown" => page,
                    "home" => -n,
                    _ => n,
                };
                let pane = self.pane_mut(side);
                pane.sel.step(delta, &refs, m.shift);
                if let Some(c) = pane.sel.cursor {
                    pane.scroll.scroll_to_item(c, ScrollStrategy::Nearest);
                }
                true
            }
            "a" if ctrl => {
                self.pane_mut(side).sel.select_all(&refs);
                true
            }
            "r" if ctrl && !m.shift => {
                self.refresh(side, cx);
                true
            }
            "f2" => {
                self.rename_dialog(side, window, cx);
                true
            }
            "f4" => {
                self.edit_selection(side, cx);
                true
            }
            "f5" => {
                self.copy_selection(side, false, window, cx);
                true
            }
            "f6" => {
                if self.show_local {
                    self.copy_selection(side, true, window, cx);
                }
                true
            }
            "f7" => {
                self.new_folder(side, window, cx);
                true
            }
            "delete" => {
                self.confirm_delete(side, window, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    // ---- copy / move ------------------------------------------------------

    /// F5/F6: to the other pane's folder (after confirming), or — in
    /// single-pane mode — download to a folder picked in the OS dialog.
    fn copy_selection(
        &mut self,
        side: Side,
        mv: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items: Vec<Item> = self.pane(side).targets().iter().map(Item::from).collect();
        if items.is_empty() {
            return;
        }
        if !self.show_local {
            if side == Side::Remote {
                self.download_with_picker(items, cx);
            }
            return;
        }
        let to = side.other();
        let dir = self.pane(to).cwd.clone();
        let what = if items.len() == 1 {
            format!("“{}”", items[0].name)
        } else {
            format!("{} items", items.len())
        };
        let verb = match (mv, side) {
            (true, _) => "Move",
            (false, Side::Local) => "Upload",
            (false, Side::Remote) => "Download",
        };
        let view = cx.entity().downgrade();
        let src = self.pane(side).fs.clone();
        let text = format!("{verb} {what} to {dir}?");
        window.open_dialog(cx, move |dialog, _, _| {
            let (view, items, src, dir) = (view.clone(), items.clone(), src.clone(), dir.clone());
            let view2 = view.clone();
            let go = std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                let _ = view.update(cx, |v, cx| {
                    v.refocus(window, cx);
                    v.transfer(src.clone(), items.clone(), to, dir.clone(), mv, cx)
                });
            });
            let (on_ok, on_click) = (go.clone(), go);
            dialog
                .title(verb)
                .child(text.clone())
                .on_ok(move |_, window, cx| {
                    on_ok(window, cx);
                    false
                })
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(Button::new("cancel").label("Cancel").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = view2.update(cx, |v, cx| v.refocus(window, cx));
                            },
                        ))
                        .child(
                            Button::new("go")
                                .primary()
                                .label(verb)
                                .on_click(move |_, window, cx| on_click(window, cx)),
                        ),
                )
        });
    }

    /// Copy `items` from `src` into `dir` on side `to` through the transfer
    /// engine (progress shows in the transfer panel); with `mv`, delete
    /// each source once all of its files arrived.
    fn transfer(
        &mut self,
        src: Arc<dyn RemoteFs>,
        items: Vec<Item>,
        to: Side,
        dir: String,
        mv: bool,
        cx: &mut Context<Self>,
    ) {
        let dst = self.pane(to).fs.clone();
        let engine = self.backend.engine.clone();
        let src2 = src.clone();
        let job = self.backend.rt.spawn(async move {
            let mut failed: Vec<String> = Vec::new();
            let mut groups = Vec::new();
            for it in items {
                let target = hostwick_sftp::join(&dir, &it.name);
                if it.path == target {
                    failed.push(format!("{}: source and destination are the same", it.name));
                    continue;
                }
                if it.is_dir {
                    match engine
                        .enqueue_tree(
                            src2.clone(),
                            &it.path,
                            dst.clone(),
                            &target,
                            ExistingPolicy::Overwrite,
                        )
                        .await
                    {
                        Ok(ids) => groups.push((it, ids)),
                        Err(e) => failed.push(format!("{}: {e}", it.name)),
                    }
                } else {
                    let id = engine.enqueue(Job {
                        src: src2.clone(),
                        src_path: it.path.clone(),
                        dst: dst.clone(),
                        dst_path: target,
                        policy: ExistingPolicy::Overwrite,
                    });
                    groups.push((it, vec![id]));
                }
            }
            for (it, ids) in groups {
                let mut ok = true;
                for id in ids {
                    match engine.wait(id).await {
                        Some(TransferState::Done { .. } | TransferState::Skipped) => {}
                        Some(TransferState::Failed { error }) => {
                            ok = false;
                            failed.push(format!("{}: {error}", it.name));
                        }
                        _ => ok = false,
                    }
                }
                if ok && mv {
                    let entry = Entry {
                        name: it.name.clone(),
                        path: it.path.clone(),
                        kind: if it.is_dir {
                            hostwick_sftp::EntryKind::Dir
                        } else {
                            hostwick_sftp::EntryKind::File
                        },
                        size: 0,
                        modified: None,
                        modified_nanos: None,
                        permissions: None,
                        owner: None,
                        group: None,
                        uid: None,
                        gid: None,
                    };
                    if let Err(e) = remove_recursive(src2.as_ref(), &entry).await {
                        failed.push(format!("{}: copied, but not removed: {e}", it.name));
                    }
                }
            }
            failed
        });
        cx.spawn(async move |this, cx| {
            let failed = job
                .await
                .unwrap_or_else(|e| vec![format!("internal error: {e}")]);
            let _ = this.update(cx, |v, cx| {
                if !failed.is_empty() {
                    v.banner = Some((true, failed.join("; ")));
                }
                v.refresh(to, cx);
                if mv || Arc::ptr_eq(&src, &v.pane(to.other()).fs) {
                    v.refresh(to.other(), cx);
                }
            });
        })
        .detach();
    }

    fn download_with_picker(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Download here".into()),
        });
        let src = self.remote.fs.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(dirs))) = picked.await else {
                return;
            };
            let Some(dir) = dirs.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |v, cx| {
                v.transfer(src, items, Side::Local, path_to_slash(&dir), false, cx)
            });
        })
        .detach();
    }

    fn upload_with_picker(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Upload".into()),
        });
        let dir = self.remote.cwd.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |v, cx| v.copy_os_paths(paths, Side::Remote, dir, cx));
        })
        .detach();
    }

    /// Rows dropped on a pane (or a folder row): from the other pane =
    /// copy; within the same pane onto a folder = move there.
    fn drop_rows(
        &mut self,
        d: DragRows,
        to: Side,
        dir: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.dragged(&d);
        if entries.is_empty() {
            return;
        }
        if d.side != to {
            let src = self.pane(d.side).fs.clone();
            let items = entries.iter().map(Item::from).collect();
            self.transfer(src, items, to, dir, false, cx);
            return;
        }
        // Same pane: move into the folder (never into itself).
        let fs = self.pane(to).fs.clone();
        let moves: Vec<(String, String)> = entries
            .iter()
            .filter(|e| e.path != dir && !dir.starts_with(&format!("{}/", e.path)))
            .map(|e| (e.path.clone(), hostwick_sftp::join(&dir, &e.name)))
            .collect();
        if moves.is_empty() {
            return;
        }
        let job = self.backend.rt.spawn(async move {
            let mut failed = Vec::new();
            for (from, to) in moves {
                let res = match fs.stat(&to).await {
                    Ok(Some(_)) => Err(format!("{to} already exists")),
                    Ok(None) => fs.rename(&from, &to).await.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                if let Err(e) = res {
                    failed.push(e);
                }
            }
            failed
        });
        cx.spawn(async move |this, cx| {
            let failed = job.await.unwrap_or_default();
            let _ = this.update(cx, |v, cx| {
                if !failed.is_empty() {
                    v.banner = Some((true, failed.join("; ")));
                }
                v.refresh(to, cx);
            });
        })
        .detach();
    }

    /// Files dropped from the OS file manager.
    fn drop_external(
        &mut self,
        paths: Vec<PathBuf>,
        to: Side,
        dir: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_os_paths(paths, to, dir, cx);
    }

    fn copy_os_paths(
        &mut self,
        paths: Vec<PathBuf>,
        to: Side,
        dir: String,
        cx: &mut Context<Self>,
    ) {
        let items: Vec<Item> = paths
            .iter()
            .map(|p| Item {
                path: path_to_slash(p),
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                is_dir: p.is_dir(),
            })
            .filter(|i| !i.name.is_empty())
            .collect();
        if items.is_empty() {
            return;
        }
        let src: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
        self.transfer(src, items, to, dir, false, cx);
    }

    // ---- delete / rename / new -------------------------------------------

    fn confirm_delete(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.pane(side).targets();
        if entries.is_empty() {
            return;
        }
        let view = cx.entity().downgrade();
        let text = if entries.len() == 1 {
            let what = if entries[0].is_dir() {
                "folder and everything in it"
            } else {
                "file"
            };
            format!("Permanently delete the {what} “{}”?", entries[0].name)
        } else {
            format!(
                "Permanently delete {} items (folders with everything in them)?",
                entries.len()
            )
        };
        window.open_dialog(cx, move |dialog, _, _| {
            let (view, entries) = (view.clone(), entries.clone());
            let (enter_view, enter_entries) = (view.clone(), entries.clone());
            dialog
                .title("Delete")
                .child(text.clone())
                .on_ok(move |_, window, cx| {
                    window.close_dialog(cx);
                    let _ = enter_view.update(cx, |v, cx| {
                        v.refocus(window, cx);
                        v.delete(side, enter_entries.clone(), cx)
                    });
                    false
                })
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child({
                            let view = view.clone();
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = view.update(cx, |v, cx| v.refocus(window, cx));
                                })
                        })
                        .child(Button::new("delete").danger().label("Delete").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = view.update(cx, |v, cx| {
                                    v.refocus(window, cx);
                                    v.delete(side, entries.clone(), cx)
                                });
                            },
                        )),
                )
        });
    }

    fn delete(&mut self, side: Side, entries: Vec<Entry>, cx: &mut Context<Self>) {
        let fs = self.pane(side).fs.clone();
        let job = self.backend.rt.spawn(async move {
            let mut failed = Vec::new();
            for e in entries {
                if let Err(err) = remove_recursive(fs.as_ref(), &e).await {
                    failed.push(format!("{}: {err}", e.name));
                }
            }
            failed
        });
        cx.spawn(async move |this, cx| {
            let failed = job.await.unwrap_or_default();
            let _ = this.update(cx, |v, cx| {
                if !failed.is_empty() {
                    v.banner = Some((true, failed.join("; ")));
                }
                v.refresh(side, cx);
            });
        })
        .detach();
    }

    /// A one-field dialog; `submit` gets the trimmed text.
    #[allow(clippy::too_many_arguments)] // labels + the submit callback
    fn text_dialog(
        &mut self,
        title: &'static str,
        button: &'static str,
        initial: String,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        submit: impl Fn(&mut Self, String, &mut Window, &mut Context<Self>) -> Result<(), String>
        + 'static,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial)
        });
        let view = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        let submit = std::rc::Rc::new(submit);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (input2, view2, submit2) = (input.clone(), view.clone(), submit.clone());
            let go = std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let text = input2.read(cx).value().trim().to_owned();
                let res = view2
                    .update(cx, |v, cx| submit2(v, text, window, cx))
                    .unwrap_or(Ok(()));
                match res {
                    Ok(()) => {
                        window.close_dialog(cx);
                        let _ = view2.update(cx, |v, cx| v.refocus(window, cx));
                    }
                    Err(msg) => window.push_notification(Notification::warning(msg), cx),
                }
            });
            let view3 = view.clone();
            let (on_ok, on_click) = (go.clone(), go);
            dialog
                .title(title)
                .child(Input::new(&input))
                .on_ok(move |_, window, cx| {
                    on_ok(window, cx);
                    false
                })
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(Button::new("cancel").label("Cancel").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = view3.update(cx, |v, cx| v.refocus(window, cx));
                            },
                        ))
                        .child(
                            Button::new("ok")
                                .primary()
                                .label(button)
                                .on_click(move |_, window, cx| on_click(window, cx)),
                        ),
                )
        });
    }

    fn valid_name(name: &str) -> Result<(), String> {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            Err("Enter a name without “/”".into())
        } else {
            Ok(())
        }
    }

    fn new_folder(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        self.text_dialog(
            "New folder",
            "Create",
            String::new(),
            "Folder name",
            window,
            cx,
            move |v, name, _, cx| {
                Self::valid_name(&name)?;
                let path = hostwick_sftp::join(&v.pane(side).cwd, &name);
                v.run_op(side, move |fs| async move { fs.mkdir(&path).await }, cx);
                Ok(())
            },
        );
    }

    fn rename_dialog(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .pane(side)
            .current()
            .cloned()
            .or_else(|| self.pane(side).targets().into_iter().next())
        else {
            return;
        };
        self.text_dialog(
            "Rename",
            "Rename",
            entry.name.clone(),
            "New name",
            window,
            cx,
            move |v, name, _, cx| {
                Self::valid_name(&name)?;
                if name == entry.name {
                    return Ok(());
                }
                let from = entry.path.clone();
                let to = hostwick_sftp::join(&hostwick_sftp::parent(&from), &name);
                v.run_op(
                    side,
                    move |fs| async move {
                        // Refuse to overwrite: SFTP rename semantics differ.
                        if fs.stat(&to).await?.is_some() {
                            return Err(hostwick_sftp::FsError::Remote {
                                path: to,
                                message: format!("“{name}” already exists"),
                            });
                        }
                        fs.rename(&from, &to).await
                    },
                    cx,
                );
                Ok(())
            },
        );
    }

    fn new_symlink(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .pane(side)
            .current()
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let default = if target.is_empty() {
            String::new()
        } else {
            format!("{target} -> link-to-{target}")
        };
        self.text_dialog(
            "New symlink",
            "Create",
            default,
            "target -> link name",
            window,
            cx,
            move |v, text, _, cx| {
                let Some((target, link)) = text
                    .split_once("->")
                    .map(|(a, b)| (a.trim().to_owned(), b.trim().to_owned()))
                else {
                    return Err("Write it as “target -> link name”".into());
                };
                if target.is_empty() {
                    return Err("The target is empty".into());
                }
                Self::valid_name(&link)?;
                let link = hostwick_sftp::join(&v.pane(side).cwd, &link);
                v.run_op(
                    side,
                    move |fs| async move { fs.symlink(&target, &link).await },
                    cx,
                );
                Ok(())
            },
        );
    }

    /// Run one file-system call on the runtime, then show an error (if any)
    /// and refresh the pane.
    fn run_op<F, Fut>(&mut self, side: Side, op: F, cx: &mut Context<Self>)
    where
        F: FnOnce(Arc<dyn RemoteFs>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), hostwick_sftp::FsError>> + Send + 'static,
    {
        let fs = self.pane(side).fs.clone();
        let job = self.backend.rt.spawn(async move { op(fs).await });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                match res {
                    Ok(Err(e)) => v.banner = Some((true, e.to_string())),
                    Err(e) => v.banner = Some((true, format!("internal error: {e}"))),
                    Ok(Ok(())) => {}
                }
                v.refresh(side, cx);
            });
        })
        .detach();
    }

    fn properties(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .pane(side)
            .current()
            .cloned()
            .or_else(|| self.pane(side).targets().into_iter().next())
        else {
            return;
        };
        props::open(self, side, entry, window, cx);
    }

    // ---- edit -------------------------------------------------------------

    /// F4 / "Edit": remote files are downloaded and watched; local files
    /// just open in the editor.
    fn edit_selection(&mut self, side: Side, cx: &mut Context<Self>) {
        let files: Vec<Entry> = self
            .pane(side)
            .targets()
            .into_iter()
            .filter(|e| !e.is_dir())
            .collect();
        for e in files {
            match side {
                Side::Local => self.launch_editor(PathBuf::from(&e.path), cx),
                Side::Remote => self.start_edit(e, true, cx),
            }
        }
    }

    fn start_edit(&mut self, entry: Entry, watch: bool, cx: &mut Context<Self>) {
        if watch {
            // Already open: just bring the editor up again.
            if let Some(s) = self
                .edits
                .iter()
                .find(|s| s.remote_path == entry.path && !matches!(s.status, EditStatus::Error(_)))
            {
                let p = s.local_path.clone();
                self.launch_editor(p, cx);
                return;
            }
        }
        let id = self.next_edit;
        self.next_edit += 1;
        let local_path = self.edit_root.join(id.to_string()).join(&entry.name);
        let cancel = CancellationToken::new();
        if watch {
            self.edits.push(EditSession {
                id,
                name: entry.name.clone(),
                remote_path: entry.path.clone(),
                local_path: local_path.clone(),
                status: EditStatus::Downloading,
                cancel: cancel.clone(),
            });
        } else {
            self.open_only.insert(id);
        }
        self.backend.rt.spawn(edit::run(
            id,
            self.remote.fs.clone(),
            entry.path,
            local_path,
            watch,
            self.edit_tx.clone(),
            cancel,
        ));
        cx.notify();
    }

    fn on_edit_event(&mut self, ev: EditEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            EditEvent::Status(id, status) => {
                if let EditStatus::Error(e) = &status
                    && self.open_only.remove(&id)
                {
                    self.banner = Some((true, e.clone()));
                }
                let uploaded = matches!(status, EditStatus::Uploaded(_));
                if let Some(s) = self.edits.iter_mut().find(|s| s.id == id) {
                    s.status = status;
                }
                if uploaded {
                    // The listing's size and date changed.
                    self.refresh(Side::Remote, cx);
                }
            }
            EditEvent::Ready(id, path) => {
                if self.open_only.remove(&id) {
                    cx.open_with_system(&path);
                } else {
                    self.launch_editor(path, cx);
                }
            }
            EditEvent::Retarget(id, path) => {
                if let Some(s) = self.edits.iter_mut().find(|s| s.id == id) {
                    s.name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                    s.remote_path = path;
                }
                self.refresh(Side::Remote, cx);
            }
            EditEvent::Conflict { name, reply } => {
                self.conflict_dialog(name, reply, window, cx);
            }
        }
        cx.notify();
    }

    fn conflict_dialog(
        &mut self,
        name: String,
        reply: futures::channel::oneshot::Sender<ConflictChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reply = std::rc::Rc::new(std::cell::RefCell::new(Some(reply)));
        // Closing the dialog any other way drops the sender: "later".
        window.open_dialog(cx, move |dialog, _, _| {
            let b = |id: &'static str, label: &'static str, c: ConflictChoice| {
                let reply = reply.clone();
                Button::new(id).label(label).on_click(move |_, window, cx| {
                    if let Some(tx) = reply.borrow_mut().take() {
                        let _ = tx.send(c);
                    }
                    window.close_dialog(cx);
                })
            };
            dialog
                .title("Changed on the server")
                .w(px(600.0))
                .child(format!(
                    "“{name}” was changed on the server since you opened it. What should happen with your saved version?"
                ))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(b("later", "Decide later", ConflictChoice::Later))
                        .child(b("theirs", "Download theirs", ConflictChoice::DownloadTheirs))
                        .child(b("copy", "Save as copy", ConflictChoice::SaveCopy))
                        .child(b("overwrite", "Overwrite", ConflictChoice::Overwrite).danger()),
                )
        });
    }

    fn launch_editor(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let template = Settings::get(cx).external_editor.clone();
        if template.trim().is_empty() {
            cx.open_with_system(&path);
            return;
        }
        let Some((program, args)) = editor_command(&template, &path) else {
            cx.open_with_system(&path);
            return;
        };
        let job = self.backend.rt.spawn_blocking(move || {
            match std::process::Command::new(&program).args(&args).spawn() {
                // Reap it in the background so it never lingers as a zombie.
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                    Ok(())
                }
                Err(e) => Err(format!("could not start the editor “{program}”: {e}")),
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = job.await {
                let _ = this.update(cx, |v, cx| {
                    v.banner = Some((true, e));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn stop_edit(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        self.edits.retain(|s| {
            let stop = id.is_none_or(|i| i == s.id);
            if stop {
                s.cancel.cancel();
            }
            !stop
        });
        cx.notify();
    }

    // ---- root ---------------------------------------------------------------

    fn toggle_root(&mut self, cx: &mut Context<Self>) {
        if self.switching_root {
            return;
        }
        self.switching_root = true;
        let elevate = !self.elevated;
        let job = self.backend.rt.spawn(
            self.backend
                .clone()
                .open_remote(self.profile.clone(), elevate),
        );
        let cwd = self.remote.cwd.clone();
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                v.switching_root = false;
                match res {
                    Ok(Ok(session)) => {
                        v.remote.fs = session.fs;
                        v.elevated = session.elevated;
                        v.banner = None;
                        // Same folder, now with the new rights.
                        v.navigate(
                            Side::Remote,
                            if cwd.is_empty() { ".".into() } else { cwd },
                            cx,
                        );
                    }
                    Ok(Err(e)) => v.banner = Some((true, format!("{e:#}"))),
                    Err(e) => v.banner = Some((true, format!("internal error: {e}"))),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ---- keep up to date ------------------------------------------------------

    /// WinSCP's "Keep remote directory up to date": changes under `local`
    /// upload into `remote` until stopped (never deletes remote files).
    fn start_sync(
        &mut self,
        local: String,
        remote: String,
        excludes: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();
        let cancel = CancellationToken::new();
        let (engine, fs) = (self.backend.engine.clone(), self.remote.fs.clone());
        let (src_root, dst_root, token) = (local.clone(), remote.clone(), cancel.clone());
        self.backend.rt.spawn(async move {
            let src: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
            let opts = SyncOptions {
                mode: SyncMode::Update,
                excludes,
                dry_run: false,
            };
            hostwick_transfer::watch(
                &engine,
                src,
                &src_root,
                fs,
                &dst_root,
                &opts,
                std::time::Duration::from_secs(2),
                token,
                |r| {
                    let msg = match r {
                        Ok(r) if r.is_noop() => "up to date".to_owned(),
                        Ok(r) => format!(
                            "synced {} file(s), {}{}",
                            r.copied.len(),
                            human_size(r.bytes),
                            if r.failed.is_empty() {
                                String::new()
                            } else {
                                format!(", {} failed", r.failed.len())
                            }
                        ),
                        Err(e) => format!("error (retrying): {e}"),
                    };
                    let _ = tx.unbounded_send(msg);
                },
            )
            .await;
        });
        let ui = cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.next().await {
                let alive = this
                    .update(cx, |v, cx| {
                        let changed = msg != "up to date";
                        if let Some(s) = v.sync.as_mut() {
                            s.status = msg;
                        }
                        if changed {
                            v.refresh(Side::Remote, cx);
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        self.sync = Some(ActiveSync {
            local,
            remote,
            cancel,
            status: "starting…".into(),
            _ui: ui,
        });
        cx.notify();
    }

    fn set_show_local(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.show_local = show;
        let _ = self
            .backend
            .store
            .set_setting(SHOW_LOCAL_KEY, if show { "true" } else { "false" });
        if !show {
            self.activate(Side::Remote, window, cx);
        }
        cx.notify();
    }

    // ---- rendering ------------------------------------------------------------

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let n_edits = self.edits.len();
        let edits_label = if n_edits == 0 {
            "Edits".to_owned()
        } else {
            let busy = self
                .edits
                .iter()
                .find(|e| {
                    matches!(
                        e.status,
                        EditStatus::Uploading | EditStatus::Conflict | EditStatus::Error(_)
                    )
                })
                .or_else(|| self.edits.last())
                .map(|e| e.status.describe())
                .unwrap_or_default();
            format!("Edits ({n_edits}): {busy}")
        };
        let has_error = self
            .edits
            .iter()
            .any(|e| matches!(e.status, EditStatus::Error(_) | EditStatus::Conflict));
        let view = cx.entity().downgrade();
        let edits_menu = Button::new("edits")
            .small()
            .label(edits_label)
            .disabled(n_edits == 0)
            .when(has_error, |b| b.warning())
            .tooltip("Files open in an editor; saves upload automatically")
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, cx| {
                let Some(v) = view.upgrade() else {
                    return menu;
                };
                let rows: Vec<(u64, String, String)> = v
                    .read(cx)
                    .edits
                    .iter()
                    .map(|e| (e.id, e.remote_path.clone(), e.status.describe()))
                    .collect();
                let mut menu = menu;
                for (id, path, status) in rows {
                    let view = view.clone();
                    menu = menu
                        .item(PopupMenuItem::new(format!("{path} — {status}")).disabled(true))
                        .item(
                            PopupMenuItem::new("    Stop watching").on_click(move |_, _, cx| {
                                let _ = view.update(cx, |v, cx| v.stop_edit(Some(id), cx));
                            }),
                        );
                }
                let view = view.clone();
                menu.separator()
                    .item(
                        PopupMenuItem::new("Stop watching all").on_click(move |_, _, cx| {
                            let _ = view.update(cx, |v, cx| v.stop_edit(None, cx));
                        }),
                    )
            });

        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("two-panes")
                    .small()
                    .icon(IconName::PanelLeft)
                    .label(if self.show_local {
                        "Hide local"
                    } else {
                        "Show local"
                    })
                    .selected(self.show_local)
                    .tooltip("Show or hide the local computer's files")
                    .on_click(cx.listener(|v, _, w, cx| {
                        let show = !v.show_local;
                        v.set_show_local(show, w, cx)
                    })),
            )
            .when(!self.show_local, |d| {
                d.child(
                    Button::new("upload")
                        .small()
                        .primary()
                        .label("Upload…")
                        .on_click(cx.listener(|v, _, _, cx| v.upload_with_picker(cx))),
                )
            })
            .child(
                Button::new("sync")
                    .small()
                    .label("Synchronize…")
                    .tooltip("Compare folders, review the changes, then run them")
                    .on_click(cx.listener(|v, _, w, cx| syncdlg::open(v, w, cx))),
            )
            .child(
                Button::new("keep")
                    .small()
                    .label(if self.sync.is_some() {
                        "Stop keeping up to date"
                    } else {
                        "Keep up to date…"
                    })
                    .when(self.sync.is_some(), |b| b.warning())
                    .tooltip(
                        "Upload changes from a local folder into a remote folder automatically",
                    )
                    .on_click(cx.listener(|v, _, w, cx| {
                        if v.sync.take().is_some() {
                            cx.notify();
                        } else {
                            syncdlg::open(v, w, cx);
                        }
                    })),
            )
            .child(edits_menu)
            .when_some(self.link.clone(), |d, term| {
                d.child(
                    Button::new("follow")
                        .small()
                        .icon(IconName::RefreshCw)
                        .label(if self.follow {
                            "Following terminal"
                        } else {
                            "Follow terminal"
                        })
                        .selected(self.follow)
                        .when(self.follow, |b| b.primary())
                        .tooltip(format!(
                            "Show the folder the terminal “{term}” is in, as it changes"
                        ))
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_follow(cx))),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("root")
                    .small()
                    .icon(IconName::TriangleAlert)
                    .label(match (self.switching_root, self.elevated) {
                        (true, _) => "Switching…",
                        (false, true) => "Root: on",
                        (false, false) => "Reopen as root",
                    })
                    .when(self.elevated, |b| b.danger())
                    .disabled(self.switching_root)
                    .tooltip("Browse the server as root (or the host's sudo user) through sudo")
                    .on_click(cx.listener(|v, _, _, cx| v.toggle_root(cx))),
            )
            .into_any_element()
    }
}

impl Render for FilesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let toolbar = self.render_toolbar(cx);
        let local = self
            .show_local
            .then(|| self.render_pane(Side::Local, window, cx));
        let remote = self.render_pane(Side::Remote, window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(toolbar)
            .when_some(self.banner.clone(), |d, (is_err, text)| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(12.0))
                        .py(px(5.0))
                        .text_size(px(13.0))
                        .text_color(if is_err {
                            theme.danger
                        } else {
                            theme.foreground
                        })
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().flex_1().child(text))
                        .child(
                            Button::new("dismiss")
                                .xsmall()
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.banner = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .children(local)
                    .child(remote),
            )
            .when_some(self.sync.as_ref(), |d, s| {
                d.child(
                    div()
                        .px(px(10.0))
                        .py(px(3.0))
                        .text_size(px(12.0))
                        .text_color(theme.muted_foreground)
                        .border_t_1()
                        .border_color(theme.border)
                        .child(format!(
                            "Keeping {} → {} up to date: {}",
                            s.local, s.remote, s.status
                        )),
                )
            })
    }
}
