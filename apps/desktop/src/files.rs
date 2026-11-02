//! SFTP file browser tab: virtualized listing, navigation, upload/download
//! through the shared transfer engine, new folder, delete.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_sftp::{Entry, EntryKind, LocalFs, RemoteFs, path_to_slash};
use hostwick_transfer::{ExistingPolicy, Job};

use crate::backend::Backend;
use crate::util::{human_size, human_time};

pub struct FilesView {
    backend: Arc<Backend>,
    fs: Arc<dyn RemoteFs>,
    cwd: String,
    entries: Vec<Entry>,
    selected: Option<usize>,
    loading: bool,
    error: Option<String>,
    path_input: Entity<InputState>,
    path_input_dirty: bool,
    focus: FocusHandle,
    _load: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Focusable for FilesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FilesView {
    pub fn new(
        fs: Arc<dyn RemoteFs>,
        backend: Arc<Backend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path_input = cx.new(|cx| InputState::new(window, cx).placeholder("/path"));
        let sub = cx.subscribe_in(
            &path_input,
            window,
            |this: &mut Self, input, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let path = input.read(cx).value().to_string();
                    this.navigate(path, cx);
                }
            },
        );
        let mut v = Self {
            backend,
            fs,
            cwd: ".".into(),
            entries: Vec::new(),
            selected: None,
            loading: false,
            error: None,
            path_input,
            path_input_dirty: false,
            focus: cx.focus_handle(),
            _load: None,
            _subs: vec![sub],
        };
        v.navigate(".".into(), cx);
        v
    }

    fn navigate(&mut self, path: String, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let fs = self.fs.clone();
        let job = self.backend.rt.spawn(async move {
            let abs = fs.canonicalize(&path).await?;
            let list = fs.list_dir(&abs).await?;
            Ok::<_, hostwick_sftp::FsError>((abs, list))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |v, cx| {
                v.loading = false;
                match result {
                    Ok(Ok((abs, list))) => {
                        v.cwd = abs;
                        v.entries = list;
                        v.selected = None;
                        v.path_input_dirty = true;
                    }
                    Ok(Err(e)) => v.error = Some(e.to_string()),
                    Err(e) => v.error = Some(format!("task failed: {e}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.navigate(self.cwd.clone(), cx);
    }

    fn up(&mut self, cx: &mut Context<Self>) {
        self.navigate(hostwick_sftp::parent(&self.cwd), cx);
    }

    fn open(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(e) = self
            .entries
            .get(ix)
            .filter(|e| e.kind == EntryKind::Dir || e.kind == EntryKind::Symlink)
        {
            let path = e.path.clone();
            self.navigate(path, cx);
        }
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)).cloned() else {
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Download here".into()),
        });
        let (fs, backend) = (self.fs.clone(), self.backend.clone());
        cx.spawn(async move |_, _| {
            let Ok(Ok(Some(dirs))) = picked.await else {
                return;
            };
            let Some(dir) = dirs.into_iter().next() else {
                return;
            };
            let dst_dir = path_to_slash(&dir);
            let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
            let engine = backend.engine.clone();
            let target = hostwick_sftp::join(&dst_dir, &entry.name);
            let _ = backend
                .rt
                .spawn(async move {
                    if entry.is_dir() {
                        let _ = engine
                            .enqueue_tree(
                                fs,
                                &entry.path,
                                local,
                                &target,
                                ExistingPolicy::Overwrite,
                            )
                            .await;
                    } else {
                        engine.enqueue(Job {
                            src: fs,
                            src_path: entry.path.clone(),
                            dst: local,
                            dst_path: target,
                            policy: ExistingPolicy::Overwrite,
                        });
                    }
                })
                .await;
        })
        .detach();
    }

    fn upload(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Upload".into()),
        });
        let (fs, backend, cwd) = (self.fs.clone(), self.backend.clone(), self.cwd.clone());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
            let engine = backend.engine.clone();
            let ids = backend
                .rt
                .spawn(async move {
                    let mut ids = Vec::new();
                    for p in paths {
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let src = path_to_slash(&p);
                        let dst = hostwick_sftp::join(&cwd, &name);
                        if p.is_dir() {
                            if let Ok(more) = engine
                                .enqueue_tree(
                                    local.clone(),
                                    &src,
                                    fs.clone(),
                                    &dst,
                                    ExistingPolicy::Overwrite,
                                )
                                .await
                            {
                                ids.extend(more);
                            }
                        } else {
                            ids.push(engine.enqueue(Job {
                                src: local.clone(),
                                src_path: src,
                                dst: fs.clone(),
                                dst_path: dst,
                                policy: ExistingPolicy::Overwrite,
                            }));
                        }
                    }
                    for id in ids {
                        engine.wait(id).await;
                    }
                })
                .await;
            if ids.is_ok() {
                let _ = this.update(cx, |v, cx| v.refresh(cx));
            }
        })
        .detach();
    }

    fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        let view = cx.entity().downgrade();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            let (input2, view2) = (input.clone(), view.clone());
            let create = std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let name = input2.read(cx).value().trim().to_owned();
                if !name.is_empty() && !name.contains('/') {
                    let _ = view2.update(cx, |v, cx| v.mkdir(name, cx));
                }
                window.close_dialog(cx);
            });
            let (on_click, on_enter) = (create.clone(), create.clone());
            dialog
                .title("New folder")
                .child(Input::new(&input))
                .on_ok(move |_, window, cx| {
                    on_enter(window, cx);
                    false
                })
                .footer(
                    Button::new("create")
                        .primary()
                        .label("Create")
                        .on_click(move |_, window, cx| on_click(window, cx)),
                )
        });
    }

    fn mkdir(&mut self, name: String, cx: &mut Context<Self>) {
        let (fs, path) = (self.fs.clone(), hostwick_sftp::join(&self.cwd, &name));
        let job = self.backend.rt.spawn(async move { fs.mkdir(&path).await });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                if let Ok(Err(e)) = res {
                    v.error = Some(e.to_string());
                }
                v.refresh(cx);
            });
        })
        .detach();
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)).cloned() else {
            return;
        };
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (view2, entry2) = (view.clone(), entry.clone());
            let what = if entry.is_dir() {
                "folder and everything in it"
            } else {
                "file"
            };
            dialog
                .title("Delete")
                .child(format!("Permanently delete the {what} “{}”?", entry.name))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("delete").danger().label("Delete").on_click(
                            move |_, window, cx| {
                                let _ = view2.update(cx, |v, cx| v.delete(entry2.clone(), cx));
                                window.close_dialog(cx);
                            },
                        )),
                )
        });
    }

    fn delete(&mut self, entry: Entry, cx: &mut Context<Self>) {
        let fs = self.fs.clone();
        let job = self
            .backend
            .rt
            .spawn(async move { remove_recursive(fs.as_ref(), &entry).await });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                if let Ok(Err(e)) = res {
                    v.error = Some(e.to_string());
                }
                v.refresh(cx);
            });
        })
        .detach();
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let e = &self.entries[ix];
        let selected = self.selected == Some(ix);
        let theme = cx.theme();
        let (icon, size) = match e.kind {
            EntryKind::Dir => (IconName::Folder, String::new()),
            EntryKind::Symlink => (IconName::ExternalLink, String::new()),
            _ => (IconName::File, human_size(e.size)),
        };
        div()
            .id(("row", ix))
            .w_full()
            .flex()
            .items_center()
            .h(px(26.0))
            .px(px(8.0))
            .gap(px(8.0))
            .text_size(px(13.0))
            .when(selected, |d| d.bg(theme.list_active))
            .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
            .on_click(cx.listener(move |v, ev: &ClickEvent, _, cx| {
                v.selected = Some(ix);
                if ev.click_count() >= 2 {
                    v.open(ix, cx);
                }
                cx.notify();
            }))
            .child(div().w(px(18.0)).child(icon))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(e.name.clone()),
            )
            .child(
                div()
                    .w(px(90.0))
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(size),
            )
            .child(
                div()
                    .w(px(140.0))
                    .text_color(theme.muted_foreground)
                    .child(e.modified.map(human_time).unwrap_or_default()),
            )
            .child(
                div()
                    .w(px(90.0))
                    .text_color(theme.muted_foreground)
                    .child(e.permissions.map(|p| format!("{p:o}")).unwrap_or_default()),
            )
            .into_any_element()
    }
}

async fn remove_recursive(fs: &dyn RemoteFs, entry: &Entry) -> Result<(), hostwick_sftp::FsError> {
    if !entry.is_dir() {
        return fs.remove_file(&entry.path).await;
    }
    // Iterative post-order delete: files first, then directories deepest-first.
    let mut dirs = vec![entry.path.clone()];
    let mut stack = vec![entry.path.clone()];
    while let Some(d) = stack.pop() {
        for e in fs.list_dir(&d).await? {
            if e.is_dir() {
                dirs.push(e.path.clone());
                stack.push(e.path);
            } else {
                fs.remove_file(&e.path).await?;
            }
        }
    }
    for d in dirs.iter().rev() {
        fs.remove_dir(d).await?;
    }
    Ok(())
}

impl Render for FilesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.path_input_dirty) {
            let cwd = self.cwd.clone();
            self.path_input
                .update(cx, |s, cx| s.set_value(cwd, window, cx));
        }
        let theme = cx.theme().clone();
        let count = self.entries.len();
        let has_sel = self.selected.is_some();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .p(px(8.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("up")
                            .small()
                            .label("↑ Up")
                            .on_click(cx.listener(|v, _, _, cx| v.up(cx))),
                    )
                    .child(
                        Button::new("refresh")
                            .small()
                            .label("Refresh")
                            .on_click(cx.listener(|v, _, _, cx| v.refresh(cx))),
                    )
                    .child(div().flex_1().child(Input::new(&self.path_input).small()))
                    .child(
                        Button::new("upload")
                            .small()
                            .primary()
                            .label("Upload")
                            .on_click(cx.listener(|v, _, _, cx| v.upload(cx))),
                    )
                    .child(
                        Button::new("download")
                            .small()
                            .label("Download")
                            .disabled(!has_sel)
                            .on_click(cx.listener(|v, _, _, cx| v.download(cx))),
                    )
                    .child(
                        Button::new("mkdir")
                            .small()
                            .label("New folder")
                            .on_click(cx.listener(|v, _, w, cx| v.new_folder(w, cx))),
                    )
                    .child(
                        Button::new("rm")
                            .small()
                            .danger()
                            .label("Delete")
                            .disabled(!has_sel)
                            .on_click(cx.listener(|v, _, w, cx| v.confirm_delete(w, cx))),
                    ),
            )
            .when_some(self.error.clone(), |d, err| {
                d.child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .text_size(px(13.0))
                        .text_color(theme.danger)
                        .child(err),
                )
            })
            .child(
                div()
                    .flex()
                    .px(px(8.0))
                    .gap(px(8.0))
                    .h(px(24.0))
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().w(px(18.0)))
                    .child(div().flex_1().child("Name"))
                    .child(div().w(px(90.0)).text_right().child("Size"))
                    .child(div().w(px(140.0)).child("Modified (UTC)"))
                    .child(div().w(px(90.0)).child("Mode")),
            )
            .child(
                uniform_list(
                    "entries",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted_foreground)
                    .border_t_1()
                    .border_color(theme.border)
                    .child(if self.loading {
                        "Loading…".to_owned()
                    } else {
                        format!("{count} item(s) — {}", self.cwd)
                    }),
            )
    }
}
