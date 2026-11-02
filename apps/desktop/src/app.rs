//! Root view: session sidebar, tabs (terminals and file browsers), broadcast
//! input, the transfer queue, and dialogs for host keys and passwords.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use futures::channel::oneshot;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, LogVars, ProfileId, ProfileStore, expand_log_path};
use hostwick_ssh::{HostKeyDecision, HostKeyStatus};
use hostwick_transfer::{TransferEvent, TransferId, TransferState};

use crate::backend::{Backend, Prompt, quick_profile};
use crate::files::FilesView;
use crate::settings::Settings;
use crate::terminal::{GridSize, TerminalEvent, TerminalView};
use crate::util::human_size;

enum TabBody {
    Connecting,
    Failed(String),
    Terminal(Entity<TerminalView>),
    Files(Entity<FilesView>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabKind {
    Terminal,
    Files,
}

struct Tab {
    id: u64,
    title: SharedString,
    profile: HostProfile,
    kind: TabKind,
    body: TabBody,
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
    next_tab: u64,
    broadcast: bool,
    transfers: Vec<TransferRow>,
    prompts: VecDeque<Prompt>,
    prompt_open: bool,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search hosts"));
        let quick = cx.new(|cx| InputState::new(window, cx).placeholder("user@host:port  ⏎"));
        let mut subs = vec![
            cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
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
                        Ok(p) => this.open_tab(p, TabKind::Terminal, window, cx),
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
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

        let mut app = Self {
            backend,
            profiles: Vec::new(),
            selected: None,
            search,
            quick,
            tabs: Vec::new(),
            active: None,
            next_tab: 1,
            broadcast: false,
            transfers: Vec::new(),
            prompts: VecDeque::new(),
            prompt_open: false,
            _tasks: vec![prompt_task, transfer_task],
            _subs: subs,
        };
        app.reload_profiles();
        app
    }

    fn reload_profiles(&mut self) {
        self.profiles = self.backend.store.list().unwrap_or_default();
        self.profiles.sort_by_key(|p| p.path().to_lowercase());
    }

    // ---- tabs ---------------------------------------------------------------

    fn open_tab(
        &mut self,
        profile: HostProfile,
        kind: TabKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_tab;
        self.next_tab += 1;
        let title: SharedString = match kind {
            TabKind::Terminal => crate::terminal::tab_title(&profile).into(),
            TabKind::Files => format!("Files: {}", profile.name).into(),
        };
        self.tabs.push(Tab {
            id,
            title: title.clone(),
            profile: profile.clone(),
            kind,
            body: TabBody::Connecting,
        });
        self.active = Some(self.tabs.len() - 1);
        self.connect_tab(id, window, cx);
        cx.notify();
    }

    fn connect_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        tab.body = TabBody::Connecting;
        let (profile, kind, title) = (tab.profile.clone(), tab.kind, tab.title.clone());
        let backend = self.backend.clone();
        let rt = backend.handle();
        let initial = GridSize {
            cols: 120,
            rows: 32,
        };
        match kind {
            TabKind::Terminal => {
                let profile_for_log = profile.clone();
                let job = rt.spawn(backend.clone().open_shell(profile, 120, 32));
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
                                app.auto_session_log(&profile_for_log, &view, cx);
                                view.update(cx, |v, cx| v.attach_profile(&profile_for_log, cx));
                                view.read(cx).focus_handle(cx).focus(window, cx);
                                TabBody::Terminal(view)
                            }
                            Ok(Err(e)) => TabBody::Failed(format!("{e:#}")),
                            Err(e) => TabBody::Failed(format!("internal error: {e}")),
                        };
                        if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == id) {
                            tab.body = body;
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            TabKind::Files => {
                let job = rt.spawn(backend.clone().open_files(profile.clone()));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        let body = match result {
                            Ok(Ok(session)) => {
                                let backend = app.backend.clone();
                                TabBody::Files(cx.new(|cx| {
                                    FilesView::new(session, profile, backend, window, cx)
                                }))
                            }
                            Ok(Err(e)) => TabBody::Failed(format!("{e:#}")),
                            Err(e) => TabBody::Failed(format!("internal error: {e}")),
                        };
                        if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == id) {
                            tab.body = body;
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.tabs.remove(ix);
            self.active = match self.active {
                _ if self.tabs.is_empty() => None,
                Some(a) if a >= self.tabs.len() => Some(self.tabs.len() - 1),
                Some(a) if a > ix => Some(a - 1),
                other => other,
            };
            cx.notify();
        }
    }

    fn on_terminal_event(
        &mut self,
        source: &Entity<TerminalView>,
        ev: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_ix = self.tabs.iter().position(
            |t| matches!(&t.body, TabBody::Terminal(v) if v.entity_id() == source.entity_id()),
        );
        match ev {
            TerminalEvent::UserInput(bytes) if self.broadcast => {
                for tab in &self.tabs {
                    if let TabBody::Terminal(v) = &tab.body
                        && v.entity_id() != source.entity_id()
                    {
                        v.read(cx).write_raw(bytes);
                    }
                }
            }
            TerminalEvent::UserInput(_) => {}
            TerminalEvent::TitleChanged
            | TerminalEvent::Exited
            | TerminalEvent::LoggingChanged
            | TerminalEvent::CwdChanged(_) => cx.notify(),
            TerminalEvent::Reconnect => {
                if let Some(ix) = tab_ix {
                    self.reconnect_tab(ix, window, cx);
                }
            }
            TerminalEvent::Duplicate => {
                if let Some(ix) = tab_ix {
                    let p = self.tabs[ix].profile.clone();
                    self.open_tab(p, TabKind::Terminal, window, cx);
                }
            }
            TerminalEvent::OpenFiles => {
                if let Some(ix) = tab_ix {
                    let p = self.tabs[ix].profile.clone();
                    self.open_tab(p, TabKind::Files, window, cx);
                }
            }
        }
    }

    fn reconnect_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.tabs.get(ix).map(|t| t.id) {
            self.active = Some(ix);
            self.connect_tab(id, window, cx);
            cx.notify();
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

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        crate::settingsform::open(self.backend.clone(), window, cx, move |s, _, cx| {
            let s = s.clone();
            let _ = this.update(cx, |app, cx| {
                for tab in &app.tabs {
                    if let TabBody::Terminal(v) = &tab.body {
                        v.update(cx, |v, cx| v.apply_settings(&s, cx));
                    }
                }
                cx.notify();
            });
        });
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

    // ---- prompts ----------------------------------------------------------------

    fn enqueue_prompt(&mut self, p: Prompt, window: &mut Window, cx: &mut Context<Self>) {
        if let Prompt::Banner(text) = p {
            window.push_notification(Notification::info(text), cx);
            return;
        }
        self.prompts.push_back(p);
        if !self.prompt_open {
            self.show_next_prompt(window, cx);
        }
    }

    fn prompt_done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_open = false;
        self.show_next_prompt(window, cx);
    }

    fn show_next_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.prompts.pop_front() else {
            return;
        };
        self.prompt_open = true;
        let app = cx.entity().downgrade();
        match p {
            Prompt::Banner(_) => self.prompt_done(window, cx),
            Prompt::HostKey { info, reply } => {
                let reply = slot(reply);
                let changed = matches!(info.status, HostKeyStatus::Changed { .. });
                let target = if info.port == 22 {
                    info.host.clone()
                } else {
                    format!("{}:{}", info.host, info.port)
                };
                let body = if changed {
                    format!(
                        "WARNING: the host key for {target} has CHANGED.\n\nThis can mean someone is intercepting the connection (man-in-the-middle), or the server was reinstalled.\n\nNew {} key:\n{}\n\nOnly continue if you know the key was changed on purpose.",
                        info.algorithm, info.fingerprint
                    )
                } else {
                    format!(
                        "This is the first connection to {target}.\n\n{} key fingerprint:\n{}\n\nVerify it with the server's administrator before trusting it.",
                        info.algorithm, info.fingerprint
                    )
                };
                window.open_dialog(cx, move |dialog, _, _| {
                    let decide =
                        |label: &'static str, id: &'static str, decision: HostKeyDecision| {
                            let (reply, app) = (reply.clone(), app.clone());
                            Button::new(id).label(label).on_click(move |_, window, cx| {
                                answer(&reply, decision);
                                window.close_dialog(cx);
                                let _ = app.update(cx, |a, cx| a.prompt_done(window, cx));
                            })
                        };
                    let (reply_close, app_close) = (reply.clone(), app.clone());
                    dialog
                        .title(if changed {
                            "Host key changed!"
                        } else {
                            "Unknown host"
                        })
                        .w(px(520.0))
                        .child(
                            div()
                                .whitespace_normal()
                                .text_size(px(13.0))
                                .child(body.clone()),
                        )
                        .on_close(move |_, window, cx| {
                            if reply_close.borrow().is_some() {
                                answer(&reply_close, HostKeyDecision::Reject);
                                let _ = app_close.update(cx, |a, cx| a.prompt_done(window, cx));
                            }
                        })
                        .footer(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .child(decide("Reject", "hk-reject", HostKeyDecision::Reject))
                                .child(decide("Trust once", "hk-once", HostKeyDecision::AcceptOnce))
                                .child(
                                    decide(
                                        if changed {
                                            "Replace saved key"
                                        } else {
                                            "Trust and remember"
                                        },
                                        "hk-remember",
                                        HostKeyDecision::AcceptAndRemember,
                                    )
                                    .when(!changed, |b| b.primary())
                                    .when(changed, |b| b.danger()),
                                ),
                        )
                });
            }
            Prompt::Secret {
                title,
                label,
                reply,
            } => {
                let reply = slot(reply);
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder(label).masked(true));
                self.secret_dialog(
                    title,
                    vec![input],
                    String::new(),
                    window,
                    cx,
                    move |values| {
                        answer(&reply, values.and_then(|v| v.into_iter().next()));
                    },
                );
            }
            Prompt::KeyboardInteractive {
                title,
                instructions,
                prompts,
                reply,
            } => {
                let reply = slot(reply);
                let inputs: Vec<_> = prompts
                    .iter()
                    .map(|(p, echo)| {
                        let echo = *echo;
                        let p = p.trim().to_owned();
                        cx.new(|cx| InputState::new(window, cx).placeholder(p).masked(!echo))
                    })
                    .collect();
                let labels = prompts
                    .iter()
                    .map(|(p, _)| p.trim().to_owned())
                    .collect::<Vec<_>>()
                    .join(" / ");
                let text = if instructions.is_empty() {
                    labels
                } else {
                    format!("{instructions}\n{labels}")
                };
                self.secret_dialog(title, inputs, text, window, cx, move |values| {
                    answer(&reply, values)
                });
            }
        }
    }

    /// Dialog with one or more (masked) inputs. `done(None)` on cancel.
    fn secret_dialog(
        &mut self,
        title: String,
        inputs: Vec<Entity<InputState>>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl Fn(Option<Vec<String>>) + 'static,
    ) {
        let app = cx.entity().downgrade();
        let done = Rc::new(done);
        let finished = Rc::new(RefCell::new(false));
        let submit = {
            let (inputs, done, finished, app) =
                (inputs.clone(), done.clone(), finished.clone(), app.clone());
            Rc::new(move |ok: bool, window: &mut Window, cx: &mut App| {
                if finished.replace(true) {
                    return;
                }
                let values = ok.then(|| {
                    inputs
                        .iter()
                        .map(|i| i.read(cx).value().to_string())
                        .collect()
                });
                // Clear typed secrets from the inputs immediately.
                for i in &inputs {
                    i.update(cx, |s, cx| s.set_value("", window, cx));
                }
                done(values);
                window.close_dialog(cx);
                let _ = app.update(cx, |a, cx| a.prompt_done(window, cx));
            })
        };
        if let Some(last) = inputs.last() {
            let submit = submit.clone();
            let sub = cx.subscribe_in(last, window, move |_, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    submit(true, window, cx);
                }
            });
            self._subs.push(sub);
        }
        let first = Rc::new(std::cell::Cell::new(true));
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false)
                && let Some(i) = inputs.first()
            {
                i.focus_handle(cx).focus(window, cx);
            }
            let (ok, cancel, close, enter) = (
                submit.clone(),
                submit.clone(),
                submit.clone(),
                submit.clone(),
            );
            let mut body = div().flex().flex_col().gap(px(8.0));
            if !text.is_empty() {
                body = body.child(
                    div()
                        .text_size(px(13.0))
                        .whitespace_normal()
                        .child(text.clone()),
                );
            }
            for i in &inputs {
                body = body.child(Input::new(i).mask_toggle());
            }
            dialog
                .title(title.clone())
                .w(px(420.0))
                .child(body)
                // Enter triggers the dialog's Confirm action: submit, and
                // return false because `submit` closes the dialog itself.
                .on_ok(move |_, window, cx| {
                    enter(true, window, cx);
                    false
                })
                .on_close(move |_, window, cx| close(false, window, cx))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(move |_, w, cx| cancel(false, w, cx)),
                        )
                        .child(
                            Button::new("ok")
                                .primary()
                                .label("Continue")
                                .on_click(move |_, w, cx| ok(true, w, cx)),
                        ),
                )
        });
    }

    // ---- profiles ---------------------------------------------------------------

    /// New host (`None`) or edit an existing one.
    fn host_dialog(
        &mut self,
        existing: Option<HostProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity().downgrade();
        crate::hostform::open(existing, window, cx, move |p, window, cx| {
            let _ = app.update(cx, |a, cx| {
                match a.backend.store.upsert(&p) {
                    Ok(()) => {
                        a.selected = Some(p.id);
                        a.reload_profiles();
                        // Open tabs keep working; new connections use the edit.
                        for t in &mut a.tabs {
                            if t.profile.id == p.id {
                                t.profile = p.clone();
                            }
                        }
                    }
                    Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                }
                cx.notify();
            });
        });
    }

    fn duplicate_profile(&mut self, id: ProfileId, cx: &mut Context<Self>) {
        let Some(mut p) = self.profiles.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        p.id = ProfileId::new();
        p.name = format!("{} (copy)", p.name);
        p.source = Default::default();
        if self.backend.store.upsert(&p).is_ok() {
            self.selected = Some(p.id);
            self.reload_profiles();
        }
        cx.notify();
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.delete_profile(id, window, cx);
        }
    }

    fn delete_profile(&mut self, id: ProfileId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (app2, p2) = (app.clone(), p.clone());
            dialog
                .title("Delete host")
                .child(format!("Delete the saved host “{}”?", p.path()))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("del").danger().label("Delete").on_click(
                            move |_, window, cx| {
                                let _ = app2.update(cx, |a, cx| {
                                    let _ = a.backend.store.delete(p2.id);
                                    if a.selected == Some(p2.id) {
                                        a.selected = None;
                                    }
                                    a.reload_profiles();
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            },
                        )),
                )
        });
    }

    fn import(&mut self, which: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        // "putty"/"winscp" read this PC's registry (Windows); "*-file" asks for a file.
        let from_registry = cfg!(windows) && matches!(which, "putty" | "winscp");
        let which = which.trim_end_matches("-file");
        if which == "ssh" || from_registry {
            let result = match which {
                "ssh" => std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::Path::new(&h).join(".ssh").join("config"))
                    .ok_or_else(|| "no home directory".to_owned())
                    .and_then(|p| {
                        hostwick_import::load_ssh_config(&p)
                            .map_err(|e| format!("{}: {e}", p.display()))
                    })
                    .map(|text| hostwick_import::parse_ssh_config(&text)),
                "putty" => hostwick_import::putty_from_registry().map_err(|e| e.to_string()),
                _ => hostwick_import::winscp_from_registry().map_err(|e| e.to_string()),
            };
            self.apply_import(result, window, cx);
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                match which {
                    "putty" => "PuTTY .reg export",
                    "solar" => "Solar-PuTTY export (.json)",
                    _ => "WinSCP.ini",
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = match std::fs::read(&path) {
                Ok(b) => hostwick_import::decode_text(&b),
                Err(e) => {
                    let _ =
                        this.update_in(cx, |a, w, cx| a.apply_import(Err(e.to_string()), w, cx));
                    return;
                }
            };
            let result = match which {
                "putty" => hostwick_import::parse_putty_reg(&text),
                "solar" => hostwick_import::parse_solarputty_json(&text),
                _ => hostwick_import::parse_winscp_ini(&text),
            }
            .map_err(|e| e.to_string());
            let _ = this.update_in(cx, |a, w, cx| a.apply_import(result, w, cx));
        })
        .detach();
    }

    fn apply_import(
        &mut self,
        result: Result<hostwick_import::ImportResult, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                window.push_notification(Notification::error(format!("Import failed: {e}")), cx);
                return;
            }
        };
        let (mut added, mut updated) = (Vec::new(), Vec::new());
        let mut failed = Vec::new();
        for mut p in r.profiles {
            let prior = self
                .profiles
                .iter()
                .find(|e| e.source == p.source && p.source != Default::default());
            if let Some(prior) = prior {
                p.id = prior.id;
                // Keep what the person changed locally after the last import.
                p.session_log = prior.session_log.clone();
                p.notes = prior.notes.clone();
            }
            match self.backend.store.upsert(&p) {
                Ok(()) if prior.is_some() => updated.push(p.path()),
                Ok(()) => added.push(p.path()),
                Err(e) => failed.push(format!("{}: {e}", p.path())),
            }
        }
        self.reload_profiles();
        for w in &r.warnings {
            tracing::warn!("import: {w}");
        }
        let summary = format!(
            "{} added, {} updated, {} with notes",
            added.len(),
            updated.len(),
            r.warnings.len() + failed.len()
        );
        tracing::info!("import: {summary}");
        if r.warnings.is_empty() && failed.is_empty() {
            window.push_notification(Notification::success(format!("Imported: {summary}")), cx);
            cx.notify();
            return;
        }
        // Something needs attention: show the full report instead of a toast.
        let notes: Vec<String> = failed.into_iter().chain(r.warnings).collect();
        let lists = Rc::new((added, updated, notes));
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let section = |title: String, items: &[String]| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().pt(px(6.0)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .children(items.iter().map(|i| {
                        div()
                            .pl(px(10.0))
                            .text_size(px(12.0))
                            .child(SharedString::from(format!("• {i}")))
                    }))
            };
            let (added, updated, notes) = &*lists;
            dialog
                .title("Import report")
                .w(px(640.0))
                .child(
                    div()
                        .id("import-report")
                        .max_h(px(480.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .when(!notes.is_empty(), |d| {
                            d.child(section(format!("Needs attention ({})", notes.len()), notes))
                        })
                        .when(!added.is_empty(), |d| {
                            d.child(section(format!("Added ({})", added.len()), added))
                        })
                        .when(!updated.is_empty(), |d| {
                            d.child(section(format!("Updated ({})", updated.len()), updated))
                        })
                        .child(
                            div()
                                .pt(px(8.0))
                                .text_size(px(11.0))
                                .text_color(muted)
                                .child("Hosts listed under “Needs attention” were still imported unless the note says skipped. Right-click a host → Edit… to adjust it."),
                        ),
                )
                .footer(
                    Button::new("ok")
                        .primary()
                        .label("OK")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
        });
        cx.notify();
    }

    // ---- rendering ----------------------------------------------------------------

    fn sidebar_rows(&self, cx: &App) -> Vec<SidebarRow> {
        let needle = self.search.read(cx).value().to_lowercase();
        let mut rows = Vec::new();
        let mut last_folder: Option<&str> = None;
        for (i, p) in self.profiles.iter().enumerate() {
            if !needle.is_empty() {
                let hay = format!(
                    "{} {} {}",
                    p.path(),
                    p.host,
                    p.user.as_deref().unwrap_or("")
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
                            .gap(px(8.0))
                            .child(img(logo()).size(px(22.0)))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Hostwick"),
                            )
                            .child(
                                Button::new("settings")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Settings)
                                    .tooltip("Settings")
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.open_settings(w, cx)),
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
                                    .child(f.clone())
                                    .into_any_element(),
                                SidebarRow::Profile(i) => {
                                    let p = &this.profiles[*i];
                                    let selected = this.selected == Some(p.id);
                                    let id = p.id;
                                    let indent = if p.folder.is_empty() { 10.0 } else { 22.0 };
                                    div()
                                        .id(("profile", ix))
                                        .w_full()
                                        .h(px(28.0))
                                        .pl(px(indent))
                                        .pr(px(10.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .text_size(px(13.0))
                                        .when(selected, |d| d.bg(theme.sidebar_accent))
                                        .when(!selected, |d| {
                                            d.hover(|s| s.bg(theme.sidebar_accent))
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
                                        .child(
                                            div()
                                                .flex_1()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .child(p.name.clone()),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(move |app, _, _, cx| {
                                                app.selected = Some(id);
                                                cx.notify();
                                            }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted_foreground)
                                                .whitespace_nowrap()
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

    fn import_button(app: WeakEntity<Self>) -> impl IntoElement {
        let item = move |label: &'static str, which: &'static str| {
            let app = app.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let _ = app.update(cx, |a, cx| a.import(which, window, cx));
            })
        };
        Button::new("import")
            .small()
            .label("Import…")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                let m = menu.item(item("OpenSSH ~/.ssh/config", "ssh"));
                let m = if cfg!(windows) {
                    m.item(item("PuTTY (this PC's saved sessions)", "putty"))
                        .item(item("WinSCP (this PC's saved sites)", "winscp"))
                } else {
                    m
                };
                m.item(item("PuTTY .reg export file…", "putty-file"))
                    .item(item("WinSCP.ini file…", "winscp-file"))
                    .item(item("Solar-PuTTY .json export…", "solar"))
            })
    }

    /// Right-click menu for a saved host.
    fn host_menu(
        app: WeakEntity<Self>,
        id: ProfileId,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, _| {
            let act = |f: fn(&mut Self, ProfileId, &mut Window, &mut Context<Self>)| {
                let app = app.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| f(a, id, window, cx));
                }
            };
            menu.item(
                PopupMenuItem::new("Open terminal").on_click(act(|a, id, w, cx| {
                    a.open_profile(id, TabKind::Terminal, w, cx)
                })),
            )
            .item(
                PopupMenuItem::new("Open files (SFTP)").on_click(act(|a, id, w, cx| {
                    a.open_profile(id, TabKind::Files, w, cx)
                })),
            )
            .separator()
            .item(PopupMenuItem::new("Edit…").on_click(act(|a, id, w, cx| {
                let p = a.profiles.iter().find(|p| p.id == id).cloned();
                if p.is_some() {
                    a.host_dialog(p, w, cx);
                }
            })))
            .item(
                PopupMenuItem::new("Duplicate")
                    .on_click(act(|a, id, _, cx| a.duplicate_profile(id, cx))),
            )
            .item(
                PopupMenuItem::new("Copy address").on_click(act(|a, id, _, cx| {
                    if let Some(p) = a.profiles.iter().find(|p| p.id == id) {
                        let user = p.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
                        let port = if p.port == 22 {
                            String::new()
                        } else {
                            format!(":{}", p.port)
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(format!(
                            "{user}{}{port}",
                            p.host
                        )));
                    }
                })),
            )
            .separator()
            .item(
                PopupMenuItem::new("Delete…")
                    .on_click(act(|a, id, w, cx| a.delete_profile(id, w, cx))),
            )
        }
    }

    /// Right-click menu for a tab.
    fn tab_menu(
        app: WeakEntity<Self>,
        tab_id: u64,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, _| {
            let act = |f: fn(&mut Self, usize, &mut Window, &mut Context<Self>)| {
                let app = app.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| {
                        if let Some(ix) = a.tabs.iter().position(|t| t.id == tab_id) {
                            f(a, ix, window, cx);
                        }
                    });
                }
            };
            menu.item(
                PopupMenuItem::new("Reconnect")
                    .on_click(act(|a, ix, w, cx| a.reconnect_tab(ix, w, cx))),
            )
            .item(
                PopupMenuItem::new("Duplicate").on_click(act(|a, ix, w, cx| {
                    let (p, k) = (a.tabs[ix].profile.clone(), a.tabs[ix].kind);
                    a.open_tab(p, k, w, cx);
                })),
            )
            .item(
                PopupMenuItem::new("Open files (SFTP)").on_click(act(|a, ix, w, cx| {
                    let p = a.tabs[ix].profile.clone();
                    a.open_tab(p, TabKind::Files, w, cx);
                })),
            )
            .item(
                PopupMenuItem::new("Open terminal").on_click(act(|a, ix, w, cx| {
                    let p = a.tabs[ix].profile.clone();
                    a.open_tab(p, TabKind::Terminal, w, cx);
                })),
            )
            .separator()
            .item(PopupMenuItem::new("Close").on_click(act(|a, ix, _, cx| a.close_tab(ix, cx))))
            .item(
                PopupMenuItem::new("Close others").on_click(act(|a, ix, _, cx| {
                    let keep = a.tabs.remove(ix);
                    a.tabs = vec![keep];
                    a.active = Some(0);
                    cx.notify();
                })),
            )
            .item(PopupMenuItem::new("Close all").on_click(act(|a, _, _, cx| {
                a.tabs.clear();
                a.active = None;
                cx.notify();
            })))
        }
    }

    /// App-wide shortcuts, handled before the terminal sees the key. They
    /// all use Shift (or Cmd on macOS) so plain Ctrl+letter keys still reach
    /// the shell (Ctrl+W, Ctrl+T, Ctrl+N mean things there).
    fn on_shortcut(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        let cmd = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control && m.shift
        };
        let handled = match key {
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
                if let Some(t) = self.active.and_then(|i| self.tabs.get(i)) {
                    let (p, k) = (t.profile.clone(), t.kind);
                    self.open_tab(p, k, window, cx);
                }
                true
            }
            "w" if cmd => {
                if let Some(ix) = self.active {
                    self.close_tab(ix, cx);
                }
                true
            }
            "r" if cmd => {
                if let Some(ix) = self.active {
                    self.reconnect_tab(ix, window, cx);
                }
                true
            }
            "n" if cmd => {
                self.host_dialog(None, window, cx);
                true
            }
            "," if m.control || m.platform => {
                self.open_settings(window, cx);
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

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = Some(ix);
        if let Some(TabBody::Terminal(v)) = self.tabs.get(ix).map(|t| &t.body) {
            v.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
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

    fn render_tabs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active = self.active;
        div()
            .h(px(36.0))
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
                        let alive = match &t.body {
                            TabBody::Terminal(v) => v.read(cx).is_alive(),
                            TabBody::Failed(_) => false,
                            _ => true,
                        };
                        let title = match &t.body {
                            TabBody::Terminal(v) if t.kind == TabKind::Terminal => {
                                let vt = v.read(cx).title.clone();
                                if vt.is_empty() { t.title.clone() } else { vt }
                            }
                            _ => t.title.clone(),
                        };
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
                                app.active = Some(ix);
                                if let Some(TabBody::Terminal(v)) =
                                    app.tabs.get(ix).map(|t| &t.body)
                                {
                                    v.read(cx).focus_handle(cx).focus(window, cx);
                                }
                                cx.notify();
                            }))
                            .child(div().size(px(7.0)).rounded_full().bg(if alive {
                                theme.success
                            } else {
                                theme.danger
                            }))
                            .child(div().overflow_hidden().whitespace_nowrap().child(title))
                            .context_menu(Self::tab_menu(cx.entity().downgrade(), t.id))
                            .child(
                                Button::new(("close", t.id))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .on_click(
                                        cx.listener(move |app, _, _, cx| app.close_tab(ix, cx)),
                                    ),
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
                        .tooltip("Send keystrokes to every open terminal")
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.broadcast = !app.broadcast;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(tab) = self.active.and_then(|i| self.tabs.get(i)) else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .text_color(theme.muted_foreground)
                .child(div().text_size(px(18.0)).child("No open sessions"))
                .child("Double-click a host, or type user@host in the quick-connect box.")
                .child("Right-click hosts, tabs, files and terminals for more actions.")
                .child("Import from ~/.ssh/config, PuTTY, WinSCP or Solar-PuTTY with Import… (bottom left).")
                .child(
                    div().pt(px(12.0)).text_size(px(12.0)).child(if cfg!(target_os = "macos") {
                        "⌘N new host · ⌘T duplicate tab · ⌘W close tab · ⌘R reconnect · ⌃Tab next tab · ⌘, settings"
                    } else {
                        "Ctrl+Shift+N new host · Ctrl+Shift+T duplicate tab · Ctrl+Shift+W close tab · Ctrl+Shift+R reconnect · Ctrl+Tab next tab · Ctrl+, settings"
                    }),
                )
                .into_any_element();
        };
        let id = tab.id;
        match &tab.body {
            TabBody::Terminal(v) => v.clone().into_any_element(),
            TabBody::Files(v) => v.clone().into_any_element(),
            TabBody::Connecting => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child(format!("Connecting to {}…", tab.profile.host))
                .into_any_element(),
            TabBody::Failed(err) => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .child(
                    div()
                        .text_color(theme.danger)
                        .max_w(px(640.0))
                        .whitespace_normal()
                        .child(err.clone()),
                )
                .child(
                    Button::new("retry")
                        .primary()
                        .label("Retry")
                        .on_click(cx.listener(move |app, _, w, cx| {
                            app.connect_tab(id, w, cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        }
    }

    fn render_transfers(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
            .capture_key_down(cx.listener(Self::on_shortcut))
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(tabs)
                    .child(div().flex_1().overflow_hidden().child(body))
                    .children(transfers),
            )
    }
}
