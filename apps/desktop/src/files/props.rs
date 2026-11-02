//! Properties dialog: what a file is, its permissions (9 checkboxes plus
//! the special bits, kept in sync with an octal field), owner and group,
//! folder size and SHA-256 on demand, symlink target.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_sftp::{Entry, EntryKind, FsError, HashAlgo, RemoteFs};

use super::FilesView;
use super::pane::Side;
use crate::util::{human_size, local_time};

pub fn open(
    view: &mut FilesView,
    side: Side,
    entry: Entry,
    window: &mut Window,
    cx: &mut Context<FilesView>,
) {
    let fs = view.pane(side).fs.clone();
    let rt = view.backend.handle();
    let weak = cx.entity().downgrade();
    let props = cx.new(|cx| Props::new(fs, rt, entry, side, weak, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let busy = props.read(cx).busy;
        let (apply, close) = (props.clone(), props.clone());
        dialog
            .title(crate::a11y::text(t!("props-title")))
            .w(px(560.0))
            .child(props.clone())
            .on_ok(move |_, window, cx| {
                apply.update(cx, |p, cx| p.apply(window, cx));
                false
            })
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(Button::new("close").label(t!("button-close")).on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            close.update(cx, |p, cx| {
                                p.cancel_work();
                                let _ = p.view.update(cx, |v, cx| v.refocus(window, cx));
                            });
                        },
                    ))
                    .child({
                        let props = props.clone();
                        Button::new("apply")
                            .primary()
                            .label(if busy {
                                t!("props-applying")
                            } else {
                                t!("props-apply")
                            })
                            .disabled(busy)
                            .on_click(move |_, window, cx| {
                                props.update(cx, |p, cx| p.apply(window, cx));
                            })
                    }),
            )
    });
}

pub struct Props {
    fs: Arc<dyn RemoteFs>,
    rt: tokio::runtime::Handle,
    entry: Entry,
    side: Side,
    view: WeakEntity<FilesView>,
    /// Permission bits being edited (`0o7777`).
    mode: u32,
    octal: Entity<InputState>,
    uid: Entity<InputState>,
    gid: Entity<InputState>,
    names: (Option<String>, Option<String>),
    link_target: Option<String>,
    recursive: bool,
    checksum: Option<String>,
    usage: Option<String>,
    work: Vec<tokio::task::JoinHandle<()>>,
    busy: bool,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl Drop for Props {
    fn drop(&mut self) {
        self.cancel_work();
    }
}

/// `"755"`, `"0644"`, `"4755"` → bits; `None` if not octal or too large.
pub fn parse_octal(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.len() > 5 {
        return None;
    }
    let v = u32::from_str_radix(s, 8).ok()?;
    (v <= 0o7777).then_some(v)
}

/// New mode for an item under a recursive change: folders get `mode`;
/// files keep their own execute bits (so `755` on a tree does not make
/// every text file executable).
pub fn recursive_mode(mode: u32, current: Option<u32>, is_dir: bool) -> u32 {
    if is_dir {
        mode
    } else {
        (mode & !0o111) | (current.unwrap_or(0) & 0o111)
    }
}

impl Props {
    fn new(
        fs: Arc<dyn RemoteFs>,
        rt: tokio::runtime::Handle,
        entry: Entry,
        side: Side,
        view: WeakEntity<FilesView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mode = entry.permissions.unwrap_or(0) & 0o7777;
        let octal = cx.new(|cx| InputState::new(window, cx).default_value(format!("{mode:04o}")));
        let uid = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(entry.uid.map(|u| u.to_string()).unwrap_or_default())
        });
        let gid = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(entry.gid.map(|g| g.to_string()).unwrap_or_default())
        });
        let sub = cx.subscribe_in(
            &octal,
            window,
            |this: &mut Self, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev
                    && let Some(m) = parse_octal(&input.read(cx).value())
                    && m != this.mode
                {
                    this.mode = m;
                    cx.notify();
                }
            },
        );
        let p = Self {
            fs,
            rt,
            entry,
            side,
            view,
            mode,
            octal,
            uid,
            gid,
            names: (None, None),
            link_target: None,
            recursive: false,
            checksum: None,
            usage: None,
            work: Vec::new(),
            busy: false,
            error: None,
            _subs: vec![sub],
        };
        // Owner names and the link target, in the background.
        let (fs, e) = (p.fs.clone(), p.entry.clone());
        let job = p.rt.spawn(async move {
            let names = fs.lookup_ids(e.uid, e.gid).await;
            let target = if e.kind == EntryKind::Symlink {
                fs.read_link(&e.path).await.ok()
            } else {
                None
            };
            (names, target)
        });
        cx.spawn(async move |this, cx| {
            if let Ok((names, target)) = job.await {
                let _ = this.update(cx, |p, cx| {
                    p.names = names;
                    p.link_target = target;
                    cx.notify();
                });
            }
        })
        .detach();
        p
    }

    pub fn cancel_work(&mut self) {
        for w in self.work.drain(..) {
            w.abort();
        }
    }

    fn set_mode(&mut self, mode: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode & 0o7777;
        let text = format!("{:04o}", self.mode);
        self.octal.update(cx, |s, cx| s.set_value(text, window, cx));
        cx.notify();
    }

    fn calc_checksum(&mut self, cx: &mut Context<Self>) {
        self.checksum = Some(t!("props-calculating").into());
        let (fs, path) = (self.fs.clone(), self.entry.path.clone());
        let (tx, rx) = futures::channel::oneshot::channel();
        self.work.push(self.rt.spawn(async move {
            let r = fs.checksum(&path, HashAlgo::Sha256).await;
            let _ = tx.send(r);
        }));
        cx.spawn(async move |this, cx| {
            let text = match rx.await {
                Ok(Ok(h)) => h,
                Ok(Err(e)) => t!("props-error", error = e.to_string()).into(),
                Err(_) => return,
            };
            let _ = this.update(cx, |p, cx| {
                p.checksum = Some(text);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn calc_usage(&mut self, cx: &mut Context<Self>) {
        self.usage = Some(t!("props-calculating").into());
        let (fs, path) = (self.fs.clone(), self.entry.path.clone());
        let (tx, rx) = futures::channel::oneshot::channel();
        self.work.push(self.rt.spawn(async move {
            let r = fs.disk_usage(&path).await;
            let _ = tx.send(r);
        }));
        cx.spawn(async move |this, cx| {
            let text = match rx.await {
                Ok(Ok(u)) => {
                    let text = t!(
                        "props-usage",
                        size = human_size(u.bytes),
                        bytes = u.bytes,
                        files = u.files,
                        folders = u.dirs
                    );
                    if u.complete {
                        text.into()
                    } else {
                        format!("{text} {}", t!("props-usage-partial"))
                    }
                }
                Ok(Err(e)) => t!("props-error", error = e.to_string()).into(),
                Err(_) => return,
            };
            let _ = this.update(cx, |p, cx| {
                p.usage = Some(text);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let parse_id = |s: &Entity<InputState>, cx: &App| -> Result<Option<u32>, String> {
            let t = s.read(cx).value().trim().to_string();
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<u32>()
                .map(Some)
                .map_err(|_| t!("props-bad-id", id = &t).to_string())
        };
        let (uid, gid) = match (parse_id(&self.uid, cx), parse_id(&self.gid, cx)) {
            (Ok(u), Ok(g)) => (u, g),
            (Err(e), _) | (_, Err(e)) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        if parse_octal(&self.octal.read(cx).value()).is_none() {
            self.error = Some(t!("props-bad-octal").into());
            cx.notify();
            return;
        }
        let e = self.entry.clone();
        let mode = self.mode;
        let mode_changed = e.permissions.map(|p| p & 0o7777) != Some(mode);
        let uid = uid.filter(|u| Some(*u) != e.uid);
        let gid = gid.filter(|g| Some(*g) != e.gid);
        let recursive = self.recursive && e.is_dir();
        if !mode_changed && uid.is_none() && gid.is_none() && !recursive {
            window.close_dialog(cx);
            return;
        }
        self.busy = true;
        self.error = None;
        let fs = self.fs.clone();
        let job = self.rt.spawn(async move {
            let apply_one = |path: String, m: u32| {
                let fs = fs.clone();
                async move {
                    if mode_changed || recursive {
                        fs.set_permissions(&path, m).await?;
                    }
                    if uid.is_some() || gid.is_some() {
                        fs.set_owner(&path, uid, gid).await?;
                    }
                    Ok::<(), FsError>(())
                }
            };
            apply_one(e.path.clone(), mode).await?;
            if recursive {
                let mut stack = vec![e.path.clone()];
                while let Some(dir) = stack.pop() {
                    for c in fs.list_dir(&dir).await? {
                        if c.kind == EntryKind::Symlink {
                            continue;
                        }
                        let m = recursive_mode(mode, c.permissions, c.is_dir());
                        if c.is_dir() {
                            stack.push(c.path.clone());
                        }
                        apply_one(c.path, m).await?;
                    }
                }
            }
            Ok::<(), FsError>(())
        });
        let side = self.side;
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |p, window, cx| {
                p.busy = false;
                let _ = p.view.update(cx, |v, cx| v.refresh(side, cx));
                match res {
                    Ok(Ok(())) => {
                        window.close_dialog(cx);
                        let _ = p.view.update(cx, |v, cx| v.refocus(window, cx));
                    }
                    Ok(Err(e)) => p.error = Some(e.to_string()),
                    Err(e) => p.error = Some(t!("error-internal", error = e.to_string()).into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

/// "Owner: Read" for the permission checkbox `id` (ur, gw, ox, suid …).
fn permission_bit_name(id: &str, label: &SharedString) -> String {
    let who = match id.as_bytes().first() {
        Some(b'u') if id.len() == 2 => t!("props-owner"),
        Some(b'g') if id.len() == 2 => t!("props-group"),
        Some(b'o') if id.len() == 2 => t!("props-others"),
        _ => t!("props-special"),
    };
    format!("{who}: {label}")
}

impl Render for Props {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let e = &self.entry;
        let row = |label: SharedString, value: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .min_h(px(26.0))
                .child(
                    div()
                        .w(px(110.0))
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(div().flex_1().min_w(px(0.0)).child(value))
        };
        let text = |s: String| div().child(s).into_any_element();
        let kind: String = match e.kind {
            EntryKind::Dir => t!("props-kind-folder").into(),
            EntryKind::File => t!("props-kind-file").into(),
            EntryKind::Symlink => t!(
                "props-kind-symlink",
                target = self.link_target.as_deref().unwrap_or("…")
            )
            .into(),
            EntryKind::Other => t!("props-kind-special").into(),
        };
        let name_with_id = |id: Option<u32>, name: &Option<String>| -> SharedString {
            match (id, name) {
                (_, Some(n)) => n.clone().into(),
                (Some(_), None) => t!("props-unknown-name"),
                (None, _) => t!("props-not-reported"),
            }
        };

        // Permission grid: rows owner/group/others, columns r/w/x.
        let view = cx.entity().downgrade();
        let bit = |id: &'static str, label: SharedString, mask: u32, mode: u32| {
            let view = view.clone();
            let spoken = permission_bit_name(id, &label);
            crate::a11y::checkbox(id, label, false, cx)
                .accessibility_label(spoken)
                .checked(mode & mask != 0)
                .on_click(move |on, window, cx| {
                    let _ = view.update(cx, |p, cx| {
                        let m = if *on { p.mode | mask } else { p.mode & !mask };
                        p.set_mode(m, window, cx);
                    });
                })
        };
        let m = self.mode;
        let who = |name: SharedString, shift: u32, ids: [&'static str; 3]| {
            div()
                .flex()
                .gap(px(14.0))
                .items_center()
                .child(div().w(px(90.0)).child(name))
                .child(bit(ids[0], t!("props-read"), 0o4 << shift, m))
                .child(bit(ids[1], t!("props-write"), 0o2 << shift, m))
                .child(bit(ids[2], t!("props-execute"), 0o1 << shift, m))
        };
        let perms = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(who(t!("props-owner"), 6, ["ur", "uw", "ux"]))
            .child(who(t!("props-group"), 3, ["gr", "gw", "gx"]))
            .child(who(t!("props-others"), 0, ["or", "ow", "ox"]))
            .child(
                div()
                    .flex()
                    .gap(px(14.0))
                    .items_center()
                    .child(div().w(px(90.0)).child(t!("props-special")))
                    .child(bit("suid", "Set UID".into(), 0o4000, m))
                    .child(bit("sgid", "Set GID".into(), 0o2000, m))
                    .child(bit("sticky", "Sticky".into(), 0o1000, m)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .items_center()
                    .child(div().w(px(90.0)).child(t!("props-octal")))
                    .child(
                        div().w(px(80.0)).child(
                            Input::new(&self.octal)
                                .small()
                                .aria_label(t!("props-octal")),
                        ),
                    ),
            );

        let size: AnyElement = if e.is_dir() {
            let label = self.usage.clone().unwrap_or_default();
            div()
                .flex()
                .gap(px(8.0))
                .items_center()
                .child(
                    Button::new("calc-size")
                        .xsmall()
                        .label(t!("props-calculate"))
                        .on_click(cx.listener(|p, _, _, cx| p.calc_usage(cx))),
                )
                .child(label)
                .into_any_element()
        } else {
            text(t!("props-size", size = human_size(e.size), bytes = e.size).into())
        };

        let checksum: Option<AnyElement> = (e.kind == EntryKind::File).then(|| {
            let value = self.checksum.clone();
            div()
                .flex()
                .gap(px(8.0))
                .items_center()
                .child(
                    Button::new("calc-sum")
                        .xsmall()
                        .label("SHA-256")
                        .on_click(cx.listener(|p, _, _, cx| p.calc_checksum(cx))),
                )
                .when_some(value, |d, v| {
                    let copy = v.clone();
                    d.child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .text_size(px(11.0))
                            .font_family(crate::terminal::monospace_family())
                            .child(v),
                    )
                    .child(
                        Button::new("copy-sum")
                            .xsmall()
                            .ghost()
                            .label(t!("term-menu-copy"))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            }),
                    )
                })
                .into_any_element()
        });

        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .text_size(px(13.0))
            .child(row(t!("files-col-name"), text(e.name.clone())))
            .child(row(
                t!("props-location"),
                text(hostwick_sftp::parent(&e.path)),
            ))
            .child(row(t!("props-type"), text(kind)))
            .child(row(t!("files-col-size"), size))
            .when_some(checksum, |d, c| d.child(row(t!("props-checksum"), c)))
            .child(row(
                t!("files-col-modified"),
                text(
                    e.modified
                        .map(local_time)
                        .unwrap_or_else(|| t!("props-unknown").into()),
                ),
            ))
            .child(row(
                t!("props-owner"),
                div()
                    .flex()
                    .gap(px(8.0))
                    .items_center()
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Input::new(&self.uid).small().aria_label(t!("props-owner"))),
                    )
                    .child(name_with_id(e.uid, &self.names.0))
                    .into_any_element(),
            ))
            .child(row(
                t!("props-group"),
                div()
                    .flex()
                    .gap(px(8.0))
                    .items_center()
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Input::new(&self.gid).small().aria_label(t!("props-group"))),
                    )
                    .child(name_with_id(e.gid, &self.names.1))
                    .into_any_element(),
            ))
            .child(row(t!("props-permissions"), perms.into_any_element()))
            .when(e.is_dir(), |d| {
                let recursive = self.recursive;
                d.child(row(
                    SharedString::default(),
                    crate::a11y::checkbox("recursive", t!("props-recursive"), false, cx)
                        .checked(recursive)
                        .on_click(cx.listener(|p, on: &bool, _, cx| {
                            p.recursive = *on;
                            cx.notify();
                        }))
                        .into_any_element(),
                ))
            })
            .when_some(self.error.clone(), |d, err| {
                d.child(div().text_color(theme.danger).child(err))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_octal, recursive_mode};

    #[test]
    fn octal_and_recursive_modes() {
        assert_eq!(parse_octal("755"), Some(0o755));
        assert_eq!(parse_octal("0644"), Some(0o644));
        assert_eq!(parse_octal("4755"), Some(0o4755));
        assert_eq!(parse_octal("8"), None);
        assert_eq!(parse_octal("17777"), None);
        assert_eq!(parse_octal(""), None);
        assert_eq!(recursive_mode(0o755, Some(0o600), true), 0o755);
        assert_eq!(recursive_mode(0o755, Some(0o600), false), 0o644);
        assert_eq!(recursive_mode(0o755, Some(0o700), false), 0o744);
        assert_eq!(recursive_mode(0o640, Some(0o755), false), 0o751);
    }
}
