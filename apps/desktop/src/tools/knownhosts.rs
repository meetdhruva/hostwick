//! "Known hosts…": the known_hosts file as a searchable list; remove
//! selected lines or every key of a host (like `ssh-keygen -R`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_ssh::{KnownHostEntry, KnownHosts};

use crate::backend::Backend;

pub struct KnownHostsView {
    backend: Arc<Backend>,
    search: Entity<InputState>,
    forget: Entity<InputState>,
    entries: Option<Vec<KnownHostEntry>>,
    selected: BTreeSet<usize>,
    busy: bool,
    message: Option<(bool, String)>,
    _subs: Vec<Subscription>,
}

/// Does the entry match every word of the search?
pub fn matches(e: &KnownHostEntry, needle: &str) -> bool {
    let hay = format!(
        "{} {} {} {} {}",
        e.hosts_display(),
        e.key_type,
        e.fingerprint,
        e.comment,
        e.marker.map(|m| m.as_str()).unwrap_or("")
    )
    .to_lowercase();
    needle
        .to_lowercase()
        .split_whitespace()
        .all(|w| hay.contains(w))
}

/// `host`, `host:port` or `[host]:port`.
pub fn parse_host_port(text: &str) -> Option<(String, u16)> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('[') {
        let (h, p) = rest.split_once("]:")?;
        return Some((h.to_owned(), p.parse().ok()?));
    }
    match t.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => Some((h.to_owned(), p.parse().ok()?)),
        _ => Some((t.to_owned(), 22)),
    }
}

impl KnownHostsView {
    fn new(backend: Arc<Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("kh-search-placeholder")));
        let forget =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("kh-forget-placeholder")));
        let subs = vec![cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                cx.notify();
            }
        })];
        let mut v = Self {
            backend,
            search,
            forget,
            entries: None,
            selected: BTreeSet::new(),
            busy: false,
            message: None,
            _subs: subs,
        };
        v.reload(window, cx);
        v
    }

    fn known_hosts(&self) -> KnownHosts {
        KnownHosts::new(&self.backend.paths.known_hosts)
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kh = self.known_hosts();
        let job = self.backend.rt.spawn_blocking(move || kh.entries());
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                match res {
                    Ok(Ok(e)) => v.entries = Some(e),
                    Ok(Err(e)) => {
                        v.entries = Some(Vec::new());
                        v.message = Some((true, e.to_string()));
                    }
                    Err(e) => {
                        v.message = Some((true, t!("error-internal", error = e.to_string()).into()))
                    }
                }
                v.selected.retain(|l| {
                    v.entries
                        .as_ref()
                        .is_some_and(|es| es.iter().any(|e| e.line == *l))
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn apply(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(&KnownHosts) -> Result<usize, String> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        let kh = self.known_hosts();
        let job = self.backend.rt.spawn_blocking(move || work(&kh));
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |v, window, cx| {
                v.busy = false;
                v.message = Some(match res {
                    Ok(Ok(0)) => (false, t!("kh-nothing-matched").into()),
                    Ok(Ok(n)) => (false, t!("kh-removed", count = n).into()),
                    Ok(Err(e)) => (true, e),
                    Err(e) => (true, t!("error-internal", error = e.to_string()).into()),
                });
                v.selected.clear();
                v.reload(window, cx);
            });
        })
        .detach();
    }

    fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lines: Vec<usize> = self.selected.iter().copied().collect();
        self.apply(window, cx, move |kh| {
            kh.remove_lines(&lines).map_err(|e| e.to_string())
        });
    }

    fn forget_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.forget.read(cx).value().to_string();
        let Some((host, port)) = parse_host_port(&text) else {
            self.message = Some((true, t!("kh-type-host").into()));
            cx.notify();
            return;
        };
        self.apply(window, cx, move |kh| {
            kh.remove_host(&host, port).map_err(|e| e.to_string())
        });
    }
}

impl Render for KnownHostsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let needle = self.search.read(cx).value().to_string();
        let rows: Vec<KnownHostEntry> = self
            .entries
            .as_ref()
            .map(|es| es.iter().filter(|e| matches(e, &needle)).cloned().collect())
            .unwrap_or_default();
        let total = self.entries.as_ref().map(|e| e.len()).unwrap_or(0);
        let header = div()
            .flex()
            .gap(px(8.0))
            .px(px(6.0))
            .py(px(3.0))
            .text_size(px(11.0))
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(24.0)))
            .child(div().w(px(44.0)).child(t!("kh-col-line")))
            .child(div().flex_1().child(t!("kh-col-hosts")))
            .child(div().w(px(150.0)).child(t!("keygen-type")))
            .child(div().w(px(250.0)).child(t!("keyconv-fingerprint")));
        let list =
            div()
                .id("kh-list")
                .h(px(330.0))
                .overflow_y_scroll()
                .border_1()
                .border_color(theme.border)
                .rounded(px(4.0))
                .child(header)
                .when(self.entries.is_none(), |d| {
                    d.child(
                        div()
                            .p(px(8.0))
                            .text_color(theme.muted_foreground)
                            .child(t!("loading")),
                    )
                })
                .when(self.entries.is_some() && rows.is_empty(), |d| {
                    d.child(div().p(px(8.0)).text_color(theme.muted_foreground).child(
                        if total == 0 {
                            t!("kh-empty")
                        } else {
                            t!("kh-no-match")
                        },
                    ))
                })
                .children(rows.iter().map(|e| {
                    let line = e.line;
                    let sel = self.selected.contains(&line);
                    let hosts = match e.marker {
                        Some(m) => format!("{} {}", m.as_str(), e.hosts_display()),
                        None => e.hosts_display(),
                    };
                    div()
                        .id(("kh", line))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(6.0))
                        .py(px(2.0))
                        .text_size(px(12.0))
                        .when(sel, |d| d.bg(theme.accent))
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if !v.selected.remove(&line) {
                                v.selected.insert(line);
                            }
                            cx.notify();
                        }))
                        .child(
                            div().w(px(24.0)).child(
                                Checkbox::new(("khc", line))
                                    .accessibility_label(t!(
                                        "kh-row-label",
                                        line = line,
                                        hosts = hosts.clone()
                                    ))
                                    .checked(sel)
                                    .on_click(cx.listener(move |v, on: &bool, _, cx| {
                                        if *on {
                                            v.selected.insert(line);
                                        } else {
                                            v.selected.remove(&line);
                                        }
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            div()
                                .w(px(44.0))
                                .text_color(theme.muted_foreground)
                                .child(line.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(hosts),
                        )
                        .child(
                            div()
                                .w(px(150.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(e.key_type.clone()),
                        )
                        .child(
                            div()
                                .w(px(250.0))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(crate::terminal::monospace_family())
                                .text_size(px(11.0))
                                .child(e.fingerprint.clone()),
                        )
                }));
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.search).small().cleanable(true)),
                    )
                    .child(
                        Button::new("kh-reload")
                            .small()
                            .label(t!("kh-reload"))
                            .on_click(cx.listener(|v, _, w, cx| v.reload(w, cx))),
                    ),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Button::new("kh-remove")
                            .small()
                            .danger()
                            .label(t!("kh-remove-selected", count = self.selected.len()))
                            .disabled(self.selected.is_empty() || self.busy)
                            .on_click(cx.listener(|v, _, w, cx| v.remove_selected(w, cx))),
                    )
                    .child(div().flex_1())
                    .child(div().text_size(px(13.0)).child(t!("kh-forget-host")))
                    .child(
                        div().w(px(200.0)).child(
                            Input::new(&self.forget)
                                .small()
                                .aria_label(t!("kh-forget-host")),
                        ),
                    )
                    .child(
                        Button::new("kh-forget")
                            .small()
                            .label(t!("kh-forget-button"))
                            .disabled(self.busy)
                            .on_click(cx.listener(|v, _, w, cx| v.forget_host(w, cx))),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .whitespace_normal()
                    .text_color(theme.muted_foreground)
                    .child(t!(
                        "kh-footer",
                        path = self.backend.paths.known_hosts.display().to_string(),
                        shown = rows.len(),
                        total = total
                    )),
            )
            .when_some(self.message.clone(), |d, (err, m)| {
                d.child(
                    div()
                        .text_size(px(13.0))
                        .text_color(if err { theme.danger } else { theme.success })
                        .child(m),
                )
            })
    }
}

pub fn open(backend: Arc<Backend>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| KnownHostsView::new(backend, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(crate::a11y::text(t!("kh-title")))
            .w(px(860.0))
            .margin_top(px(40.0))
            .child(view.clone())
            .footer(
                div().flex().child(
                    Button::new("close")
                        .label(t!("button-close"))
                        .on_click(|_, w, cx| w.close_dialog(cx)),
                ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::{matches, parse_host_port};
    use hostwick_ssh::KnownHostEntry;

    #[test]
    fn search_and_host_parsing() {
        let e = KnownHostEntry {
            line: 3,
            marker: None,
            hosts: vec!["[web]:2222".into(), "10.0.0.5".into()],
            hashed: false,
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
            comment: String::new(),
        };
        assert!(matches(&e, "web ed25519"));
        assert!(matches(&e, ""));
        assert!(!matches(&e, "rsa"));
        assert_eq!(parse_host_port("web"), Some(("web".into(), 22)));
        assert_eq!(parse_host_port("web:2222"), Some(("web".into(), 2222)));
        assert_eq!(parse_host_port("[::1]:2222"), Some(("::1".into(), 2222)));
        assert_eq!(parse_host_port("::1"), Some(("::1".into(), 22)));
        assert_eq!(parse_host_port(""), None);
    }
}
