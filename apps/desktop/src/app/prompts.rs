//! Questions from the SSH layer as dialogs, one at a time: host keys,
//! passwords (with "Remember"), keyboard-interactive, the vault; and
//! notices as rate-limited notifications.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_ssh::{HostKeyDecision, HostKeyStatus};

use super::{HostwickApp, answer, slot};
use crate::backend::{Prompt, RememberOffer, SecretAnswer};

/// Same notice again within this window is dropped.
const NOTICE_DEDUP: Duration = Duration::from_secs(60);
/// At most this many notices per `NOTICE_WINDOW`.
const NOTICE_BURST: usize = 4;
const NOTICE_WINDOW: Duration = Duration::from_secs(10);

/// Whether to show a notice now, updating the bookkeeping.
pub(crate) fn admit_notice(
    seen: &mut std::collections::HashMap<String, Instant>,
    times: &mut std::collections::VecDeque<Instant>,
    text: &str,
    now: Instant,
) -> bool {
    seen.retain(|_, t| now.duration_since(*t) < NOTICE_DEDUP);
    while times
        .front()
        .is_some_and(|t| now.duration_since(*t) >= NOTICE_WINDOW)
    {
        times.pop_front();
    }
    if seen.contains_key(text) || times.len() >= NOTICE_BURST {
        return false;
    }
    seen.insert(text.to_owned(), now);
    times.push_back(now);
    true
}

impl HostwickApp {
    pub(super) fn enqueue_prompt(
        &mut self,
        p: Prompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match p {
            Prompt::Banner(text) => {
                window.push_notification(Notification::info(text), cx);
                return;
            }
            Prompt::Notice(text) => {
                if admit_notice(
                    &mut self.notices,
                    &mut self.notice_times,
                    &text,
                    Instant::now(),
                ) {
                    tracing::info!(notice = %text);
                    window.push_notification(Notification::warning(text).title("Connection"), cx);
                }
                return;
            }
            Prompt::ProfilesChanged => {
                self.reload_profiles();
                for t in &mut self.tabs {
                    for pane in t.root.leaves_mut() {
                        if let Some(p) = self.profiles.iter().find(|p| p.id == pane.profile.id) {
                            pane.profile = p.clone();
                        }
                    }
                }
                self.mark_sync_dirty();
                cx.notify();
                return;
            }
            p => self.prompts.push_back(p),
        }
        if !self.prompt_open {
            self.show_next_prompt(window, cx);
        }
    }

    pub(super) fn prompt_done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            Prompt::Banner(_) | Prompt::Notice(_) | Prompt::ProfilesChanged => {
                self.prompt_done(window, cx)
            }
            Prompt::Vault {
                create,
                reason,
                reply,
            } => {
                let reply = slot(reply);
                crate::vaultui::open_unlock(
                    self.backend.clone(),
                    create,
                    reason,
                    window,
                    cx,
                    move |ok, window, cx| {
                        answer(&reply, ok);
                        let _ = app.update(cx, |a, cx| {
                            a.prompt_done(window, cx);
                            cx.notify();
                        });
                    },
                );
            }
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
                note,
                remember,
                reply,
            } => {
                let reply = slot(reply);
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder(label).masked(true));
                let remember_cell =
                    Rc::new(Cell::new(remember.as_ref().is_some_and(|r| r.checked)));
                let rc = remember_cell.clone();
                self.secret_dialog(
                    title,
                    vec![input],
                    note.unwrap_or_default(),
                    remember.map(|r| (r, remember_cell)),
                    window,
                    cx,
                    move |values| {
                        answer(
                            &reply,
                            values
                                .and_then(|v| v.into_iter().next())
                                .map(|value| SecretAnswer {
                                    value,
                                    remember: rc.get(),
                                }),
                        );
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
                self.secret_dialog(title, inputs, text, None, window, cx, move |values| {
                    answer(&reply, values)
                });
            }
        }
    }

    /// Dialog with one or more (masked) inputs and an optional "Remember"
    /// checkbox. `done(None)` on cancel.
    #[allow(clippy::too_many_arguments)]
    fn secret_dialog(
        &mut self,
        title: String,
        inputs: Vec<Entity<InputState>>,
        text: String,
        remember: Option<(RememberOffer, Rc<Cell<bool>>)>,
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
        let first = Rc::new(Cell::new(true));
        let vault_open = self.backend.vault.is_unlocked();
        let vault_exists = self.backend.vault.exists();
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false)
                && let Some(i) = inputs.first()
            {
                i.focus_handle(cx).focus(window, cx);
            }
            let theme = cx.theme().clone();
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
            if let Some((offer, cell)) = &remember {
                let c = cell.clone();
                let hint = if vault_open {
                    "Saved encrypted in your password vault."
                } else if vault_exists {
                    "You'll be asked for your vault's master password."
                } else {
                    "You'll create a password vault with a master password."
                };
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            Checkbox::new("remember")
                                .label(offer.label.clone())
                                .checked(cell.get())
                                .on_click(move |v, window, _| {
                                    c.set(*v);
                                    window.refresh();
                                }),
                        )
                        .when(cell.get(), |d| {
                            d.child(
                                div()
                                    .pl(px(24.0))
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground)
                                    .child(hint),
                            )
                        }),
                );
            }
            dialog
                .title(title.clone())
                .w(px(440.0))
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
}

#[cfg(test)]
mod tests {
    use super::admit_notice;
    use std::time::{Duration, Instant};

    #[test]
    fn notices_are_deduplicated_and_rate_limited() {
        let (mut seen, mut times) = Default::default();
        let t0 = Instant::now();
        assert!(admit_notice(&mut seen, &mut times, "x11: no display", t0));
        assert!(!admit_notice(&mut seen, &mut times, "x11: no display", t0));
        for i in 0..3 {
            assert!(admit_notice(&mut seen, &mut times, &format!("n{i}"), t0));
        }
        // Burst of four reached.
        assert!(!admit_notice(&mut seen, &mut times, "another", t0));
        let later = t0 + Duration::from_secs(11);
        assert!(admit_notice(&mut seen, &mut times, "another", later));
        assert!(!admit_notice(
            &mut seen,
            &mut times,
            "x11: no display",
            later
        ));
        let much_later = t0 + Duration::from_secs(61);
        assert!(admit_notice(
            &mut seen,
            &mut times,
            "x11: no display",
            much_later
        ));
    }
}
