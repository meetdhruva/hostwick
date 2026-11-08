//! Vault dialogs: unlock, create (with a strength hint), change the master
//! password. Argon2 runs on a blocking thread; the dialog shows progress
//! and keeps itself open on a wrong password.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::SecretString;
use hostwick_vault::VaultError;

use crate::backend::Backend;
use crate::vault::strength_hint;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unlock,
    Create,
    Change,
}

type Done = Box<dyn FnOnce(bool, &mut Window, &mut App)>;

pub struct VaultForm {
    backend: Arc<Backend>,
    mode: Mode,
    reason: String,
    old: Entity<InputState>,
    pw: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<String>,
    busy: bool,
    done: Option<Done>,
    /// Set once the dialog is finished or dismissed.
    closed: Rc<Cell<bool>>,
    _subs: Vec<Subscription>,
}

impl VaultForm {
    fn new(
        backend: Arc<Backend>,
        mode: Mode,
        reason: String,
        done: Done,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mk = |ph: SharedString, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).masked(true))
        };
        let old = mk(t!("vault-current-master"), window, cx);
        let pw = mk(
            if mode == Mode::Unlock {
                t!("vault-master")
            } else {
                t!("vault-new-master")
            },
            window,
            cx,
        );
        let confirm = mk(t!("vault-repeat"), window, cx);
        let mut subs = Vec::new();
        for i in [&old, &pw, &confirm] {
            subs.push(cx.subscribe_in(
                i,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => {
                        this.error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            ));
        }
        Self {
            backend,
            mode,
            reason,
            old,
            pw,
            confirm,
            error: None,
            busy: false,
            done: Some(done),
            closed: Rc::new(Cell::new(false)),
            _subs: subs,
        }
    }

    fn first_input(&self) -> &Entity<InputState> {
        if self.mode == Mode::Change {
            &self.old
        } else {
            &self.pw
        }
    }

    fn clear(&self, window: &mut Window, cx: &mut Context<Self>) {
        for i in [&self.old, &self.pw, &self.confirm] {
            i.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    fn finish(&mut self, ok: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(window, cx);
        if self.closed.replace(true) {
            return;
        }
        if let Some(done) = self.done.take() {
            window.defer(cx, |window, cx| window.close_dialog(cx));
            done(ok, window, cx);
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let pw = self.pw.read(cx).value().to_string();
        let confirm = self.confirm.read(cx).value().to_string();
        let old = self.old.read(cx).value().to_string();
        if pw.is_empty() {
            self.error = Some(t!("vault-enter-password").into());
            cx.notify();
            return;
        }
        if self.mode != Mode::Unlock {
            if strength_hint(&pw).0 < 2 {
                self.error = Some(t!("vault-too-weak").into());
                cx.notify();
                return;
            }
            if pw != confirm {
                self.error = Some(t!("vault-mismatch").into());
                cx.notify();
                return;
            }
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let vault = self.backend.vault.clone();
        let mode = self.mode;
        let job = self.backend.rt.spawn_blocking(move || {
            let pw = SecretString::from(pw);
            match mode {
                Mode::Unlock => vault.unlock(pw).map_err(|e| match e {
                    VaultError::WrongPassword => t!("vault-wrong-master").to_string(),
                    e => e.to_string(),
                }),
                Mode::Create => vault.create(pw).map_err(|e| e.to_string()),
                Mode::Change => vault.change_master(&SecretString::from(old), pw),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |f, window, cx| {
                f.busy = false;
                match res {
                    Ok(Ok(())) => f.finish(true, window, cx),
                    Ok(Err(e)) => {
                        f.error = Some(e);
                        f.first_input().clone().update(cx, |s, cx| {
                            s.set_value("", window, cx);
                            s.focus(window, cx);
                        });
                    }
                    Err(e) => f.error = Some(t!("error-internal", error = e.to_string()).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for VaultForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let pw = self.pw.read(cx).value().to_string();
        let (level, hint) = strength_hint(&pw);
        let strength_color = match level {
            0 | 1 => theme.danger,
            2 => theme.warning,
            _ => theme.success,
        };
        let path = self.backend.vault.path().display().to_string();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(px(13.0))
            .when(!self.reason.is_empty(), |d| {
                d.child(
                    div()
                        .whitespace_normal()
                        .child(crate::a11y::text(self.reason.clone())),
                )
            })
            .when(self.mode == Mode::Change, |d| {
                d.child(Input::new(&self.old).mask_toggle())
            })
            .child(Input::new(&self.pw).mask_toggle())
            .when(self.mode != Mode::Unlock, |d| {
                d.child(Input::new(&self.confirm).mask_toggle())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(div().flex().gap(px(3.0)).children((1..=3).map(|i| {
                                div()
                                    .w(px(28.0))
                                    .h(px(4.0))
                                    .rounded(px(2.0))
                                    .bg(if i <= level {
                                        strength_color
                                    } else {
                                        theme.muted
                                    })
                            })))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground)
                                    .child(if hint.is_empty() {
                                        t!("vault-strength-empty")
                                    } else {
                                        hint
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(t!("vault-never-stored")),
                    )
            })
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(theme.danger).child(e))
            })
            .when(self.busy, |d| {
                d.child(div().text_color(theme.muted_foreground).child(
                    if self.mode == Mode::Unlock {
                        t!("vault-unlocking")
                    } else {
                        t!("vault-encrypting")
                    },
                ))
            })
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(t!("vault-path", path = path)),
            )
    }
}

fn open(
    backend: Arc<Backend>,
    mode: Mode,
    reason: String,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(bool, &mut Window, &mut App) + 'static,
) {
    let form = cx.new(|cx| VaultForm::new(backend, mode, reason, Box::new(done), window, cx));
    let first = std::rc::Rc::new(std::cell::Cell::new(true));
    let title = match mode {
        Mode::Unlock => t!("vault-title-unlock"),
        Mode::Create => t!("vault-title-create"),
        Mode::Change => t!("vault-title-change"),
    };
    let closed = form.read(cx).closed.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let input = form.read(cx).first_input().clone();
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        let busy = form.read(cx).busy;
        let (f1, f2, f3) = (form.clone(), form.clone(), form.clone());
        let c = closed.clone();
        dialog
            .title(crate::a11y::text(title.clone()))
            .w(px(460.0))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                f1.update(cx, |f, cx| f.submit(window, cx));
                false
            })
            .on_close(move |_, window, cx| {
                if !c.replace(true) {
                    f3.update(cx, |f, cx| {
                        f.clear(window, cx);
                        if let Some(done) = f.done.take() {
                            done(false, window, cx);
                        }
                    });
                }
            })
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("vault-cancel")
                            .label(if mode == Mode::Unlock {
                                t!("vault-not-now")
                            } else {
                                t!("button-cancel")
                            })
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("vault-ok")
                            .primary()
                            .disabled(busy)
                            .label(match mode {
                                Mode::Unlock => t!("vault-unlock"),
                                Mode::Create => t!("vault-create"),
                                Mode::Change => t!("vault-change"),
                            })
                            .on_click(move |_, window, cx| {
                                f2.update(cx, |f, cx| f.submit(window, cx))
                            }),
                    ),
            )
    });
}

/// Unlock the vault (or create it). `done(true)` once it is open.
pub fn open_unlock(
    backend: Arc<Backend>,
    create: bool,
    reason: String,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(bool, &mut Window, &mut App) + 'static,
) {
    let mode = if create { Mode::Create } else { Mode::Unlock };
    open(backend, mode, reason, window, cx, done);
}

/// Change the master password.
pub fn open_change(
    backend: Arc<Backend>,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(bool, &mut Window, &mut App) + 'static,
) {
    open(backend, Mode::Change, String::new(), window, cx, done);
}
