//! Bulk edit: change user, port, keys, jump hosts, proxy, folder,
//! keepalive, file protocol or terminal type of many hosts at once. A field
//! starts with the value all selected hosts share (empty when they differ);
//! only fields the person changes are applied.

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{FileProtocol, HostProfile, ProfileId, Protocol, ProxySpec};

use super::HostwickApp;
use crate::hostform::{jumps_to_text, parse_jumps, parse_port};
use crate::hostops::{self, BulkEdit, common};
use crate::notify::{Toast, ToastExt as _};

/// One text field and the value it started with.
struct Field {
    input: Entity<InputState>,
    initial: String,
}

impl Field {
    fn value(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_owned()
    }

    /// The new value, if the person changed it.
    fn changed(&self, cx: &App) -> Option<String> {
        let v = self.value(cx);
        (v != self.initial).then_some(v)
    }
}

pub(crate) struct BulkEditView {
    app: WeakEntity<HostwickApp>,
    ids: Vec<ProfileId>,
    total: usize,
    ssh: usize,
    serial: usize,
    user: Field,
    port: Field,
    keys: Field,
    jump: Field,
    proxy: Field,
    folder: Field,
    keepalive: Field,
    term: Field,
    /// None = keep each host's own.
    file_protocol: Option<FileProtocol>,
    fp_initial: Option<FileProtocol>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl BulkEditView {
    fn new(
        app: WeakEntity<HostwickApp>,
        hosts: &[HostProfile],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mixed = t!("bulk-mixed");
        let mut subs = Vec::new();
        let mut field = |initial: Option<String>, cx: &mut Context<Self>| {
            // None: the hosts differ (say so); Some: their shared value.
            let ph = if initial.is_none() {
                mixed.clone()
            } else {
                SharedString::default()
            };
            let initial = initial.unwrap_or_default();
            let value = initial.clone();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(ph)
                    .default_value(value)
            });
            subs.push(cx.subscribe(&input, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }));
            Field { input, initial }
        };
        let net: Vec<&HostProfile> = hosts
            .iter()
            .filter(|p| !matches!(p.protocol, Protocol::Serial(_)))
            .collect();
        let ssh: Vec<&HostProfile> = hosts
            .iter()
            .filter(|p| p.protocol == Protocol::Ssh)
            .collect();
        let user = field(
            common(net.iter().map(|p| p.user.clone())).map(|u| u.unwrap_or_default()),
            cx,
        );
        let port = field(
            common(net.iter().map(|p| p.port)).map(|p| p.to_string()),
            cx,
        );
        let keys = field(common(ssh.iter().map(|p| hostops::key_text(p))), cx);
        let jump = field(common(ssh.iter().map(|p| jumps_to_text(&p.jump))), cx);
        let proxy = field(
            common(net.iter().map(|p| p.proxy.as_ref().map(ProxySpec::to_url)))
                .map(|u| u.unwrap_or_default()),
            cx,
        );
        let folder = field(
            common(hosts.iter().map(|p| hostops::normalize(&p.folder))),
            cx,
        );
        let keepalive = field(
            common(net.iter().map(|p| p.keepalive_secs)).map(|k| k.to_string()),
            cx,
        );
        let term = field(common(net.iter().map(|p| p.term.clone())), cx);
        let fp = common(ssh.iter().map(|p| p.file_protocol));
        Self {
            app,
            ids: hosts.iter().map(|p| p.id).collect(),
            total: hosts.len(),
            ssh: ssh.len(),
            serial: hosts.len() - net.len(),
            user,
            port,
            keys,
            jump,
            proxy,
            folder,
            keepalive,
            term,
            file_protocol: fp,
            fp_initial: fp,
            error: None,
            _subs: subs,
        }
    }

    /// The changes, or what is wrong with a field.
    fn build(&self, cx: &App) -> Result<BulkEdit, String> {
        let mut e = BulkEdit::default();
        if let Some(u) = self.user.changed(cx) {
            e.user = Some(Some(u).filter(|u| !u.is_empty()));
        }
        if let Some(p) = self.port.changed(cx) {
            e.port = Some(parse_port(&p, &t!("host-port"))?);
        }
        if let Some(k) = self.keys.changed(cx) {
            e.keys = Some(hostops::parse_keys(&k));
        }
        if let Some(j) = self.jump.changed(cx) {
            e.jump = Some(parse_jumps(&j)?);
        }
        let proxy = self.proxy.value(cx);
        if proxy == "-" || proxy.eq_ignore_ascii_case("none") {
            e.proxy = Some(None);
        } else if let Some(p) = self.proxy.changed(cx) {
            e.proxy = Some(if p.is_empty() {
                None
            } else {
                Some(
                    ProxySpec::parse_url(&p)
                        .map_err(|err| t!("bulk-error-proxy", error = err).to_string())?,
                )
            });
        }
        if let Some(f) = self.folder.changed(cx) {
            e.folder = Some(hostops::normalize(&f));
        }
        if let Some(k) = self.keepalive.changed(cx) {
            e.keepalive = Some(
                k.parse::<u32>()
                    .ok()
                    .filter(|k| *k <= 3600)
                    .ok_or_else(|| t!("host-error-keepalive").to_string())?,
            );
        }
        if let Some(t) = self.term.changed(cx) {
            e.term = Some(if t.is_empty() {
                "xterm-256color".into()
            } else {
                t
            });
        }
        if self.file_protocol != self.fp_initial {
            e.file_protocol = self.file_protocol;
        }
        Ok(e)
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edit = match self.build(cx) {
            Ok(e) => e,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        window.close_dialog(cx);
        if edit.is_empty() {
            return;
        }
        let ids = self.ids.clone();
        let _ = self.app.update(cx, |a, cx| {
            let changed: Vec<HostProfile> = a
                .profiles
                .iter()
                .filter(|p| ids.contains(&p.id))
                .cloned()
                .filter_map(|mut p| edit.apply(&mut p).then_some(p))
                .collect();
            let n = changed.len();
            a.save_profiles(changed, cx);
            window.toast(Toast::success(t!("bulk-done", count = n)), cx);
        });
    }

    fn row(&self, label: SharedString, field: &Field, hint: Option<SharedString>, cx: &App) -> Div {
        let theme = cx.theme();
        let changed = field.changed(cx).is_some();
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(150.0))
                            .flex_none()
                            .text_size(px(13.0))
                            .child(label.clone()),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(label.clone()))
                            .role(Role::Group)
                            .aria_label(label)
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&field.input).small()),
                    )
                    .child(
                        div()
                            .w(px(70.0))
                            .flex_none()
                            .text_xs()
                            .text_color(theme.link)
                            .when(changed, |d| d.child(crate::a11y::text(t!("bulk-changed")))),
                    ),
            )
            .when_some(hint, |d, h| {
                d.child(
                    div()
                        .pl(px(158.0))
                        .pr(px(78.0))
                        .text_size(px(11.0))
                        .whitespace_normal()
                        .text_color(theme.muted_foreground)
                        .child(crate::a11y::text(h)),
                )
            })
    }
}

impl Render for BulkEditView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fp_choices = [
            (None, t!("bulk-keep")),
            (Some(FileProtocol::Auto), t!("host-file-protocol-auto")),
            (Some(FileProtocol::Sftp), SharedString::from("SFTP")),
            (Some(FileProtocol::Scp), SharedString::from("SCP")),
        ];
        let mut fp_row = div().flex().gap(px(4.0)).flex_wrap();
        for (i, (value, label)) in fp_choices.into_iter().enumerate() {
            let on = self.file_protocol == value;
            fp_row = fp_row.child(
                Button::new(("bulk-fp", i))
                    .xsmall()
                    .label(label)
                    .toggled(on)
                    .when(on, |b| b.primary())
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.file_protocol = value;
                        cx.notify();
                    })),
            );
        }
        let only_ssh = (self.ssh < self.total)
            .then(|| t!("bulk-only-ssh", ssh = self.ssh, total = self.total));
        let no_serial = (self.serial > 0).then(|| t!("bulk-serial-skipped", count = self.serial));
        div()
            .id("bulk-edit")
            .role(Role::Group)
            .aria_label(t!("bulk-title", count = self.total))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(crate::a11y::text(t!("bulk-intro", count = self.total))),
            )
            .child(self.row(t!("host-user"), &self.user, no_serial.clone(), cx))
            .child(self.row(t!("host-port"), &self.port, None, cx))
            .child(self.row(t!("host-key-files"), &self.keys, only_ssh.clone(), cx))
            .child(self.row(
                t!("host-jump-hosts"),
                &self.jump,
                Some(t!("host-jump-hint")),
                cx,
            ))
            .child(self.row(
                t!("host-proxy"),
                &self.proxy,
                Some(t!("bulk-proxy-hint")),
                cx,
            ))
            .child(self.row(t!("host-folder"), &self.folder, None, cx))
            .child(self.row(t!("host-keepalive"), &self.keepalive, None, cx))
            .child(self.row(t!("host-terminal-type"), &self.term, None, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(150.0))
                            .flex_none()
                            .text_size(px(13.0))
                            .child(t!("host-file-protocol")),
                    )
                    .child(
                        div()
                            .id("bulk-fp-group")
                            .role(Role::Group)
                            .aria_label(t!("host-file-protocol"))
                            .flex_1()
                            .child(fp_row),
                    ),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .id("bulk-error")
                        .role(Role::Alert)
                        .aria_label(e.clone())
                        .text_size(px(13.0))
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }
}

impl HostwickApp {
    /// The bulk editor for `ids`.
    pub(crate) fn bulk_edit(
        &mut self,
        ids: Vec<ProfileId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hosts: Vec<HostProfile> = self
            .profiles
            .iter()
            .filter(|p| ids.contains(&p.id))
            .cloned()
            .collect();
        if hosts.is_empty() {
            return;
        }
        if hosts.len() == 1 {
            let p = hosts.into_iter().next();
            self.host_dialog(p, window, cx);
            return;
        }
        let n = hosts.len();
        let app = cx.entity().downgrade();
        let view = cx.new(|cx| BulkEditView::new(app, &hosts, window, cx));
        let first = std::rc::Rc::new(std::cell::Cell::new(true));
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let input = view.read(cx).user.input.clone();
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            let (v1, v2) = (view.clone(), view.clone());
            let muted = cx.theme().muted_foreground;
            dialog
                .title(crate::a11y::text(t!("bulk-title", count = n)))
                .w(px(640.0))
                .margin_top(px(50.0))
                .child(view.clone())
                .on_ok(move |_, window, cx| {
                    v2.update(cx, |v, cx| v.apply(window, cx));
                    false
                })
                .footer(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.0))
                                .text_color(muted)
                                .child(crate::a11y::text(t!("bulk-applies", count = n))),
                        )
                        .child(
                            Button::new("cancel")
                                .label(t!("button-cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("apply")
                                .primary()
                                .label(t!("bulk-apply"))
                                .on_click(move |_, window, cx| {
                                    v1.update(cx, |v, cx| v.apply(window, cx))
                                }),
                        ),
                )
        });
    }
}
