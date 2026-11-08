//! In-terminal file transfers (ZMODEM `rz`/`sz` and trzsz) on the desktop
//! side: what the parser hands over when it spots a start sequence, the
//! overlay that asks where to save or which files to send, and the task
//! that runs the protocol from `hostwick-term`.
//!
//! While a transfer runs the parser forwards every byte of output to the
//! transfer instead of the screen (and hands the receiving end back
//! afterwards, so nothing is lost or shown out of order); keyboard input is
//! not sent to the remote side, and Esc or Ctrl+C cancels.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::zoom::px;
use bytes::Bytes;
use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_term::ShellCommand;
use hostwick_term::trzsz::{self, Mode};
use hostwick_term::xfer::{Detected, Link, Outcome, Progress, SendFile, XferError};
use hostwick_term::zmodem::{self, Start};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{Note, TerminalView};

/// A transfer the parser found; sent to the view in [`Note::Xfer`].
pub struct XferStart {
    pub detected: Detected,
    /// What the remote program already sent after its start sequence.
    pub initial: Vec<u8>,
    /// Output from the remote program, forwarded by the parser.
    pub rx: mpsc::Receiver<Bytes>,
    /// Where to hand `rx` back when the transfer is over.
    pub done: mpsc::UnboundedSender<Done>,
    /// The session's input (bounded: a slow link slows the file reader).
    pub input: mpsc::Sender<ShellCommand>,
}

/// The parser takes output back from the transfer.
pub struct Done {
    pub rx: mpsc::Receiver<Bytes>,
    /// Output received but not used by the transfer.
    pub spare: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// The remote program sends; we save.
    Download,
    /// The remote program receives; we send.
    Upload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Zmodem,
    Trzsz,
}

impl Proto {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zmodem => "ZMODEM",
            Self::Trzsz => "trzsz",
        }
    }
}

/// How a transfer ended, for the view.
pub struct Ended {
    /// The session it ran in; a result from an older session is dropped.
    pub generation: u64,
    pub proto: Proto,
    pub dir: Dir,
    pub result: Result<Outcome, XferError>,
}

enum Phase {
    /// Download offered: waiting for "Receive".
    AskSave,
    /// Upload requested: waiting for the file picker.
    Picking,
    Running {
        progress: Option<Progress>,
        cancel: CancellationToken,
        since: Instant,
    },
}

/// The transfer in progress (at most one per terminal).
pub struct XferUi {
    /// The session this belongs to.
    generation: u64,
    proto: Proto,
    dir: Dir,
    start: Option<XferStart>,
    dest: PathBuf,
    phase: Phase,
}

impl XferUi {
    /// The session ended or is being replaced: an offer nobody answered is
    /// void (its channels close, which lets the old parser finish) and a
    /// running transfer is cancelled. True when something was running.
    pub fn abandon(self) -> bool {
        match self.phase {
            Phase::Running { cancel, .. } => {
                cancel.cancel();
                true
            }
            _ => false,
        }
    }
}

/// A finished download, shown for a while with a button to open the folder.
pub struct Finished {
    pub text: String,
    pub folder: Option<PathBuf>,
}

/// Where downloads go unless the person picks another folder.
pub fn default_download_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("HOSTWICK_DOWNLOAD_DIR") {
        return PathBuf::from(d);
    }
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Test and automation seam: files to upload without asking
/// (`HOSTWICK_PICK_FILES=/a:/b`, path-list separator of the OS).
fn preset_files() -> Option<Vec<PathBuf>> {
    let v = std::env::var_os("HOSTWICK_PICK_FILES")?;
    let list: Vec<PathBuf> = std::env::split_paths(&v).collect();
    (!list.is_empty()).then_some(list)
}

fn human_bytes(b: u64) -> String {
    super::metrics::format_bytes(b)
}

impl TerminalView {
    /// A transfer started (from the parser).
    pub(super) fn begin_transfer(&mut self, start: XferStart, cx: &mut Context<Self>) {
        if self.x.xfer.is_some() {
            // One at a time: refuse the second.
            self.spawn_abort(start);
            return;
        }
        let (proto, dir) = match &start.detected {
            Detected::Zmodem(Start::Download) => (Proto::Zmodem, Dir::Download),
            Detected::Zmodem(Start::Upload) => (Proto::Zmodem, Dir::Upload),
            Detected::Trzsz(m) => (
                Proto::Trzsz,
                if m.mode == Mode::Download {
                    Dir::Download
                } else {
                    Dir::Upload
                },
            ),
        };
        if let Detected::Trzsz(m) = &start.detected
            && m.mode == Mode::UploadDirs
        {
            // Folder upload (`trz -d`) is not supported: say so, and tell the
            // server no, so it returns to the prompt.
            self.spawn_abort(start);
            self.show_notice(t!("tx-xfer-no-dirs").into(), cx);
            return;
        }
        let dest = self
            .x
            .download_dir
            .clone()
            .unwrap_or_else(default_download_dir);
        let phase = match dir {
            Dir::Download => Phase::AskSave,
            Dir::Upload => Phase::Picking,
        };
        self.x.xfer = Some(XferUi {
            generation: self.generation,
            proto,
            dir,
            start: Some(start),
            dest,
            phase,
        });
        let announce = match dir {
            Dir::Download => t!("tx-xfer-offer-download", protocol = proto.label()),
            Dir::Upload => t!("tx-xfer-offer-upload", protocol = proto.label()),
        };
        self.set_live(announce.to_string(), cx);
        if dir == Dir::Upload {
            self.pick_upload(cx);
        }
        cx.notify();
    }

    /// Ask which files to send (or use the preset ones).
    fn pick_upload(&mut self, cx: &mut Context<Self>) {
        if let Some(files) = preset_files() {
            self.run_upload(files, cx);
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(t!("tx-xfer-pick-prompt")),
        });
        cx.spawn(async move |this, cx| {
            let picked = rx.await;
            let _ = this.update(cx, |v, cx| match picked {
                Ok(Ok(Some(files))) if !files.is_empty() => v.run_upload(files, cx),
                _ => v.cancel_transfer(cx),
            });
        })
        .detach();
    }

    fn run_upload(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let files: Vec<SendFile> = paths
            .iter()
            .filter_map(|p| SendFile::from_path(p))
            .collect();
        if files.is_empty() {
            self.cancel_transfer(cx);
            return;
        }
        self.run_transfer(Some(files), cx);
    }

    /// "Receive" was pressed.
    pub(super) fn confirm_download(&mut self, cx: &mut Context<Self>) {
        let ok = matches!(
            self.x.xfer,
            Some(XferUi {
                phase: Phase::AskSave,
                ..
            })
        );
        if ok {
            self.run_transfer(None, cx);
        }
    }

    /// Pick another folder for the download.
    pub(super) fn change_download_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("tx-xfer-folder-prompt")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut dirs))) = rx.await
                && let Some(dir) = dirs.pop()
            {
                let _ = this.update(cx, |v, cx| {
                    v.x.download_dir = Some(dir.clone());
                    if let Some(x) = v.x.xfer.as_mut() {
                        x.dest = dir;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Start the protocol: `files` is set for uploads.
    fn run_transfer(&mut self, files: Option<Vec<SendFile>>, cx: &mut Context<Self>) {
        let Some(x) = self.x.xfer.as_mut() else {
            return;
        };
        let Some(start) = x.start.take() else {
            return;
        };
        let cancel = CancellationToken::new();
        x.phase = Phase::Running {
            progress: None,
            cancel: cancel.clone(),
            since: Instant::now(),
        };
        let (proto, dir, dest, generation) = (x.proto, x.dir, x.dest.clone(), x.generation);
        let notes = self.notes.clone();
        self.rt.spawn(async move {
            let XferStart {
                detected,
                initial,
                rx,
                done,
                input,
            } = start;
            let mut link = Link::new(rx, input);
            let mut last = Instant::now() - Duration::from_secs(1);
            let n2 = notes.clone();
            let mut report = |p: &Progress| {
                if last.elapsed() >= Duration::from_millis(100) || p.done >= p.total {
                    last = Instant::now();
                    let _ = n2.unbounded_send(Note::XferProgress(generation, p.clone()));
                }
            };
            let result = match (&detected, files) {
                (Detected::Zmodem(_), None) => {
                    zmodem::receive(&mut link, &initial, &dest, &cancel, &mut report).await
                }
                (Detected::Zmodem(_), Some(files)) => {
                    zmodem::send(&mut link, &initial, &files, &cancel, &mut report).await
                }
                (Detected::Trzsz(m), None) => {
                    trzsz::download(
                        &mut link,
                        &initial,
                        &m.version,
                        &dest,
                        "Received",
                        &cancel,
                        &mut report,
                    )
                    .await
                }
                (Detected::Trzsz(m), Some(files)) => {
                    trzsz::upload(
                        &mut link,
                        &initial,
                        &m.version,
                        &files,
                        "Sent",
                        &cancel,
                        &mut report,
                    )
                    .await
                }
            };
            // A ZMODEM sender may still be streaming file data after a
            // cancel or an error: let it stop without showing the noise.
            let spare = match &result {
                Err(XferError::Closed) | Ok(_) => link.leftover(),
                Err(_) if matches!(detected, Detected::Zmodem(_)) => {
                    super::settle::settle(&mut link).await
                }
                Err(_) => link.leftover(),
            };
            let _ = done.send(Done { rx: link.rx, spare });
            let _ = notes.unbounded_send(Note::XferEnded(Ended {
                generation,
                proto,
                dir,
                result,
            }));
        });
        cx.notify();
    }

    /// Cancel: ask the remote program to stop, before or during the run.
    pub fn cancel_transfer(&mut self, cx: &mut Context<Self>) {
        let Some(x) = self.x.xfer.as_mut() else {
            return;
        };
        match &x.phase {
            Phase::Running { cancel, .. } => cancel.cancel(),
            _ => {
                if let Some(start) = x.start.take() {
                    self.spawn_abort(start);
                }
                self.x.xfer = None;
                self.set_live(t!("tx-xfer-cancelled").to_string(), cx);
            }
        }
        cx.notify();
    }

    /// Tell the remote program no and give the output back to the screen.
    fn spawn_abort(&self, start: XferStart) {
        self.rt.spawn(async move {
            let XferStart {
                detected,
                rx,
                done,
                input,
                ..
            } = start;
            let mut link = Link::new(rx, input);
            match &detected {
                Detected::Zmodem(_) => {
                    let _ = link.send(zmodem::cancel_sequence()).await;
                }
                Detected::Trzsz(m) => {
                    let _ = trzsz::decline(&mut link, &m.version).await;
                }
            }
            // Let the other side answer and print its prompt.
            tokio::time::sleep(Duration::from_millis(250)).await;
            let spare = link.leftover();
            let _ = done.send(Done { rx: link.rx, spare });
        });
    }

    /// The session ended or is being replaced (reconnect): drop the offer or
    /// cancel the transfer, so keys are not swallowed by a dialog about a
    /// connection that is gone.
    pub(super) fn abandon_transfer(&mut self, cx: &mut Context<Self>) {
        if let Some(x) = self.x.xfer.take() {
            x.abandon();
            cx.notify();
        }
    }

    pub(super) fn on_xfer_progress(
        &mut self,
        generation: u64,
        p: Progress,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        if let Some(XferUi {
            phase: Phase::Running { progress, .. },
            ..
        }) = self.x.xfer.as_mut()
        {
            *progress = Some(p);
            cx.notify();
        }
    }

    pub(super) fn on_xfer_ended(&mut self, e: Ended, cx: &mut Context<Self>) {
        if e.generation != self.generation {
            return;
        }
        self.x.xfer = None;
        let name = e.proto.label();
        let text = match &e.result {
            Ok(o) => {
                let n = o.files.len();
                let size = human_bytes(o.bytes);
                match e.dir {
                    Dir::Download => {
                        t!("tx-xfer-received", protocol = name, count = n, size = size)
                    }
                    Dir::Upload => t!("tx-xfer-sent", protocol = name, count = n, size = size),
                }
                .to_string()
            }
            Err(XferError::Cancelled) => t!("tx-xfer-cancelled").to_string(),
            Err(XferError::RemoteCancelled) => t!("tx-xfer-remote-cancelled").to_string(),
            Err(XferError::Closed) => t!("tx-xfer-closed").to_string(),
            Err(XferError::Timeout) => t!("tx-xfer-timeout").to_string(),
            Err(other) => {
                t!("tx-xfer-failed", protocol = name, error = other.to_string()).to_string()
            }
        };
        let folder = match (&e.result, e.dir) {
            (Ok(o), Dir::Download) if !o.files.is_empty() => o
                .files
                .first()
                .and_then(|f| f.parent())
                .map(Path::to_path_buf),
            _ => None,
        };
        self.set_live(text.clone(), cx);
        self.x.finished = Some(Finished { text, folder });
        self.x.finished_gen += 1;
        let g = self.x.finished_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(20))
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.x.finished_gen == g {
                    v.x.finished = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// The overlay: the question, the progress or the result.
    pub(super) fn render_transfer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let card = |id: &'static str| {
            div()
                .id(id)
                .occlude()
                .role(Role::Status)
                .w(px(520.0))
                .max_w_full()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .p(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_lg()
                .text_size(px(13.0))
        };
        let wrap = |inner: AnyElement| {
            div()
                .absolute()
                .top(px(14.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .px(px(12.0))
                .child(inner)
                .into_any_element()
        };
        if let Some(x) = self.x.xfer.as_ref() {
            let proto = x.proto.label();
            let title = |text: String| {
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_normal()
                    .child(text)
            };
            let el = match &x.phase {
                Phase::AskSave => {
                    let label = t!("tx-xfer-offer-download", protocol = proto).to_string();
                    card("xfer-ask")
                        .aria_label(label.clone())
                        .child(title(label))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child(t!("tx-xfer-save-to")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(x.dest.display().to_string()),
                                )
                                .child(
                                    Button::new("xfer-change")
                                        .small()
                                        .label(t!("tx-xfer-change"))
                                        .on_click(
                                            cx.listener(|v, _, _, cx| v.change_download_dir(cx)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(
                                    Button::new("xfer-cancel")
                                        .small()
                                        .label(t!("button-cancel"))
                                        .on_click(cx.listener(|v, _, _, cx| v.cancel_transfer(cx))),
                                )
                                .child(
                                    Button::new("xfer-receive")
                                        .small()
                                        .primary()
                                        .label(t!("tx-xfer-receive"))
                                        .on_click(
                                            cx.listener(|v, _, _, cx| v.confirm_download(cx)),
                                        ),
                                ),
                        )
                }
                Phase::Picking => {
                    let label = t!("tx-xfer-offer-upload", protocol = proto).to_string();
                    card("xfer-pick")
                        .aria_label(label.clone())
                        .child(title(label))
                        .child(
                            div().flex().justify_end().child(
                                Button::new("xfer-cancel")
                                    .small()
                                    .label(t!("button-cancel"))
                                    .on_click(cx.listener(|v, _, _, cx| v.cancel_transfer(cx))),
                            ),
                        )
                }
                Phase::Running {
                    progress, since, ..
                } => {
                    let verb = match x.dir {
                        Dir::Download => t!("tx-xfer-receiving", protocol = proto),
                        Dir::Upload => t!("tx-xfer-sending", protocol = proto),
                    }
                    .to_string();
                    let (name, frac, line) = match progress {
                        Some(p) => {
                            let frac = if p.total > 0 {
                                (p.done as f32 / p.total as f32).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let secs = since.elapsed().as_secs_f64().max(0.001);
                            let rate = (p.done as f64 / secs) as u64;
                            let files = if p.count > 1 {
                                format!("{}/{} · ", p.index, p.count)
                            } else {
                                String::new()
                            };
                            (
                                p.name.clone(),
                                frac,
                                format!(
                                    "{files}{} / {} · {}/s",
                                    human_bytes(p.done),
                                    human_bytes(p.total),
                                    human_bytes(rate)
                                ),
                            )
                        }
                        None => (String::new(), 0.0, t!("tx-xfer-waiting").to_string()),
                    };
                    let spoken = format!("{verb} {name} {line}");
                    card("xfer-run")
                        .aria_label(spoken.clone())
                        .aria_value(spoken)
                        .child(title(format!("{verb}  {name}")))
                        .child(
                            div()
                                .h(px(6.0))
                                .w_full()
                                .rounded(px(3.0))
                                .bg(theme.border)
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(frac))
                                        .rounded(px(3.0))
                                        .bg(theme.primary),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(theme.muted_foreground)
                                        .child(line),
                                )
                                .child(
                                    Button::new("xfer-cancel")
                                        .small()
                                        .label(t!("button-cancel"))
                                        .tooltip(t!("tx-xfer-cancel-hint"))
                                        .on_click(cx.listener(|v, _, _, cx| v.cancel_transfer(cx))),
                                ),
                        )
                }
            };
            return Some(wrap(el.into_any_element()));
        }
        let f = self.x.finished.as_ref()?;
        let folder = f.folder.clone();
        let el = card("xfer-done").aria_label(f.text.clone()).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .child(f.text.clone()),
                )
                .when_some(folder, |d, dir| {
                    d.child(
                        Button::new("xfer-open-folder")
                            .small()
                            .label(t!("tx-xfer-open-folder"))
                            .on_click(move |_, _, cx| cx.open_with_system(&dir)),
                    )
                })
                .child(
                    Button::new("xfer-dismiss")
                        .small()
                        .ghost()
                        .label("✕")
                        .accessibility_label(t!("dismiss"))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.x.finished = None;
                            cx.notify();
                        })),
                ),
        );
        Some(wrap(el.into_any_element()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use bytes::Bytes;
    use hostwick_term::ShellCommand;
    use hostwick_term::xfer::Detected;
    use hostwick_term::zmodem::Start;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::{Dir, Phase, Proto, XferStart, XferUi};

    fn offer(phase: Phase) -> XferUi {
        let (_tx, rx) = mpsc::channel::<Bytes>(1);
        let (done, _done_rx) = mpsc::unbounded_channel();
        let (input, _input_rx) = mpsc::channel::<ShellCommand>(1);
        XferUi {
            generation: 3,
            proto: Proto::Zmodem,
            dir: Dir::Download,
            start: Some(XferStart {
                detected: Detected::Zmodem(Start::Download),
                initial: Vec::new(),
                rx,
                done,
                input,
            }),
            dest: PathBuf::from("."),
            phase,
        }
    }

    #[test]
    fn an_unanswered_offer_is_void_when_the_session_ends() {
        for phase in [Phase::AskSave, Phase::Picking] {
            let x = offer(phase);
            assert!(!x.abandon(), "nothing was running");
        }
    }

    #[test]
    fn a_running_transfer_is_cancelled_when_the_session_ends() {
        let cancel = CancellationToken::new();
        let x = offer(Phase::Running {
            progress: None,
            cancel: cancel.clone(),
            since: Instant::now(),
        });
        assert!(!cancel.is_cancelled());
        assert!(x.abandon());
        assert!(cancel.is_cancelled());
    }
}
