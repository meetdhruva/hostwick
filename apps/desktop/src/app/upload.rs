//! Files dropped on a terminal: the question that comes before anything is
//! uploaded.
//!
//! The folder to upload into usually comes from the shell (OSC 7), and
//! anything that prints to the terminal can send that. So the person always
//! sees where the files are going, on which host, how much it is, and what
//! happens to files that already exist (by default nothing: they are
//! skipped).

use std::path::PathBuf;
use std::sync::Arc;

use crate::zoom::px;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::HostProfile;
use hostwick_sftp::{FsError, RemoteFs};
use hostwick_transfer::ExistingPolicy;

use super::HostwickApp;
use crate::terminal::{TermExt, TerminalEvent, TerminalView, links};

/// What happens to a file that is already on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Existing {
    /// Leave the old one alone, upload nothing for it (the default).
    #[default]
    Skip,
    Overwrite,
    /// Upload under a new name (`report (1).txt`).
    Rename,
}

impl Existing {
    pub fn policy(self) -> ExistingPolicy {
        match self {
            Self::Overwrite => ExistingPolicy::Overwrite,
            // Renamed items go to names that are free; skip is the safe
            // answer to anything that still turns up.
            Self::Skip | Self::Rename => ExistingPolicy::Skip,
        }
    }
}

/// How much is being dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measure {
    pub files: usize,
    pub bytes: u64,
    /// The walk stopped at its limit: the real numbers are larger.
    pub truncated: bool,
}

/// Count the files under `paths` (folders recursively, links not followed)
/// and add up their sizes, looking at no more than `max_entries` entries.
pub fn measure(paths: &[PathBuf], max_entries: usize) -> Measure {
    let mut m = Measure::default();
    let mut seen = 0usize;
    let mut stack: Vec<PathBuf> = paths.to_vec();
    while let Some(p) = stack.pop() {
        seen += 1;
        if seen > max_entries {
            m.truncated = true;
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.filter_map(Result::ok).map(|e| e.path()));
            }
        } else {
            m.files += 1;
            m.bytes += meta.len();
        }
    }
    m
}

/// `name`, or `stem (1).ext`, `(2)`… — the first one that does not exist in
/// `dir` on `fs`. (`lstat`, so a dangling link counts as taken.)
pub async fn free_remote_name(
    fs: &Arc<dyn RemoteFs>,
    dir: &str,
    name: &str,
) -> Result<String, FsError> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut candidate = name.to_owned();
    for n in 1..1000 {
        let path = hostwick_sftp::join_checked(dir, &candidate)?;
        if fs.lstat(&path).await?.is_none() {
            return Ok(candidate);
        }
        candidate = format!("{stem} ({n}){ext}");
    }
    Err(FsError::Remote {
        path: dir.to_owned(),
        message: format!("no free name for {name}"),
    })
}

/// The dialog's state.
struct UploadDialog {
    folder: Entity<InputState>,
    existing: Existing,
    /// `user@host`.
    target: SharedString,
    /// What to tell about the folder, if anything.
    note: Option<SharedString>,
    /// Offer "Enable now" (the shell has not reported a folder).
    offer_integration: bool,
    source: Entity<TerminalView>,
}

impl Render for UploadDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let choice = |id: &'static str, label: SharedString, value: Existing, cur: Existing| {
            Button::new(id)
                .small()
                .label(label)
                .selected(value == cur)
                .toggled(value == cur)
                .when(value == cur, |b| b.primary())
                .on_click(cx.listener(move |d: &mut Self, _, _, cx| {
                    d.existing = value;
                    cx.notify();
                }))
        };
        let cur = self.existing;
        let source = self.source.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(px(13.0))
            .child(crate::a11y::text(t!(
                "tx-up-folder",
                target = self.target.clone()
            )))
            .child(Input::new(&self.folder))
            .when_some(self.note.clone(), |d, note| {
                d.child(
                    div()
                        .text_color(theme.warning)
                        .whitespace_normal()
                        .child(crate::a11y::text(note)),
                )
            })
            .when(self.offer_integration, |d| {
                d.child(
                    div().flex().child(
                        Button::new("enable-integration")
                            .label(t!("tx-enable-now"))
                            .on_click(move |_, _, cx| {
                                source.update(cx, |_, cx| {
                                    cx.emit(TerminalEvent::Ext(TermExt::EnableIntegration))
                                });
                            }),
                    ),
                )
            })
            .child(crate::a11y::text(t!("tx-up-existing")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(choice("up-skip", t!("tx-up-skip"), Existing::Skip, cur))
                    .child(choice(
                        "up-rename",
                        t!("tx-up-rename"),
                        Existing::Rename,
                        cur,
                    ))
                    .child(choice(
                        "up-overwrite",
                        t!("tx-up-overwrite"),
                        Existing::Overwrite,
                        cur,
                    )),
            )
            .when(cur == Existing::Overwrite, |d| {
                d.child(
                    div()
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(crate::a11y::text(t!("tx-up-overwrite-warn"))),
                )
            })
    }
}

impl HostwickApp {
    /// Ask before uploading `paths` that were dropped on `source`'s
    /// terminal: where, how much, and what to do with files that exist.
    /// `cwd` is the folder the shell reported, `trusted` whether that report
    /// named this session's own host.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn confirm_upload(
        &mut self,
        source: &Entity<TerminalView>,
        profile: HostProfile,
        paths: Vec<PathBuf>,
        cwd: Option<String>,
        trusted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let measured = measure(&paths, 20_000);
        let size = {
            let s = crate::terminal::format_bytes(measured.bytes);
            if measured.truncated {
                format!("{s}+")
            } else {
                s
            }
        };
        let count = paths.len();
        let user = profile.user_or_local();
        let target: SharedString = if user.is_empty() {
            profile.host.clone().into()
        } else {
            format!("{user}@{}", profile.host).into()
        };
        let title: SharedString = t!(
            "tx-up-title",
            count = count,
            size = size,
            target = target.clone()
        );
        let guess = links::resolve_path("~/x", None, Some(&user))
            .map(|p| p.trim_end_matches("/x").to_owned())
            .unwrap_or_else(|| "/".to_owned());
        let start = cwd.clone().unwrap_or_else(|| guess.clone());
        let folder = cx.new(|cx| InputState::new(window, cx).default_value(start.clone()));
        folder.update(cx, |i, cx| i.focus(window, cx));
        // No folder reported: the real home folder, when the server tells.
        if cwd.is_none() {
            let backend = self.backend.clone();
            let p = profile.clone();
            let job = backend.rt.spawn({
                let backend = backend.clone();
                async move {
                    let s = backend.open_remote(p, false).await.ok()?;
                    s.fs.canonicalize(".").await.ok()
                }
            });
            let (folder, guess) = (folder.clone(), guess.clone());
            cx.spawn_in(window, async move |_, cx| {
                if let Ok(Some(home)) = job.await {
                    let _ = cx.update(|window, cx| {
                        folder.update(cx, |i, cx| {
                            if i.value().as_ref() == guess {
                                i.set_value(home, window, cx);
                            }
                        });
                    });
                }
            })
            .detach();
        }
        let note: Option<SharedString> = match (&cwd, trusted) {
            (None, _) => Some(t!("tx-drop-hint")),
            (Some(_), false) => Some(t!("tx-up-unverified", target = target.clone())),
            (Some(_), true) => None,
        };
        let dlg = cx.new(|_| UploadDialog {
            folder: folder.clone(),
            existing: Existing::default(),
            target,
            note,
            offer_integration: cwd.is_none() && !profile.shell_integration,
            source: source.clone(),
        });
        let me = cx.entity().downgrade();
        let paths = Arc::new(paths);
        let back = source.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialog, _, _| {
            let go = {
                let (me, dlg, folder, paths, profile) = (
                    me.clone(),
                    dlg.clone(),
                    folder.clone(),
                    paths.clone(),
                    profile.clone(),
                );
                move |window: &mut Window, cx: &mut App| {
                    let dir = folder.read(cx).value().trim().to_owned();
                    if dir.is_empty() {
                        return;
                    }
                    let existing = dlg.read(cx).existing;
                    window.close_dialog(cx);
                    let (paths, profile) = ((*paths).clone(), profile.clone());
                    let _ = me.update(cx, |app, cx| {
                        app.start_upload(profile, paths, dir, existing, window, cx)
                    });
                }
            };
            let go_ok = go.clone();
            let back = back.clone();
            dialog
                .title(crate::a11y::text(title.clone()))
                .w(px(600.0))
                .child(dlg.clone())
                .on_ok(move |_, window, cx| {
                    go_ok(window, cx);
                    false
                })
                .on_close(move |_, window, cx| back.focus(window, cx))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label(t!("button-cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("upload")
                                .primary()
                                .label(t!("tx-drop-upload"))
                                .on_click(move |_, window, cx| go(window, cx)),
                        ),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Existing, ExistingPolicy, free_remote_name, measure};
    use hostwick_sftp::{LocalFs, RemoteFs};

    #[test]
    fn the_default_never_replaces_anything() {
        assert_eq!(Existing::default(), Existing::Skip);
        assert_eq!(Existing::default().policy(), ExistingPolicy::Skip);
        assert_eq!(Existing::Rename.policy(), ExistingPolicy::Skip);
        assert_eq!(Existing::Overwrite.policy(), ExistingPolicy::Overwrite);
    }

    #[test]
    fn measuring_counts_files_and_bytes_in_folders() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(d.path().join("a/b")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("a/one"), vec![0u8; 100]).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("a/b/two"), vec![0u8; 20]).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.path().join("three"), vec![0u8; 3]).unwrap_or_else(|e| panic!("{e}"));
        let m = measure(&[d.path().join("a"), d.path().join("three")], 1000);
        assert_eq!((m.files, m.bytes, m.truncated), (3, 123, false));
        // A limit stops the walk and says so.
        let m = measure(&[d.path().to_path_buf()], 2);
        assert!(m.truncated);
    }

    #[tokio::test]
    async fn rename_picks_the_first_free_remote_name() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dir = d.path().to_string_lossy().replace('\\', "/");
        let fs: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
        assert_eq!(
            free_remote_name(&fs, &dir, "r.txt").await.ok().as_deref(),
            Some("r.txt")
        );
        std::fs::write(d.path().join("r.txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            free_remote_name(&fs, &dir, "r.txt").await.ok().as_deref(),
            Some("r (1).txt")
        );
        std::fs::write(d.path().join("r (1).txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            free_remote_name(&fs, &dir, "r.txt").await.ok().as_deref(),
            Some("r (2).txt")
        );
        // Folders and dotfiles.
        std::fs::create_dir(d.path().join("pics")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            free_remote_name(&fs, &dir, "pics").await.ok().as_deref(),
            Some("pics (1)")
        );
        std::fs::write(d.path().join(".env"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            free_remote_name(&fs, &dir, ".env").await.ok().as_deref(),
            Some(".env (1)")
        );
    }
}
