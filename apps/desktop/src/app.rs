//! Root view: session sidebar, tabs (terminals with split panes, file
//! browsers), broadcast input, the transfer queue, saved-password and
//! host-key dialogs, auto-reconnect, restored tabs and settings sync.

mod menus;
mod prompts;
mod render;
mod sync;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::notify::{Toast, ToastExt as _};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use futures::channel::oneshot;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use hostwick_core::{HostProfile, LogVars, ProfileId, ProfileStore, expand_log_path};
use hostwick_term::TransportKind;
use hostwick_transfer::{TransferEvent, TransferId, TransferState};

use crate::backend::{Backend, Prompt, quick_profile};
use crate::files::FilesView;
use crate::layout::{Dir, Node, PaneId, SavedKind, SavedNode, SavedPane, SavedSession, SavedTab};
use crate::settings::Settings;
use crate::terminal::{GridSize, TerminalEvent, TerminalView};

pub(crate) enum PaneBody {
    /// Restored at startup; connects when its tab is first shown.
    Pending,
    Connecting,
    Failed(String),
    Terminal(Entity<TerminalView>),
    Files(Entity<FilesView>),
}

pub(crate) struct Pane {
    id: u64,
    profile: HostProfile,
    /// Quick-connect text for unsaved hosts (to restore the tab).
    quick: Option<String>,
    body: PaneBody,
    /// Files pane: the terminal pane whose folder it follows.
    follows: Option<u64>,
    /// Files pane: open at this remote folder.
    start_dir: Option<String>,
}

impl PaneId for Pane {
    fn pane_id(&self) -> u64 {
        self.id
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabKind {
    Terminal,
    Files,
}

pub(crate) struct Tab {
    id: u64,
    kind: TabKind,
    root: Node<Pane>,
    focused: u64,
}

impl Tab {
    fn focused_pane(&self) -> Option<&Pane> {
        self.root
            .find(self.focused)
            .or_else(|| self.root.leaves().into_iter().next())
    }

    fn profile(&self) -> Option<&HostProfile> {
        self.focused_pane().map(|p| &p.profile)
    }

    fn panes(&self) -> Vec<&Pane> {
        self.root.leaves()
    }
}

struct TransferRow {
    id: TransferId,
    label: String,
    done: u64,
    total: u64,
    rate: f64,
    state: Option<TransferState>,
}

enum SidebarRow {
    Folder(String),
    Profile(usize),
}

pub struct HostwickApp {
    backend: Arc<Backend>,
    profiles: Vec<HostProfile>,
    selected: Option<ProfileId>,
    search: Entity<InputState>,
    quick: Entity<InputState>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    next_id: u64,
    broadcast: bool,
    transfers: Vec<TransferRow>,
    prompts: VecDeque<Prompt>,
    prompt_open: bool,
    /// Recently shown notices, for de-duplication and rate limiting.
    notices: HashMap<String, Instant>,
    notice_times: VecDeque<Instant>,
    sync: sync::SyncState,
    /// Root focus, so app shortcuts work before any terminal has focus.
    focus: FocusHandle,
    /// The saved-hosts list (arrow keys, Enter, F2, Del, Menu).
    list_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    /// The tab bar (arrow keys switch tabs).
    tabs_focus: FocusHandle,
    /// Where keyboard-opened context menus appear.
    host_anchor: crate::a11y::MenuAnchor,
    tab_anchor: crate::a11y::MenuAnchor,
    /// Language the window was last drawn in (placeholders follow it).
    lang: &'static str,
    /// The tab layout last written, to skip writing the same again.
    saved_tabs: RefCell<String>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

/// Settings key of the settings-sync folder.
pub fn sync_folder_key() -> &'static str {
    sync::FOLDER_KEY
}

/// Settings key of the last settings-sync result.
pub fn sync_last_key() -> &'static str {
    sync::LAST_KEY
}

/// The share of split `id`, if it is in `node`.
fn split_ratio(node: &Node<Pane>, id: u64) -> Option<f32> {
    match node {
        Node::Split {
            id: s, ratio, a, b, ..
        } => {
            if *s == id {
                Some(*ratio)
            } else {
                split_ratio(a, id).or_else(|| split_ratio(b, id))
            }
        }
        _ => None,
    }
}

/// The app icon, decoded once and shared.
fn logo() -> Arc<Image> {
    static LOGO: std::sync::OnceLock<Arc<Image>> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../packaging/icons/hostwick.png").to_vec(),
        ))
    })
    .clone()
}

/// One-shot reply that the dialog's (re-runnable) closures can fire once.
type ReplySlot<T> = Rc<RefCell<Option<oneshot::Sender<T>>>>;

fn slot<T>(tx: oneshot::Sender<T>) -> ReplySlot<T> {
    Rc::new(RefCell::new(Some(tx)))
}

fn answer<T>(s: &ReplySlot<T>, v: T) {
    if let Some(tx) = s.borrow_mut().take() {
        let _ = tx.send(v);
    }
}

impl HostwickApp {
    pub fn new(
        backend: Arc<Backend>,
        prompts_rx: UnboundedReceiver<Prompt>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("sidebar-search-placeholder")));
        let quick = cx.new(|cx| InputState::new(window, cx).placeholder("user@host:port  ⏎"));
        let mut subs = vec![
            cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
            // An element that had keyboard focus disappeared (a host-editor
            // section switched, a pane closed): give focus to its closest
            // surviving ancestor (the dialog, the view) so Escape, Enter and
            // the shortcuts keep working; else to the window.
            cx.on_focus_lost(window, |this, window, cx| {
                match window.focus_lost_restore_target(cx) {
                    Some(h) => h.focus(window, cx),
                    None => this.focus.focus(window, cx),
                }
            }),
        ];
        subs.push(cx.subscribe_in(
            &quick,
            window,
            |this: &mut Self, input, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let text = input.read(cx).value().trim().to_owned();
                    if text.is_empty() {
                        return;
                    }
                    match quick_profile(&text) {
                        Ok(p) => this.open_tab_quick(p, Some(text), TabKind::Terminal, window, cx),
                        Err(e) => window.toast(Toast::error(e.to_string()), cx),
                    }
                }
            },
        ));

        // Prompts from the SSH layer → dialogs, one at a time.
        let prompt_task = cx.spawn_in(window, async move |this, cx| {
            let mut rx = prompts_rx;
            while let Some(p) = rx.next().await {
                if this
                    .update_in(cx, |app, window, cx| app.enqueue_prompt(p, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        // Transfer engine events (Tokio broadcast) → UI.
        let (tx, mut rx) = unbounded::<TransferEvent>();
        let mut events = backend.engine.subscribe();
        backend.rt.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(ev) => {
                        if tx.unbounded_send(ev).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
        let transfer_task = cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |app, cx| app.on_transfer(ev, cx)).is_err() {
                    break;
                }
            }
        });

        // Housekeeping every 15 s: vault auto-lock, settings sync schedule.
        let tick_task = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(15))
                    .await;
                if this
                    .update_in(cx, |app, window, cx| app.housekeeping(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut app = Self {
            backend,
            profiles: Vec::new(),
            selected: None,
            search,
            quick,
            tabs: Vec::new(),
            active: None,
            next_id: 1,
            broadcast: false,
            transfers: Vec::new(),
            prompts: VecDeque::new(),
            prompt_open: false,
            notices: HashMap::new(),
            notice_times: VecDeque::new(),
            sync: sync::SyncState::load(),
            focus: cx.focus_handle(),
            list_focus: cx.focus_handle().tab_stop(true),
            list_scroll: UniformListScrollHandle::new(),
            tabs_focus: cx.focus_handle().tab_stop(true),
            host_anchor: Default::default(),
            tab_anchor: Default::default(),
            lang: crate::i18n::current(),
            saved_tabs: RefCell::default(),
            _tasks: vec![prompt_task, transfer_task, tick_task],
            _subs: subs,
        };
        app.reload_profiles();
        app.focus.focus(window, cx);
        app.restore_tabs(window, cx);
        app.startup_sync(window, cx);
        app
    }

    fn reload_profiles(&mut self) {
        self.profiles = self.backend.store.list().unwrap_or_default();
        self.profiles.sort_by_key(|p| p.path().to_lowercase());
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn housekeeping(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let minutes = Settings::get(cx).vault_auto_lock_minutes;
        if self.backend.vault.lock_if_idle(minutes) {
            cx.notify();
        }
        self.sync_tick(window, cx);
        // Folders shown in file browsers change without a tab event.
        self.persist_tabs(cx);
    }

    // ---- tabs and panes -------------------------------------------------------

    fn new_pane(&mut self, profile: HostProfile, quick: Option<String>) -> Pane {
        Pane {
            id: self.next_id(),
            profile,
            quick,
            body: PaneBody::Pending,
            follows: None,
            start_dir: None,
        }
    }

    pub(crate) fn open_tab(
        &mut self,
        profile: HostProfile,
        kind: TabKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let quick = self.quick_text_for(&profile);
        self.open_tab_quick(profile, quick, kind, window, cx);
    }

    /// The quick-connect text of an unsaved host already open in a tab.
    fn quick_text_for(&self, profile: &HostProfile) -> Option<String> {
        self.tabs
            .iter()
            .flat_map(|t| t.panes())
            .find(|p| p.profile.id == profile.id)
            .and_then(|p| p.quick.clone())
    }

    fn open_tab_quick(
        &mut self,
        profile: HostProfile,
        quick: Option<String>,
        kind: TabKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.new_pane(profile, quick);
        self.push_tab(pane, kind, window, cx);
    }

    fn push_tab(&mut self, pane: Pane, kind: TabKind, window: &mut Window, cx: &mut Context<Self>) {
        let pane_id = pane.id;
        let id = self.next_id();
        self.tabs.push(Tab {
            id,
            kind,
            root: Node::Leaf(pane),
            focused: pane_id,
        });
        self.active = Some(self.tabs.len() - 1);
        self.connect_pane(pane_id, window, cx);
        self.persist_tabs(cx);
        cx.notify();
    }

    /// (tab index, pane) for a pane id.
    fn locate(&self, pane_id: u64) -> Option<(usize, &Pane)> {
        self.tabs
            .iter()
            .enumerate()
            .find_map(|(i, t)| t.root.find(pane_id).map(|p| (i, p)))
    }

    fn pane_mut(&mut self, pane_id: u64) -> Option<&mut Pane> {
        self.tabs.iter_mut().find_map(|t| t.root.find_mut(pane_id))
    }

    fn pane_of_terminal(&self, view: EntityId) -> Option<u64> {
        self.tabs
            .iter()
            .flat_map(|t| t.panes())
            .find_map(|p| match &p.body {
                PaneBody::Terminal(v) if v.entity_id() == view => Some(p.id),
                _ => None,
            })
    }

    fn all_terminals(&self) -> Vec<Entity<TerminalView>> {
        self.tabs
            .iter()
            .flat_map(|t| t.panes())
            .filter_map(|p| match &p.body {
                PaneBody::Terminal(v) => Some(v.clone()),
                _ => None,
            })
            .collect()
    }

    /// Connect (or reconnect) a pane. A terminal that is already open keeps
    /// its scrollback and gets the new session.
    fn connect_pane(&mut self, pane_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tab_ix, pane)) = self.locate(pane_id) else {
            return;
        };
        let kind = self.tabs[tab_ix].kind;
        let profile = pane.profile.clone();
        let backend = self.backend.clone();
        let rt = backend.handle();
        match (kind, &pane.body) {
            (TabKind::Terminal, PaneBody::Terminal(view)) => {
                let view = view.clone();
                let size = view.update(cx, |v, cx| {
                    v.begin_manual_reconnect(cx);
                    v.grid_size()
                });
                let job = rt.spawn(backend.open_shell(
                    profile.clone(),
                    u32::try_from(size.cols).unwrap_or(120),
                    u32::try_from(size.rows).unwrap_or(32),
                ));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        match result {
                            Ok(Ok(session)) => view.update(cx, |v, cx| {
                                v.reattach(session, cx);
                                v.attach_profile(&profile, cx);
                            }),
                            Ok(Err(e)) => {
                                let fatal = crate::backend::is_fatal(&e);
                                view.update(cx, |v, cx| {
                                    v.reconnect_failed(format!("{e:#}"), fatal, cx)
                                });
                            }
                            Err(e) => view.update(cx, |v, cx| {
                                v.reconnect_failed(
                                    t!("error-internal", error = e.to_string()).into(),
                                    true,
                                    cx,
                                )
                            }),
                        }
                        let _ = window;
                        app.persist_tabs(cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            (TabKind::Terminal, _) => {
                if let Some(p) = self.pane_mut(pane_id) {
                    p.body = PaneBody::Connecting;
                }
                let initial = GridSize {
                    cols: 120,
                    rows: 32,
                };
                let title: SharedString = crate::terminal::tab_title(&profile).into();
                let job = rt.spawn(backend.open_shell(profile.clone(), 120, 32));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        let body = match result {
                            Ok(Ok(shell)) => {
                                let view = cx.new(|cx| {
                                    TerminalView::new(title, shell, initial, &rt, window, cx)
                                });
                                let sub = cx.subscribe_in(&view, window, Self::on_terminal_event);
                                app._subs.push(sub);
                                app.auto_session_log(&profile, &view, cx);
                                view.update(cx, |v, cx| v.attach_profile(&profile, cx));
                                let focused = app
                                    .active
                                    .and_then(|i| app.tabs.get(i))
                                    .is_some_and(|t| t.focused == pane_id);
                                if focused {
                                    view.read(cx).focus_handle(cx).focus(window, cx);
                                }
                                PaneBody::Terminal(view)
                            }
                            Ok(Err(e)) => PaneBody::Failed(format!("{e:#}")),
                            Err(e) => {
                                PaneBody::Failed(t!("error-internal", error = e.to_string()).into())
                            }
                        };
                        if let Some(p) = app.pane_mut(pane_id) {
                            p.body = body;
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            (TabKind::Files, _) => {
                let start = pane.start_dir.clone();
                let link = pane
                    .follows
                    .and_then(|t| self.locate(t))
                    .map(|(_, p)| crate::terminal::tab_title(&p.profile));
                if let Some(p) = self.pane_mut(pane_id) {
                    p.body = PaneBody::Connecting;
                }
                let job = rt.spawn(backend.open_files(profile.clone()));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        let body = match result {
                            Ok(Ok(session)) => {
                                let backend = app.backend.clone();
                                let view = cx.new(|cx| {
                                    let mut v =
                                        FilesView::new(session, profile, backend, window, cx);
                                    if let Some(dir) = start {
                                        v.navigate_remote(dir, cx);
                                    }
                                    v.set_link(link, cx);
                                    v
                                });
                                PaneBody::Files(view)
                            }
                            Ok(Err(e)) => PaneBody::Failed(format!("{e:#}")),
                            Err(e) => {
                                PaneBody::Failed(t!("error-internal", error = e.to_string()).into())
                            }
                        };
                        if let Some(p) = app.pane_mut(pane_id) {
                            p.body = body;
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    /// Reconnect every pane of a tab (tab menu, Ctrl+Shift+R reconnects the
    /// focused pane only).
    fn reconnect_focused(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self
            .tabs
            .get(ix)
            .and_then(|t| t.focused_pane())
            .map(|p| p.id)
        {
            self.active = Some(ix);
            self.connect_pane(pane, window, cx);
            cx.notify();
        }
    }

    pub(crate) fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.tabs.remove(ix);
            self.active = match self.active {
                _ if self.tabs.is_empty() => None,
                Some(a) if a >= self.tabs.len() => Some(self.tabs.len() - 1),
                Some(a) if a > ix => Some(a - 1),
                other => other,
            };
            self.unlink_dead_follows();
            self.persist_tabs(cx);
            cx.notify();
        }
    }

    /// Close the focused pane; the last pane closes the tab.
    fn close_focused_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        if tab.root.count() <= 1 {
            self.close_tab(ix, cx);
            return;
        }
        let gone = tab.focused;
        tab.root.remove(gone);
        tab.focused = tab.root.leaves().first().map(|p| p.id).unwrap_or(0);
        self.unlink_dead_follows();
        self.focus_active(window, cx);
        self.persist_tabs(cx);
        cx.notify();
    }

    /// Files panes stop following terminals that are gone.
    fn unlink_dead_follows(&mut self) {
        let live: Vec<u64> = self
            .tabs
            .iter()
            .flat_map(|t| t.panes())
            .map(|p| p.id)
            .collect();
        for t in &mut self.tabs {
            for p in t.root.leaves_mut() {
                if p.follows.is_some_and(|f| !live.contains(&f)) {
                    p.follows = None;
                }
            }
        }
    }

    /// Split the focused terminal pane, opening the same host beside it.
    fn split_focused(&mut self, vertical: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        if tab.kind != TabKind::Terminal {
            window.toast(Toast::info(t!("split-only-terminals")), cx);
            return;
        }
        let Some(src) = tab.focused_pane() else {
            return;
        };
        let (target, profile, quick) = (src.id, src.profile.clone(), src.quick.clone());
        let pane = self.new_pane(profile, quick);
        let new_id = pane.id;
        let split_id = self.next_id();
        let tab = &mut self.tabs[ix];
        if tab.root.split(target, pane, vertical, split_id).is_ok() {
            tab.focused = new_id;
            self.connect_pane(new_id, window, cx);
        }
        self.persist_tabs(cx);
        cx.notify();
    }

    fn focus_pane(&mut self, pane_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.root.find(pane_id).is_some())
        {
            self.tabs[ix].focused = pane_id;
            if self.active == Some(ix) {
                self.focus_active(window, cx);
            }
            cx.notify();
        }
    }

    fn move_focus(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.active.and_then(|i| self.tabs.get_mut(i)) else {
            return false;
        };
        match tab.root.neighbor(tab.focused, dir) {
            Some(n) => {
                tab.focused = n;
                self.focus_active(window, cx);
                cx.notify();
                true
            }
            None => false,
        }
    }

    /// A divider was dragged: resize live, save when the drag ends.
    pub(crate) fn set_split_ratio(&mut self, split: u64, ratio: f32, cx: &mut Context<Self>) {
        if let Some(t) = self.active.and_then(|i| self.tabs.get_mut(i))
            && t.root.set_ratio(split, ratio)
        {
            cx.notify();
        }
    }

    /// Move a divider by `delta` (screen-reader increment/decrement).
    pub(crate) fn nudge_split(&mut self, split: u64, delta: f32, cx: &mut Context<Self>) {
        let current = self
            .active
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| split_ratio(&t.root, split));
        if let Some(r) = current {
            self.set_split_ratio(split, r + delta, cx);
            self.persist_tabs(cx);
        }
    }

    /// Alt+Shift+arrows: move the divider next to the focused pane.
    fn resize_focused(&mut self, dir: Dir, cx: &mut Context<Self>) -> bool {
        let Some(t) = self.active.and_then(|i| self.tabs.get_mut(i)) else {
            return false;
        };
        let (vertical, delta) = match dir {
            Dir::Left => (false, -0.05),
            Dir::Right => (false, 0.05),
            Dir::Up => (true, -0.05),
            Dir::Down => (true, 0.05),
        };
        let done = t.root.nudge(t.focused, vertical, delta);
        if done {
            self.persist_tabs(cx);
            cx.notify();
        }
        done
    }

    /// Keyboard focus to the active tab's focused pane.
    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self
            .active
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.focused_pane())
            .map(|p| &p.body)
        {
            Some(PaneBody::Terminal(v)) => v.read(cx).focus_handle(cx).focus(window, cx),
            Some(PaneBody::Files(v)) => v.read(cx).focus_handle(cx).focus(window, cx),
            _ => {}
        }
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.active = Some(ix);
        // Restored tabs connect when first shown.
        let pending: Vec<u64> = self.tabs[ix]
            .panes()
            .iter()
            .filter(|p| matches!(p.body, PaneBody::Pending))
            .map(|p| p.id)
            .collect();
        for id in pending {
            self.connect_pane(id, window, cx);
        }
        self.focus_active(window, cx);
        self.persist_tabs(cx);
        cx.notify();
    }

    /// Open a file browser for a terminal pane, at its current folder and
    /// following it.
    fn open_files_here(&mut self, term_pane: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, pane)) = self.locate(term_pane) else {
            return;
        };
        let cwd = match &pane.body {
            PaneBody::Terminal(v) => v.read(cx).cwd(),
            _ => None,
        };
        let mut files = Pane {
            id: 0,
            profile: pane.profile.clone(),
            quick: pane.quick.clone(),
            body: PaneBody::Pending,
            follows: Some(term_pane),
            start_dir: cwd.clone(),
        };
        files.id = self.next_id();
        if cwd.is_none() {
            window.toast(Toast::info(t!("files-follow-no-folder")), cx);
        }
        self.push_tab(files, TabKind::Files, window, cx);
    }

    fn on_terminal_event(
        &mut self,
        source: &Entity<TerminalView>,
        ev: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane_id = self.pane_of_terminal(source.entity_id());
        match ev {
            TerminalEvent::UserInput(bytes) if self.broadcast => {
                for v in self.all_terminals() {
                    if v.entity_id() != source.entity_id() {
                        v.read(cx).write_raw(bytes);
                    }
                }
            }
            TerminalEvent::UserInput(_) => {}
            TerminalEvent::TitleChanged | TerminalEvent::LoggingChanged => cx.notify(),
            TerminalEvent::Exited => {
                let auto = Settings::get(cx).auto_reconnect;
                source.update(cx, |v, cx| {
                    let kind = v.transport();
                    if auto
                        && v.was_dropped()
                        && v.reconnect_state().is_none()
                        && matches!(kind, TransportKind::Ssh | TransportKind::Telnet)
                    {
                        v.schedule_reconnect(0, None, cx);
                    }
                });
                cx.notify();
            }
            TerminalEvent::CwdChanged(dir) => {
                if let Some(term) = pane_id {
                    for t in &self.tabs {
                        for p in t.panes() {
                            if p.follows == Some(term)
                                && let PaneBody::Files(v) = &p.body
                            {
                                let dir = dir.clone();
                                v.update(cx, |v, cx| {
                                    if v.following() {
                                        v.navigate_remote(dir, cx);
                                    }
                                });
                            }
                        }
                    }
                }
                cx.notify();
            }
            TerminalEvent::Reconnect => {
                if let Some(id) = pane_id {
                    self.connect_pane(id, window, cx);
                }
            }
            TerminalEvent::Duplicate => {
                if let Some((_, p)) = pane_id.and_then(|id| self.locate(id)) {
                    let (profile, quick) = (p.profile.clone(), p.quick.clone());
                    self.open_tab_quick(profile, quick, TabKind::Terminal, window, cx);
                }
            }
            TerminalEvent::OpenFiles => {
                if let Some((_, p)) = pane_id.and_then(|id| self.locate(id)) {
                    let (profile, quick) = (p.profile.clone(), p.quick.clone());
                    self.open_tab_quick(profile, quick, TabKind::Files, window, cx);
                }
            }
            TerminalEvent::OpenFilesHere => {
                if let Some(id) = pane_id {
                    self.open_files_here(id, window, cx);
                }
            }
            TerminalEvent::Split(vertical) => {
                if let Some(id) = pane_id {
                    self.focus_pane(id, window, cx);
                    self.split_focused(*vertical, window, cx);
                }
            }
        }
    }

    /// Start a session log when the host (or the global setting) asks for one.
    fn auto_session_log(
        &self,
        profile: &HostProfile,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) {
        let settings = Settings::get(cx).clone();
        let (template, mode, append) = match &profile.session_log {
            Some(l) => (l.path.clone(), l.mode, l.append),
            None if settings.log_all_sessions => (
                "&S-&Y&M&D-&T.log".to_owned(),
                settings.session_log_mode,
                true,
            ),
            None => return,
        };
        let now = chrono::Local::now();
        use chrono::{Datelike as _, Timelike as _};
        let user = profile.user_or_local();
        let vars = LogVars {
            host: &profile.host,
            port: profile.port,
            user: &user,
            session: &profile.name,
            time: (
                now.year(),
                now.month(),
                now.day(),
                now.hour(),
                now.minute(),
                now.second(),
            ),
        };
        let rel = std::path::PathBuf::from(expand_log_path(&template, &vars));
        let path = if rel.is_absolute() {
            rel
        } else {
            settings.session_log_dir(&self.backend.paths).join(rel)
        };
        let header = format!(
            "{}  {}@{}:{}",
            now.format("%Y-%m-%d %H:%M:%S"),
            user,
            profile.host,
            profile.port
        );
        view.update(cx, |v, cx| {
            if let Err(e) = v.start_log(&path, mode, append, &header, cx) {
                tracing::warn!(error = %e, path = %path.display(), "cannot start session log");
            }
        });
    }

    // ---- restore tabs -----------------------------------------------------------

    fn saved_pane(p: &Pane, cx: &App) -> SavedPane {
        let dir = match &p.body {
            PaneBody::Files(v) => Some(v.read(cx).remote_dir().to_owned()),
            _ => p.start_dir.clone(),
        }
        .filter(|d| !d.is_empty());
        SavedPane {
            profile: p.quick.is_none().then_some(p.profile.id),
            quick: p.quick.clone(),
            id: Some(p.id),
            follows: p.follows,
            dir,
        }
    }

    /// Remember the open tabs (when "Restore tabs" is on).
    pub(crate) fn persist_tabs(&self, cx: &App) {
        let json = if Settings::get(cx).restore_tabs {
            SavedSession {
                tabs: self
                    .tabs
                    .iter()
                    .map(|t| SavedTab {
                        kind: match t.kind {
                            TabKind::Terminal => SavedKind::Terminal,
                            TabKind::Files => SavedKind::Files,
                        },
                        root: crate::layout::save_tree(&t.root, &|p| Self::saved_pane(p, cx)),
                    })
                    .collect(),
                active: self.active,
            }
            .to_json()
        } else {
            String::new()
        };
        if *self.saved_tabs.borrow() == json {
            return;
        }
        match self
            .backend
            .store
            .set_setting(crate::layout::TABS_KEY, &json)
        {
            Ok(()) => *self.saved_tabs.borrow_mut() = json,
            Err(e) => tracing::warn!(error = %e, "could not save open tabs"),
        }
    }

    fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !Settings::get(cx).restore_tabs {
            return;
        }
        let Some(json) = self
            .backend
            .store
            .setting(crate::layout::TABS_KEY)
            .ok()
            .flatten()
            .filter(|j| !j.is_empty())
        else {
            return;
        };
        let saved = SavedSession::from_json(&json);
        let profiles = self.profiles.clone();
        let exists = |p: &SavedPane| match (&p.profile, &p.quick) {
            (Some(id), _) => profiles.iter().any(|x| x.id == *id),
            (None, Some(q)) => quick_profile(q).is_ok(),
            _ => false,
        };
        // Saved pane id → new pane id, to restore which file browser follows
        // which terminal.
        let mut ids = HashMap::new();
        for t in saved.tabs {
            let Some(root) = t.root.prune(&exists) else {
                continue;
            };
            let root = self.build_tree(root, &mut ids);
            let focused = root.leaves().first().map(|p| p.id).unwrap_or(0);
            let id = self.next_id();
            self.tabs.push(Tab {
                id,
                kind: match t.kind {
                    SavedKind::Terminal => TabKind::Terminal,
                    SavedKind::Files => TabKind::Files,
                },
                root,
                focused,
            });
        }
        for t in &mut self.tabs {
            for p in t.root.leaves_mut() {
                p.follows = p.follows.and_then(|old| ids.get(&old).copied());
            }
        }
        if !self.tabs.is_empty() {
            let ix = saved.active.unwrap_or(0).min(self.tabs.len() - 1);
            self.activate_tab(ix, window, cx);
        }
    }

    /// Live panes from a saved tree; `ids` collects saved id → new id.
    fn build_tree(&mut self, node: SavedNode, ids: &mut HashMap<u64, u64>) -> Node<Pane> {
        match node {
            SavedNode::Pane(p) => {
                let profile = match (&p.profile, &p.quick) {
                    (Some(id), _) => self.profiles.iter().find(|x| x.id == *id).cloned(),
                    (None, Some(q)) => quick_profile(q).ok(),
                    _ => None,
                }
                .unwrap_or_else(|| HostProfile::new("?", "?"));
                let mut pane = self.new_pane(profile, p.quick);
                // Still the saved id for now; remapped once all tabs exist.
                pane.follows = p.follows;
                pane.start_dir = p.dir;
                if let Some(old) = p.id {
                    ids.insert(old, pane.id);
                }
                Node::Leaf(pane)
            }
            SavedNode::Split {
                vertical,
                ratio,
                a,
                b,
            } => {
                let a = Box::new(self.build_tree(*a, ids));
                let b = Box::new(self.build_tree(*b, ids));
                Node::Split {
                    id: self.next_id(),
                    vertical,
                    ratio: crate::layout::clamp_ratio(ratio),
                    a,
                    b,
                }
            }
        }
    }

    // ---- settings ---------------------------------------------------------------

    pub(crate) fn open_settings(
        &mut self,
        section: Option<crate::settingsform::Section>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let profiles = self.profiles.clone();
        crate::settingsform::open(
            self.backend.clone(),
            profiles,
            section,
            window,
            cx,
            move |event, window, cx| {
                let _ = this.update(cx, |app, cx| app.on_settings_event(event, window, cx));
            },
        );
    }

    fn on_settings_event(
        &mut self,
        ev: crate::settingsform::SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::settingsform::SettingsEvent;
        match ev {
            SettingsEvent::Saved => {
                self.apply_language(window, cx);
                self.apply_settings_to_views(cx);
                self.persist_tabs(cx);
                self.mark_sync_dirty();
            }
            SettingsEvent::SyncNow(mode) => self.run_sync(mode, true, window, cx),
            SettingsEvent::ProfilesChanged => {
                self.reload_profiles();
                cx.notify();
            }
        }
    }

    /// After the UI language changed: texts kept in inputs follow, and
    /// every window redraws in the new language.
    pub(crate) fn apply_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = crate::i18n::current();
        if now == self.lang {
            return;
        }
        self.lang = now;
        self.search.update(cx, |s, cx| {
            s.set_placeholder(t!("sidebar-search-placeholder"), window, cx)
        });
        cx.refresh_windows();
    }

    fn apply_settings_to_views(&mut self, cx: &mut Context<Self>) {
        let s = Settings::get(cx).clone();
        for v in self.all_terminals() {
            v.update(cx, |v, cx| v.apply_settings(&s, cx));
        }
        cx.notify();
    }

    // ---- transfers ------------------------------------------------------------

    fn on_transfer(&mut self, ev: TransferEvent, cx: &mut Context<Self>) {
        match ev {
            TransferEvent::Queued { id, label } => {
                self.transfers.push(TransferRow {
                    id,
                    label,
                    done: 0,
                    total: 0,
                    rate: 0.0,
                    state: None,
                });
                // Keep the panel bounded.
                if self.transfers.len() > 200 {
                    self.transfers.retain(|t| t.state.is_none());
                }
            }
            TransferEvent::Progress {
                id,
                done,
                total,
                bytes_per_sec,
            } => {
                if let Some(r) = self.transfers.iter_mut().find(|r| r.id == id) {
                    (r.done, r.total, r.rate) = (done, total, bytes_per_sec);
                }
            }
            TransferEvent::Finished { id, state } => {
                if let Some(r) = self.transfers.iter_mut().find(|r| r.id == id) {
                    if let TransferState::Done { .. } = state {
                        r.done = r.total;
                    }
                    r.state = Some(state);
                }
            }
        }
        cx.notify();
    }

    // ---- profiles ---------------------------------------------------------------

    /// New host (`None`) or edit an existing one.
    pub(crate) fn host_dialog(
        &mut self,
        existing: Option<HostProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        crate::hostform::open(
            existing,
            self.backend.clone(),
            window,
            cx,
            move |p, window, cx| {
                let _ = app.update(cx, |a, cx| {
                    match a.backend.store.upsert(&p) {
                        Ok(()) => {
                            a.selected = Some(p.id);
                            a.reload_profiles();
                            // Open tabs keep working; new connections use the edit.
                            for t in &mut a.tabs {
                                for pane in t.root.leaves_mut() {
                                    if pane.profile.id == p.id {
                                        pane.profile = p.clone();
                                    }
                                }
                            }
                            a.mark_sync_dirty();
                        }
                        Err(e) => window.toast(Toast::error(e.to_string()), cx),
                    }
                    cx.notify();
                });
            },
        );
    }

    fn duplicate_profile(&mut self, id: ProfileId, cx: &mut Context<Self>) {
        let Some(mut p) = self.profiles.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        p.id = ProfileId::new();
        p.name = t!("host-copy-name", name = &p.name).into();
        p.source = Default::default();
        if self.backend.store.upsert(&p).is_ok() {
            self.selected = Some(p.id);
            self.reload_profiles();
            self.mark_sync_dirty();
        }
        cx.notify();
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.delete_profile(id, window, cx);
        }
    }

    fn open_profile(
        &mut self,
        id: ProfileId,
        kind: TabKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(id);
        if let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() {
            self.open_tab(p, kind, window, cx);
        }
    }

    fn open_selected(&mut self, kind: TabKind, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self
            .selected
            .and_then(|id| self.profiles.iter().find(|p| p.id == id))
            .cloned()
        {
            self.open_tab(p, kind, window, cx);
        }
    }

    // ---- keyboard ---------------------------------------------------------------

    /// Saved hosts in the order the sidebar shows them (search applied).
    fn visible_profiles(&self, cx: &App) -> Vec<ProfileId> {
        self.sidebar_rows(cx)
            .into_iter()
            .filter_map(|r| match r {
                SidebarRow::Profile(i) => self.profiles.get(i).map(|p| p.id),
                SidebarRow::Folder(_) => None,
            })
            .collect()
    }

    /// Keys in the saved-hosts list: arrows/Home/End/PageUp/PageDown move,
    /// Enter opens a terminal, Shift+Enter the files, F2 edits, Del deletes
    /// (after asking), Shift+F10 or the Menu key opens the host's menu.
    fn on_list_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if crate::a11y::is_menu_key(ks) {
            if self.selected.is_none() {
                self.select_relative(0, window, cx);
            }
            self.host_anchor.open(window, cx);
            cx.stop_propagation();
            return;
        }
        if m.control || m.alt || m.platform {
            return;
        }
        let handled = match ks.key.as_str() {
            "up" => self.select_relative(-1, window, cx),
            "down" => self.select_relative(1, window, cx),
            "pageup" => self.select_relative(-10, window, cx),
            "pagedown" => self.select_relative(10, window, cx),
            "home" => self.select_relative(isize::MIN, window, cx),
            "end" => self.select_relative(isize::MAX, window, cx),
            "enter" => {
                let kind = if m.shift {
                    TabKind::Files
                } else {
                    TabKind::Terminal
                };
                self.open_selected(kind, window, cx);
                true
            }
            "f2" => {
                let p = self
                    .selected
                    .and_then(|id| self.profiles.iter().find(|p| p.id == id))
                    .cloned();
                if p.is_some() {
                    self.host_dialog(p, window, cx);
                }
                true
            }
            "delete" => {
                self.delete_selected(window, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    /// Move the host selection by `delta` rows (clamped; `isize::MIN`/`MAX`
    /// for first/last) and scroll it into view.
    fn select_relative(&mut self, delta: isize, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let ids = self.visible_profiles(cx);
        if ids.is_empty() {
            return true;
        }
        let cur = self
            .selected
            .and_then(|s| ids.iter().position(|id| *id == s));
        let last = ids.len() as isize - 1;
        let next = match (cur, delta) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => last,
            (None, _) => 0,
            (Some(c), d) => (c as isize).saturating_add(d).clamp(0, last),
        } as usize;
        self.selected = Some(ids[next]);
        // Scroll the row (folders are rows too) into view.
        let rows = self.sidebar_rows(cx);
        if let Some(ix) = rows.iter().position(|r| {
            matches!(r, SidebarRow::Profile(i) if self.profiles.get(*i).map(|p| p.id) == Some(ids[next]))
        }) {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
        true
    }

    /// Keys on the tab bar: Left/Right (Home/End) switch tabs, Enter or
    /// Down goes into the tab, Delete closes it, Shift+F10/Menu its menu.
    fn on_tabs_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if crate::a11y::is_menu_key(ks) {
            self.tab_anchor.open(window, cx);
            cx.stop_propagation();
            return;
        }
        if m.control || m.alt || m.platform || m.shift || self.tabs.is_empty() {
            return;
        }
        let n = self.tabs.len();
        let cur = self.active.unwrap_or(0).min(n - 1);
        let target = match ks.key.as_str() {
            "left" => Some((cur + n - 1) % n),
            "right" => Some((cur + 1) % n),
            "home" => Some(0),
            "end" => Some(n - 1),
            "enter" | "down" | "space" => {
                self.focus_active(window, cx);
                cx.stop_propagation();
                return;
            }
            "delete" => {
                self.close_tab(cur, cx);
                if self.tabs.is_empty() {
                    self.list_focus.focus(window, cx);
                }
                cx.stop_propagation();
                return;
            }
            _ => None,
        };
        if let Some(ix) = target {
            self.activate_tab(ix, window, cx);
            // Stay on the tab bar while choosing.
            self.tabs_focus.focus(window, cx);
            cx.stop_propagation();
        }
    }

    /// Ctrl+F6 / Ctrl+Shift+F6: move keyboard focus between the hosts list,
    /// the tab bar and the open session (Tab cannot leave a terminal: the
    /// shell needs it).
    fn cycle_region(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let in_list = self.list_focus.contains_focused(window, cx);
        let in_tabs = self.tabs_focus.contains_focused(window, cx);
        let has_body = self.active.is_some();
        // 0 = list, 1 = tab bar, 2 = session
        let cur = if in_list {
            0
        } else if in_tabs {
            1
        } else if has_body {
            2
        } else {
            0
        };
        let regions: &[usize] = if has_body { &[0, 1, 2] } else { &[0] };
        let pos = regions.iter().position(|r| *r == cur).unwrap_or(0);
        let next = if forward {
            regions[(pos + 1) % regions.len()]
        } else {
            regions[(pos + regions.len() - 1) % regions.len()]
        };
        match next {
            1 => self.tabs_focus.focus(window, cx),
            2 => self.focus_active(window, cx),
            _ => {
                if self.selected.is_none() {
                    self.select_relative(0, window, cx);
                }
                self.list_focus.focus(window, cx)
            }
        }
        cx.notify();
    }

    /// App-wide shortcuts, handled before the terminal sees the key. They
    /// all use Shift (or Cmd on macOS) so plain Ctrl+letter keys still reach
    /// the shell (Ctrl+W, Ctrl+T, Ctrl+N mean things there).
    fn on_shortcut(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        let mac = cfg!(target_os = "macos");
        if key == "f6" && m.control && !m.alt && !m.platform {
            self.cycle_region(!m.shift, window, cx);
            cx.stop_propagation();
            return;
        }
        let cmd = if mac {
            m.platform
        } else {
            m.control && m.shift
        };
        // Split right: Ctrl+Shift+D / ⌘D. Split down: Ctrl+Shift+E / ⌘⇧D.
        let split = if mac {
            (m.platform && key == "d").then_some(m.shift)
        } else {
            (m.control && m.shift && !m.alt)
                .then_some(())
                .and(match key {
                    "d" => Some(false),
                    "e" => Some(true),
                    _ => None,
                })
        };
        if let Some(vertical) = split {
            self.split_focused(vertical, window, cx);
            cx.stop_propagation();
            return;
        }
        let multi_pane = self
            .active
            .and_then(|i| self.tabs.get(i))
            .is_some_and(|t| t.root.count() > 1);
        let handled = match key {
            "left" | "right" | "up" | "down"
                if m.alt && !m.control && !m.platform && multi_pane =>
            {
                let dir = match key {
                    "left" => Dir::Left,
                    "right" => Dir::Right,
                    "up" => Dir::Up,
                    _ => Dir::Down,
                };
                if m.shift {
                    self.resize_focused(dir, cx);
                } else {
                    self.move_focus(dir, window, cx);
                }
                true
            }
            "tab" if m.control => {
                if !self.tabs.is_empty() {
                    let n = self.tabs.len();
                    let cur = self.active.unwrap_or(0);
                    let next = if m.shift {
                        (cur + n - 1) % n
                    } else {
                        (cur + 1) % n
                    };
                    self.activate_tab(next, window, cx);
                }
                true
            }
            "t" if cmd => {
                if let Some(t) = self.active.and_then(|i| self.tabs.get(i))
                    && let Some(p) = t.focused_pane()
                {
                    let (profile, quick, kind) = (p.profile.clone(), p.quick.clone(), t.kind);
                    self.open_tab_quick(profile, quick, kind, window, cx);
                }
                true
            }
            "w" if cmd => {
                self.close_focused_pane(window, cx);
                true
            }
            "r" if cmd => {
                if let Some(ix) = self.active {
                    self.reconnect_focused(ix, window, cx);
                }
                true
            }
            "n" if cmd => {
                self.host_dialog(None, window, cx);
                true
            }
            "," if m.control || m.platform => {
                self.open_settings(None, window, cx);
                true
            }
            "b" if cmd => {
                self.broadcast = !self.broadcast;
                cx.notify();
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }
}
