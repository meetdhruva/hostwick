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
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{AuthMethod, HostProfile, ProfileId, ProfileStore};
use hostwick_ssh::{HostKeyDecision, HostKeyStatus};
use hostwick_transfer::{TransferEvent, TransferId, TransferState};

use crate::backend::{Backend, Prompt, quick_profile};
use crate::files::FilesView;
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
            TabKind::Terminal => profile.name.clone().into(),
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
                let job = rt.spawn(backend.clone().open_shell(profile, 120, 32));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        let body = match result {
                            Ok(Ok(shell)) => {
                                let view = cx.new(|cx| {
                                    TerminalView::new(title, shell, initial, &rt, window, cx)
                                });
                                let sub = cx.subscribe(&view, Self::on_terminal_event);
                                app._subs.push(sub);
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
                let job = rt.spawn(backend.clone().open_files(profile));
                cx.spawn_in(window, async move |this, cx| {
                    let result = job.await;
                    let _ = this.update_in(cx, |app, window, cx| {
                        let body = match result {
                            Ok(Ok(fs)) => {
                                let backend = app.backend.clone();
                                TabBody::Files(cx.new(|cx| FilesView::new(fs, backend, window, cx)))
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
        source: Entity<TerminalView>,
        ev: &TerminalEvent,
        cx: &mut Context<Self>,
    ) {
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
            TerminalEvent::TitleChanged | TerminalEvent::Exited => cx.notify(),
            TerminalEvent::UserInput(_) => {}
        }
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

    fn new_host_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields: Vec<(&'static str, Entity<InputState>)> = [
            ("Name", "prod-web-1"),
            ("Folder", "prod (optional)"),
            ("Host", "10.0.0.5 or host.example.com"),
            ("Port", "22"),
            ("User", "deploy"),
            ("Key file", "~/.ssh/id_ed25519 (optional)"),
            ("Jump host", "user@bastion:22 (optional)"),
        ]
        .into_iter()
        .map(|(label, ph)| {
            (
                label,
                cx.new(|cx| InputState::new(window, cx).placeholder(ph)),
            )
        })
        .collect();
        let app = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false)
                && let Some((_, i)) = fields.first()
            {
                i.focus_handle(cx).focus(window, cx);
            }
            let mut body = div().flex().flex_col().gap(px(6.0));
            for (label, input) in &fields {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().w(px(80.0)).text_size(px(13.0)).child(*label))
                        .child(div().flex_1().child(Input::new(input))),
                );
            }
            let (fields2, app2) = (fields.clone(), app.clone());
            let save = Rc::new(move |window: &mut Window, cx: &mut App| {
                let v = |i: usize| fields2[i].1.read(cx).value().trim().to_owned();
                let (name, host) = (v(0), v(2));
                if name.is_empty() || host.is_empty() {
                    window
                        .push_notification(Notification::warning("Name and host are required"), cx);
                    return;
                }
                let mut p = HostProfile::new(name, host);
                p.folder = v(1);
                p.port = v(3).parse().unwrap_or(22);
                p.user = Some(v(4)).filter(|u| !u.is_empty());
                let key = v(5);
                p.auth = crate::backend::quick_profile("x")
                    .map(|q| q.auth)
                    .unwrap_or_default();
                if !key.is_empty() {
                    p.auth.insert(
                        1,
                        AuthMethod::KeyFile {
                            path: key.into(),
                            passphrase_credential: None,
                        },
                    );
                }
                let jump = v(6);
                if !jump.is_empty() {
                    match hostwick_core::Target::parse(&jump) {
                        Ok(t) => {
                            p.jump = vec![hostwick_core::Endpoint {
                                port: t.port_or_default(),
                                host: t.host,
                                user: t.user,
                            }]
                        }
                        Err(e) => {
                            window.push_notification(
                                Notification::error(format!("Jump host: {e}")),
                                cx,
                            );
                            return;
                        }
                    }
                }
                let _ = app2.update(cx, |a, cx| {
                    match a.backend.store.upsert(&p) {
                        Ok(()) => {
                            a.selected = Some(p.id);
                            a.reload_profiles();
                        }
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                    }
                    cx.notify();
                });
                window.close_dialog(cx);
            });
            let (save_click, save_enter) = (save.clone(), save.clone());
            dialog
                .title("New host")
                .w(px(480.0))
                .child(body)
                .on_ok(move |_, window, cx| {
                    save_enter(window, cx);
                    false
                })
                .footer(
                    Button::new("save")
                        .primary()
                        .label("Save")
                        .on_click(move |_, window, cx| save_click(window, cx)),
                )
        });
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self
            .selected
            .and_then(|id| self.profiles.iter().find(|p| p.id == id))
            .cloned()
        else {
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
                                    a.selected = None;
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
        let from_registry = cfg!(windows) && matches!(which, "putty" | "winscp");
        if which == "ssh" || from_registry {
            let result = match which {
                "ssh" => std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::Path::new(&h).join(".ssh").join("config"))
                    .ok_or_else(|| "no home directory".to_owned())
                    .and_then(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
                    .map(|b| hostwick_import::parse_ssh_config(&hostwick_import::decode_text(&b))),
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
            prompt: Some(if which == "putty" {
                "PuTTY .reg export".into()
            } else {
                "WinSCP.ini".into()
            }),
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
            let result = if which == "putty" {
                hostwick_import::parse_putty_reg(&text)
            } else {
                hostwick_import::parse_winscp_ini(&text)
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
        let (mut added, mut updated) = (0, 0);
        for mut p in r.profiles {
            if let Some(prior) = self
                .profiles
                .iter()
                .find(|e| e.source == p.source && p.source != Default::default())
            {
                p.id = prior.id;
                updated += 1;
            } else {
                added += 1;
            }
            let _ = self.backend.store.upsert(&p);
        }
        self.reload_profiles();
        let mut msg = format!("Imported: {added} added, {updated} updated");
        if !r.warnings.is_empty() {
            msg.push_str(&format!(", {} skipped", r.warnings.len()));
            for w in &r.warnings {
                tracing::warn!("import: {w}");
            }
        }
        window.push_notification(Notification::success(msg), cx);
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
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Hostwick"),
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
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted_foreground)
                                                .whitespace_nowrap()
                                                .child(p.host.clone()),
                                        )
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
                                Button::new("del-host")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .disabled(!has_sel)
                                    .on_click(
                                        cx.listener(|app, _, w, cx| app.delete_selected(w, cx)),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("new-host")
                            .small()
                            .label("New host…")
                            .on_click(cx.listener(|app, _, w, cx| app.new_host_dialog(w, cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.muted_foreground)
                                    .child("Import"),
                            )
                            .child(
                                Button::new("imp-ssh")
                                    .small()
                                    .ghost()
                                    .label("ssh_config")
                                    .on_click(cx.listener(|a, _, w, cx| a.import("ssh", w, cx))),
                            )
                            .child(
                                Button::new("imp-putty")
                                    .small()
                                    .ghost()
                                    .label("PuTTY")
                                    .on_click(cx.listener(|a, _, w, cx| a.import("putty", w, cx))),
                            )
                            .child(
                                Button::new("imp-winscp")
                                    .small()
                                    .ghost()
                                    .label("WinSCP")
                                    .on_click(cx.listener(|a, _, w, cx| a.import("winscp", w, cx))),
                            ),
                    ),
            )
            .into_any_element()
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
                .child("Import from ~/.ssh/config, PuTTY or WinSCP at the bottom left.")
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
