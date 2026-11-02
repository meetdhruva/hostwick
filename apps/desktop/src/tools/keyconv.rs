//! "Inspect or convert key…": what a key file is, change its passphrase,
//! convert between OpenSSH and PuTTY .ppk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::SecretString;
use hostwick_keys::{KeyError, KeyFormat, KeyInfo, PpkVersion};

use super::{OutFormat, mono, ssh_dir_path, wrap_chars};
use crate::backend::Backend;

pub struct KeyTool {
    backend: Arc<Backend>,
    path: Entity<InputState>,
    old: Entity<InputState>,
    new: Entity<InputState>,
    confirm: Entity<InputState>,
    out: Entity<InputState>,
    format: OutFormat,
    info: Option<KeyInfo>,
    busy: bool,
    message: Option<(bool, String)>,
    _subs: Vec<Subscription>,
}

/// Default output name when converting `path` to `format`.
pub fn converted_path(path: &Path, format: OutFormat) -> PathBuf {
    let is_ppk = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ppk"));
    match format {
        OutFormat::OpenSsh if is_ppk => path.with_extension(""),
        OutFormat::OpenSsh => {
            let mut s = path.as_os_str().to_owned();
            s.push("-openssh");
            PathBuf::from(s)
        }
        _ if is_ppk => {
            let stem = path.with_extension("");
            let mut s = stem.into_os_string();
            s.push(if format == OutFormat::Ppk2 {
                "-v2.ppk"
            } else {
                "-v3.ppk"
            });
            PathBuf::from(s)
        }
        _ => {
            let mut s = path.as_os_str().to_owned();
            s.push(".ppk");
            PathBuf::from(s)
        }
    }
}

impl KeyTool {
    fn new(backend: Arc<Backend>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mk = |ph: &str, masked: bool, window: &mut Window, cx: &mut Context<Self>| {
            let ph = ph.to_owned();
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).masked(masked))
        };
        let path = mk("~/.ssh/id_ed25519 or C:\\keys\\work.ppk", false, window, cx);
        let old = mk("Current passphrase (if it has one)", true, window, cx);
        let new = mk("New passphrase (empty = none)", true, window, cx);
        let confirm = mk("Repeat new passphrase", true, window, cx);
        let out = mk("Output file", false, window, cx);
        let mut subs = vec![cx.subscribe_in(
            &path,
            window,
            |f: &mut Self, _, ev: &InputEvent, w, cx| match ev {
                InputEvent::PressEnter { .. } => f.inspect(w, cx),
                InputEvent::Change => {
                    f.info = None;
                    cx.notify();
                }
                _ => {}
            },
        )];
        for i in [&old, &new, &confirm] {
            subs.push(cx.subscribe(i, |f: &mut Self, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    f.message = None;
                    cx.notify();
                }
            }));
        }
        Self {
            backend,
            path,
            old,
            new,
            confirm,
            out,
            format: OutFormat::Ppk3,
            info: None,
            busy: false,
            message: None,
            _subs: subs,
        }
    }

    fn key_path(&self, cx: &App) -> Option<PathBuf> {
        let t = self.path.read(cx).value().trim().to_owned();
        (!t.is_empty()).then(|| ssh_dir_path(&t))
    }

    fn secret(i: &Entity<InputState>, cx: &App) -> Option<SecretString> {
        let v = i.read(cx).value().to_string();
        (!v.is_empty()).then(|| SecretString::from(v))
    }

    fn inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.key_path(cx) else {
            return;
        };
        self.run(window, cx, move || {
            hostwick_keys::inspect(&path)
                .map(|i| (Some(i), String::new()))
                .map_err(|e| e.to_string())
        });
    }

    fn set_format(&mut self, f: OutFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.format = f;
        if let Some(p) = self.key_path(cx) {
            let out = converted_path(&p, f).display().to_string();
            self.out.update(cx, |s, cx| s.set_value(out, window, cx));
        }
        cx.notify();
    }

    fn change_passphrase(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.key_path(cx) else {
            return;
        };
        if self.new.read(cx).value() != self.confirm.read(cx).value() {
            self.message = Some((true, "The new passphrases do not match.".into()));
            cx.notify();
            return;
        }
        let (old, new) = (Self::secret(&self.old, cx), Self::secret(&self.new, cx));
        self.run(window, cx, move || {
            let fmt = hostwick_keys::change_passphrase(&path, old.as_ref(), new.as_ref())
                .map_err(describe)?;
            let info = hostwick_keys::inspect(&path).ok();
            Ok((
                info,
                format!(
                    "Passphrase {} ({fmt}).",
                    if new.is_some() { "changed" } else { "removed" }
                ),
            ))
        });
    }

    fn convert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.key_path(cx) else {
            return;
        };
        let out_text = self.out.read(cx).value().trim().to_owned();
        let out = if out_text.is_empty() {
            converted_path(&path, self.format)
        } else {
            ssh_dir_path(&out_text)
        };
        if self.new.read(cx).value() != self.confirm.read(cx).value() {
            self.message = Some((true, "The new passphrases do not match.".into()));
            cx.notify();
            return;
        }
        let old = Self::secret(&self.old, cx);
        // The converted key keeps the current passphrase unless a new one is given.
        let pass = Self::secret(&self.new, cx).or_else(|| old.clone());
        let format = self.format;
        self.run(window, cx, move || {
            let key = hostwick_keys::load_any(&path, old.as_ref()).map_err(describe)?;
            match format {
                OutFormat::OpenSsh => {
                    hostwick_keys::write_openssh(&key, &out, pass.as_ref(), false)
                }
                OutFormat::Ppk3 => {
                    hostwick_keys::write_ppk(&key, &out, pass.as_ref(), PpkVersion::V3, false)
                }
                OutFormat::Ppk2 => {
                    hostwick_keys::write_ppk(&key, &out, pass.as_ref(), PpkVersion::V2, false)
                }
            }
            .map_err(describe)?;
            Ok((
                None,
                format!("Wrote {} ({}).", out.display(), format.label()),
            ))
        });
    }

    /// Run blocking key work (bcrypt / Argon2) off the UI thread.
    fn run(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<(Option<KeyInfo>, String), String> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = None;
        cx.notify();
        let job = self.backend.rt.spawn_blocking(work);
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update_in(cx, |f, window, cx| {
                f.busy = false;
                match res {
                    Ok(Ok((info, msg))) => {
                        if let Some(i) = info {
                            f.info = Some(i);
                            if f.out.read(cx).value().is_empty()
                                && let Some(p) = f.key_path(cx)
                            {
                                let out = converted_path(&p, f.format).display().to_string();
                                f.out.update(cx, |s, cx| s.set_value(out, window, cx));
                            }
                        }
                        if !msg.is_empty() {
                            f.message = Some((false, msg));
                        }
                    }
                    Ok(Err(e)) => f.message = Some((true, e)),
                    Err(e) => f.message = Some((true, format!("internal error: {e}"))),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn describe(e: KeyError) -> String {
    match e {
        KeyError::PassphraseRequired => {
            "This key has a passphrase: type it under “Current passphrase”.".into()
        }
        KeyError::WrongPassphrase => "Wrong passphrase.".into(),
        KeyError::Exists(p) => format!(
            "{} already exists; choose another output file.",
            p.display()
        ),
        e => e.to_string(),
    }
}

impl Render for KeyTool {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let row = |label: &'static str, el: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(130.0))
                        .flex_none()
                        .text_size(px(13.0))
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(el))
        };
        let path_input = self.path.clone();
        let this = cx.entity().downgrade();
        let browse =
            Button::new("kc-browse")
                .small()
                .label("Browse…")
                .on_click(move |_, window, cx| {
                    let picked = cx.prompt_for_paths(PathPromptOptions {
                        files: true,
                        directories: false,
                        multiple: false,
                        prompt: Some("Key file".into()),
                    });
                    let (input, this) = (path_input.clone(), this.clone());
                    window
                        .spawn(cx, async move |cx| {
                            let Ok(Ok(Some(paths))) = picked.await else {
                                return;
                            };
                            if let Some(p) = paths.into_iter().next() {
                                let text = p.display().to_string();
                                let _ = cx.update(|window, cx| {
                                    input.update(cx, |s, cx| s.set_value(text, window, cx));
                                    let _ = this.update(cx, |f, cx| f.inspect(window, cx));
                                });
                            }
                        })
                        .detach();
                });
        let info = self.info.clone().map(|i| {
            let public = hostwick_keys::public_openssh(&i.public_key);
            let copy = public.clone();
            let kv = |k: &'static str, v: String| {
                div()
                    .flex()
                    .gap(px(8.0))
                    .text_size(px(12.0))
                    .child(
                        div()
                            .w(px(90.0))
                            .text_color(theme.muted_foreground)
                            .child(k),
                    )
                    .child(div().flex_1().min_w_0().child(v))
            };
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .p(px(8.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme.border)
                .child(kv("Format", i.format.to_string()))
                .child(kv(
                    "Type",
                    format!("{} {} bits ({})", i.type_label, i.bits, i.algorithm),
                ))
                .child(kv(
                    "Protected",
                    match (i.format, i.encrypted) {
                        (KeyFormat::Public, _) => "public key only".into(),
                        (_, true) => "yes, with a passphrase".into(),
                        (_, false) => "no passphrase".into(),
                    },
                ))
                .child(kv(
                    "Comment",
                    if i.comment.is_empty() {
                        "—".into()
                    } else {
                        i.comment.clone()
                    },
                ))
                .child(kv("Fingerprint", i.fingerprint.clone()))
                .child(mono(wrap_chars(&public, 72), cx))
                .child(
                    div().child(
                        Button::new("kc-copy")
                            .xsmall()
                            .label("Copy public key")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            }),
                    ),
                )
        });
        let is_private = self
            .info
            .as_ref()
            .is_some_and(|i| i.format != KeyFormat::Public);
        let mut formats = div().flex().gap(px(4.0));
        for (i, fmt) in OutFormat::ALL.iter().copied().enumerate() {
            formats = formats.child(
                Button::new(("kcf", i))
                    .xsmall()
                    .label(fmt.label())
                    .when(self.format == fmt, |b| b.primary())
                    .on_click(cx.listener(move |f, _, w, cx| f.set_format(fmt, w, cx))),
            );
        }
        div()
            .id("keytool")
            .flex()
            .flex_col()
            .gap(px(8.0))
            .max_h(px(580.0))
            .overflow_y_scroll()
            .child(row(
                "Key file",
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.path).small()),
                    )
                    .child(browse)
                    .child(
                        Button::new("kc-inspect")
                            .small()
                            .label("Inspect")
                            .disabled(self.busy)
                            .on_click(cx.listener(|f, _, w, cx| f.inspect(w, cx))),
                    )
                    .into_any_element(),
            ))
            .children(info)
            .when(is_private, |d| {
                d.child(row(
                    "Current",
                    Input::new(&self.old)
                        .small()
                        .mask_toggle()
                        .into_any_element(),
                ))
                .child(row(
                    "New passphrase",
                    Input::new(&self.new)
                        .small()
                        .mask_toggle()
                        .into_any_element(),
                ))
                .child(row(
                    "Repeat",
                    Input::new(&self.confirm)
                        .small()
                        .mask_toggle()
                        .into_any_element(),
                ))
                .child(
                    div().pl(px(138.0)).child(
                        Button::new("kc-pass")
                            .small()
                            .label("Change passphrase")
                            .disabled(self.busy)
                            .on_click(cx.listener(|f, _, w, cx| f.change_passphrase(w, cx))),
                    ),
                )
                .child(row("Convert to", formats.into_any_element()))
                .child(row(
                    "Output file",
                    Input::new(&self.out).small().into_any_element(),
                ))
                .child(
                    div()
                        .pl(px(138.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("kc-convert")
                                .small()
                                .primary()
                                .label("Convert")
                                .disabled(self.busy)
                                .on_click(cx.listener(|f, _, w, cx| f.convert(w, cx))),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.muted_foreground)
                                .child("Keeps the current passphrase unless you type a new one."),
                        ),
                )
            })
            .when(self.busy, |d| {
                d.child(div().text_color(theme.muted_foreground).child("Working…"))
            })
            .when_some(self.message.clone(), |d, (err, m)| {
                d.child(
                    div()
                        .text_size(px(13.0))
                        .whitespace_normal()
                        .text_color(if err { theme.danger } else { theme.success })
                        .child(m),
                )
            })
    }
}

pub fn open(backend: Arc<Backend>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| KeyTool::new(backend, window, cx));
    let first = std::rc::Rc::new(std::cell::Cell::new(true));
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let input = form.read(cx).path.clone();
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        dialog
            .title("Inspect or convert key")
            .w(px(720.0))
            .margin_top(px(40.0))
            .child(form.clone())
            .footer(
                div().flex().child(
                    Button::new("close")
                        .label("Close")
                        .on_click(|_, w, cx| w.close_dialog(cx)),
                ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::converted_path;
    use crate::tools::OutFormat;
    use std::path::{Path, PathBuf};

    #[test]
    fn output_names() {
        let k = Path::new("/k/id_ed25519");
        assert_eq!(
            converted_path(k, OutFormat::Ppk3),
            PathBuf::from("/k/id_ed25519.ppk")
        );
        assert_eq!(
            converted_path(k, OutFormat::OpenSsh),
            PathBuf::from("/k/id_ed25519-openssh")
        );
        let p = Path::new("/k/work.ppk");
        assert_eq!(
            converted_path(p, OutFormat::OpenSsh),
            PathBuf::from("/k/work")
        );
        assert_eq!(
            converted_path(p, OutFormat::Ppk2),
            PathBuf::from("/k/work-v2.ppk")
        );
    }
}
