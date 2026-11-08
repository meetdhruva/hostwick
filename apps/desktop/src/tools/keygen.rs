//! "Generate key…": PuTTYgen / ssh-keygen in a dialog.

use std::path::PathBuf;
use std::sync::Arc;

use crate::notify::{Toast, ToastExt as _};
use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{ProfileStore as _, SecretString};
use hostwick_keys::{KeyError, KeyType, PpkVersion};

use super::{OutFormat, default_key_dir, mono, ssh_dir_path};
use crate::app::HostwickApp;
use crate::backend::{Backend, Prompt};

/// What a successful generation produced.
#[derive(Clone)]
struct Generated {
    path: PathBuf,
    public: String,
    fingerprint: String,
    randomart: String,
}

pub struct KeyGen {
    backend: Arc<Backend>,
    kind: KeyType,
    format: OutFormat,
    comment: Entity<InputState>,
    pass: Entity<InputState>,
    confirm: Entity<InputState>,
    path: Entity<InputState>,
    /// The path is still the automatic one (follows type and format).
    auto_path: bool,
    overwrite: bool,
    busy: bool,
    error: Option<String>,
    result: Option<Generated>,
    _subs: Vec<Subscription>,
}

fn default_comment() -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "hostwick".into());
    format!(
        "{}@{host}",
        hostwick_core::HostProfile::new("", "").user_or_local()
    )
}

/// Where a new key goes by default: `~/.ssh/id_ed25519`, `.ppk` for PuTTY.
pub fn default_path(kind: KeyType, format: OutFormat) -> PathBuf {
    let name = kind.default_file_name();
    let dir = default_key_dir();
    match format {
        OutFormat::OpenSsh => dir.join(name),
        _ => dir.join(format!("{name}.ppk")),
    }
}

impl KeyGen {
    fn new(backend: Arc<Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kind = KeyType::Ed25519;
        let format = OutFormat::OpenSsh;
        let path = default_path(kind, format).display().to_string();
        let comment = default_comment();
        let comment = cx.new(|cx| InputState::new(window, cx).default_value(comment));
        let pass = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t!("keygen-passphrase-placeholder"))
        });
        let confirm = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t!("keygen-repeat"))
        });
        let path = cx.new(|cx| InputState::new(window, cx).default_value(path));
        let mut subs = Vec::new();
        subs.push(cx.subscribe(&path, |f: &mut Self, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                f.auto_path = false;
                f.error = None;
                cx.notify();
            }
        }));
        for i in [&pass, &confirm] {
            subs.push(cx.subscribe(i, |f: &mut Self, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    f.error = None;
                    cx.notify();
                }
            }));
        }
        Self {
            backend,
            kind,
            format,
            comment,
            pass,
            confirm,
            path,
            auto_path: true,
            overwrite: false,
            busy: false,
            error: None,
            result: None,
            _subs: subs,
        }
    }

    fn update_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.auto_path {
            let p = default_path(self.kind, self.format).display().to_string();
            self.path.update(cx, |s, cx| s.set_value(p, window, cx));
            // set_value fires Change, which clears auto_path: keep it.
            self.auto_path = true;
        }
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let pass = self.pass.read(cx).value().to_string();
        if pass != self.confirm.read(cx).value().as_ref() {
            self.error = Some(t!("keygen-mismatch").into());
            cx.notify();
            return;
        }
        let path_text = self.path.read(cx).value().trim().to_owned();
        if path_text.is_empty() {
            self.error = Some(t!("keygen-no-path").into());
            cx.notify();
            return;
        }
        let path = ssh_dir_path(&path_text);
        let (kind, format, force) = (self.kind, self.format, self.overwrite);
        let comment = self.comment.read(cx).value().trim().to_owned();
        self.busy = true;
        self.error = None;
        self.result = None;
        cx.notify();
        let job = self.backend.rt.spawn_blocking(move || {
            if let Some(dir) = path.parent()
                && !dir.as_os_str().is_empty()
            {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                #[cfg(unix)]
                if dir.ends_with(".ssh") {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
                }
            }
            let key = hostwick_keys::generate(kind, &comment).map_err(|e| e.to_string())?;
            let pass = (!pass.is_empty()).then(|| SecretString::from(pass));
            let res = match format {
                OutFormat::OpenSsh => {
                    hostwick_keys::write_openssh(&key, &path, pass.as_ref(), force)
                }
                OutFormat::Ppk3 | OutFormat::Ppk2 => {
                    let v = if format == OutFormat::Ppk3 {
                        PpkVersion::V3
                    } else {
                        PpkVersion::V2
                    };
                    hostwick_keys::write_ppk(&key, &path, pass.as_ref(), v, force).and_then(|()| {
                        let pubp = hostwick_keys::public_path(&path);
                        hostwick_keys::write_public(key.public_key(), &pubp, force)
                    })
                }
            };
            match res {
                Ok(()) => {}
                Err(KeyError::Exists(p)) => {
                    return Err(t!("keygen-exists", path = p.display().to_string()).into());
                }
                Err(e) => return Err(e.to_string()),
            }
            let public = key.public_key();
            Ok(Generated {
                path,
                public: hostwick_keys::public_openssh(public),
                fingerprint: hostwick_keys::fingerprint(public),
                randomart: hostwick_keys::randomart(public),
            })
        });
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |f, window, cx| {
                f.busy = false;
                match res {
                    Ok(Ok(g)) => {
                        tracing::info!(path = %g.path.display(), fp = %g.fingerprint, "generated key");
                        f.result = Some(g);
                        for i in [&f.pass, &f.confirm] {
                            i.update(cx, |s, cx| s.set_value("", window, cx));
                        }
                    }
                    Ok(Err(e)) => f.error = Some(e),
                    Err(e) => f.error = Some(t!("error-internal", error = e.to_string()).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn use_in_host(
        &mut self,
        id: hostwick_core::ProfileId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(g) = &self.result else {
            return;
        };
        match self.backend.store.get(id) {
            Ok(mut p) => {
                let added = crate::hostform::add_key_to_profile(&mut p, g.path.clone());
                if added && let Err(e) = self.backend.store.upsert(&p) {
                    self.error = Some(
                        t!(
                            "notice-update-host-failed",
                            name = &p.name,
                            error = e.to_string()
                        )
                        .into(),
                    );
                } else {
                    self.backend.send(Prompt::ProfilesChanged);
                    window.toast(
                        Toast::success(t!(
                            "keygen-used-in-host",
                            name = &p.name,
                            path = g.path.display().to_string()
                        )),
                        cx,
                    );
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}

impl Render for KeyGen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let row = |label: SharedString, el: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(110.0))
                        .flex_none()
                        .text_size(px(13.0))
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(el))
        };
        let mut types = div().flex().flex_wrap().gap(px(4.0));
        for (i, k) in KeyType::CHOICES.iter().copied().enumerate() {
            types = types.child(
                Button::new(("kt", i))
                    .xsmall()
                    .label(if k == KeyType::Ed25519 {
                        t!("keygen-ed25519-recommended").to_string()
                    } else {
                        k.label()
                    })
                    .toggled(self.kind == k)
                    .when(self.kind == k, |b| b.primary())
                    .on_click(cx.listener(move |f, _, w, cx| {
                        f.kind = k;
                        f.update_path(w, cx);
                        cx.notify();
                    })),
            );
        }
        let mut formats = div().flex().gap(px(4.0));
        for (i, fmt) in OutFormat::ALL.iter().copied().enumerate() {
            formats = formats.child(
                Button::new(("kf", i))
                    .xsmall()
                    .label(fmt.label())
                    .toggled(self.format == fmt)
                    .when(self.format == fmt, |b| b.primary())
                    .on_click(cx.listener(move |f, _, w, cx| {
                        f.format = fmt;
                        f.update_path(w, cx);
                        cx.notify();
                    })),
            );
        }
        let path_input = self.path.clone();
        let browse = Button::new("kg-browse")
            .small()
            .label(t!("button-browse"))
            .on_click(move |_, window, cx| {
                let dir = default_key_dir();
                let _ = std::fs::create_dir_all(&dir);
                let picked = cx.prompt_for_new_path(&dir, None);
                let input = path_input.clone();
                window
                    .spawn(cx, async move |cx| {
                        if let Ok(Ok(Some(p))) = picked.await {
                            let text = p.display().to_string();
                            let _ = cx.update(|window, cx| {
                                input.update(cx, |s, cx| s.set_value(text, window, cx))
                            });
                        }
                    })
                    .detach();
            });
        let result = self.result.clone().map(|g| {
            let public = g.public.clone();
            let profiles = self.backend.store.list().unwrap_or_default();
            let this = cx.entity().downgrade();
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(8.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.success)
                        .child(t!(
                            "keygen-saved",
                            file = g
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            dir = g
                                .path
                                .parent()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default()
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(12.0))
                        .child(mono(g.randomart.clone(), cx).flex_none())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .text_size(px(12.0))
                                .child(t!("key-fingerprint", fingerprint = &g.fingerprint))
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(t!("keygen-public-key")),
                                )
                                .child(mono(super::wrap_chars(&g.public, 64), cx))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.0))
                                        .child(
                                            Button::new("copy-pub")
                                                .small()
                                                .label(t!("keygen-copy-public"))
                                                .on_click(move |_, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(public.clone()),
                                                    )
                                                }),
                                        )
                                        .child(
                                            Button::new("use-in-host")
                                                .small()
                                                .label(t!("keygen-use-in-host"))
                                                .disabled(profiles.is_empty())
                                                .dropdown_menu_with_anchor(
                                                    Anchor::BottomLeft,
                                                    move |menu, _, _| {
                                                        let mut menu = menu;
                                                        for p in profiles.iter().filter(|p| {
                                                            p.protocol
                                                                == hostwick_core::Protocol::Ssh
                                                        }) {
                                                            let (id, this) = (p.id, this.clone());
                                                            menu = menu.item(
                                                                PopupMenuItem::new(p.path())
                                                                    .on_click(
                                                                        move |_, window, cx| {
                                                                            let _ = this.update(
                                                                                cx,
                                                                                |f, cx| {
                                                                                    f.use_in_host(
                                                                                        id, window,
                                                                                        cx,
                                                                                    )
                                                                                },
                                                                            );
                                                                        },
                                                                    ),
                                                            );
                                                        }
                                                        menu
                                                    },
                                                ),
                                        ),
                                ),
                        ),
                )
        });
        div()
            .id("keygen")
            .flex()
            .flex_col()
            .gap(px(8.0))
            .max_h(px(560.0))
            .overflow_y_scroll()
            .child(row(t!("keygen-type"), types.into_any_element()))
            .child(row(
                t!("keygen-comment"),
                Input::new(&self.comment)
                    .small()
                    .aria_label(t!("keygen-comment"))
                    .into_any_element(),
            ))
            .child(row(
                t!("prompt-passphrase-label"),
                Input::new(&self.pass)
                    .small()
                    .aria_label(t!("prompt-passphrase-label"))
                    .mask_toggle()
                    .into_any_element(),
            ))
            .child(row(
                t!("keygen-repeat"),
                Input::new(&self.confirm)
                    .small()
                    .aria_label(t!("keygen-repeat"))
                    .mask_toggle()
                    .into_any_element(),
            ))
            .child(row(t!("keygen-format"), formats.into_any_element()))
            .child(
                div()
                    .pl(px(118.0))
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .child(self.format.hint()),
            )
            .child(row(
                t!("keygen-save-as"),
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.path)
                                .small()
                                .aria_label(t!("keygen-save-as")),
                        ),
                    )
                    .child(browse)
                    .into_any_element(),
            ))
            .child(
                div().pl(px(118.0)).child(
                    crate::a11y::checkbox("kg-overwrite", t!("keygen-overwrite"), false, cx)
                        .checked(self.overwrite)
                        .on_click(cx.listener(|f, v: &bool, _, cx| {
                            f.overwrite = *v;
                            cx.notify();
                        })),
                ),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
            .when(self.busy, |d| {
                d.child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme.muted_foreground)
                        .child(
                            if matches!(self.kind, KeyType::Rsa { bits } if bits >= 4096) {
                                t!("keygen-generating-rsa")
                            } else {
                                t!("keygen-generating")
                            },
                        ),
                )
            })
            .children(result)
    }
}

pub fn open(
    backend: Arc<Backend>,
    _app: WeakEntity<HostwickApp>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| KeyGen::new(backend, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let busy = form.read(cx).busy;
        let f1 = form.clone();
        dialog
            .title(crate::a11y::text(t!("keygen-title")))
            .w(px(720.0))
            .margin_top(px(40.0))
            .child(form.clone())
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("close")
                            .label(t!("button-close"))
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("generate")
                            .primary()
                            .label(t!("keygen-generate"))
                            .disabled(busy)
                            .on_click(move |_, w, cx| f1.update(cx, |f, cx| f.generate(w, cx))),
                    ),
            )
    });
}
