//! Synchronize dialog (WinSCP style): pick the folders and direction,
//! preview the plan as a checklist with totals, then run the checked
//! actions through the transfer engine. Also starts "Keep remote directory
//! up to date".

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_sftp::{LocalFs, RemoteFs};
use hostwick_transfer::{
    ActionKind, PlanItem, PlanOptions, SyncDirection, SyncPlan, build_plan, execute_plan,
};
use tokio_util::sync::CancellationToken;

use super::FilesView;
use super::pane::Side;
use crate::util::human_size;

pub fn open(view: &mut FilesView, window: &mut Window, cx: &mut Context<FilesView>) {
    let local_dir = view.local.cwd.clone();
    let remote_dir = view.remote.cwd.clone();
    let remote_fs = view.remote.fs.clone();
    let rt = view.backend.handle();
    let weak = cx.entity().downgrade();
    let dlg = cx.new(|cx| SyncDialog::new(weak, remote_fs, rt, local_dir, remote_dir, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let d = dlg.read(cx);
        let (building, has_plan, one_way_up) = (
            d.building,
            d.plan.is_some(),
            d.direction == SyncDirection::LocalToRemote,
        );
        let (preview, run, keep) = (dlg.clone(), dlg.clone(), dlg.clone());
        dialog
            .title("Synchronize")
            .w(px(820.0))
            .child(dlg.clone())
            .footer(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    Button::new("close")
                        .label("Close")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("keep")
                        .label("Keep remote up to date")
                        .disabled(!one_way_up)
                        .tooltip(
                            "Watch the local folder and upload every change (local → remote only)",
                        )
                        .on_click(move |_, window, cx| {
                            keep.update(cx, |d, cx| d.keep_up_to_date(window, cx));
                        }),
                )
                .child(
                    Button::new("preview")
                        .label(if building { "Comparing…" } else { "Preview" })
                        .disabled(building)
                        .on_click(move |_, _, cx| preview.update(cx, |d, cx| d.preview(false, cx))),
                )
                .child(
                    Button::new("run")
                        .primary()
                        .label(if has_plan {
                            "Synchronize checked"
                        } else {
                            "Synchronize"
                        })
                        .disabled(building)
                        .on_click(move |_, window, cx| run.update(cx, |d, cx| d.run(window, cx))),
                ),
        )
    });
}

pub struct SyncDialog {
    view: WeakEntity<FilesView>,
    remote_fs: Arc<dyn RemoteFs>,
    rt: tokio::runtime::Handle,
    local_dir: Entity<InputState>,
    remote_dir: Entity<InputState>,
    excludes: Entity<InputState>,
    direction: SyncDirection,
    delete: bool,
    plan: Option<SyncPlan>,
    checked: Vec<bool>,
    building: bool,
    error: Option<String>,
    scroll: UniformListScrollHandle,
    _subs: Vec<Subscription>,
}

fn split_excludes(s: &str) -> Vec<String> {
    s.split([',', ' ', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

impl SyncDialog {
    fn new(
        view: WeakEntity<FilesView>,
        remote_fs: Arc<dyn RemoteFs>,
        rt: tokio::runtime::Handle,
        local: String,
        remote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let local_dir = cx.new(|cx| InputState::new(window, cx).default_value(local));
        let remote_dir = cx.new(|cx| InputState::new(window, cx).default_value(remote));
        let excludes = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. .git, *.tmp, node_modules")
                .default_value(".git")
        });
        let mut subs = Vec::new();
        for input in [&local_dir, &remote_dir, &excludes] {
            subs.push(cx.subscribe_in(
                input,
                window,
                |this: &mut Self, _, ev: &InputEvent, _, cx| {
                    if let InputEvent::Change = ev
                        && this.plan.is_some()
                    {
                        // The preview no longer matches what would run.
                        this.plan = None;
                        cx.notify();
                    }
                },
            ));
        }
        Self {
            view,
            remote_fs,
            rt,
            local_dir,
            remote_dir,
            excludes,
            direction: SyncDirection::LocalToRemote,
            delete: false,
            plan: None,
            checked: Vec::new(),
            building: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            _subs: subs,
        }
    }

    fn inputs(&self, cx: &App) -> (String, String, PlanOptions) {
        let l = self.local_dir.read(cx).value().trim().to_string();
        let r = self.remote_dir.read(cx).value().trim().to_string();
        let opts = PlanOptions {
            direction: self.direction,
            delete: self.delete && self.direction != SyncDirection::Both,
            excludes: split_excludes(&self.excludes.read(cx).value()),
        };
        (l, r, opts)
    }

    /// Compare both sides and show the plan.
    fn preview(&mut self, then_run: bool, cx: &mut Context<Self>) {
        let (l, r, opts) = self.inputs(cx);
        if l.is_empty() || r.is_empty() {
            self.error = Some("Choose both folders".into());
            cx.notify();
            return;
        }
        self.building = true;
        self.error = None;
        self.plan = None;
        let remote = self.remote_fs.clone();
        let job = self.rt.spawn(async move {
            let local = LocalFs::system();
            let remote_abs = remote.canonicalize(&r).await.unwrap_or(r);
            build_plan(&local, &l, remote.as_ref(), &remote_abs, opts).await
        });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |d, cx| {
                d.building = false;
                match res {
                    Ok(Ok(plan)) => {
                        d.checked = plan.items.iter().map(PlanItem::default_selected).collect();
                        if then_run {
                            d.start(plan, cx);
                        } else {
                            d.plan = Some(plan);
                        }
                    }
                    Ok(Err(e)) => d.error = Some(e.to_string()),
                    Err(e) => d.error = Some(format!("internal error: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.plan.take() {
            Some(plan) => {
                self.start(plan, cx);
                window.close_dialog(cx);
            }
            None => {
                // Compare and run in the view: this dialog is going away.
                let (l, r, opts) = self.inputs(cx);
                if l.is_empty() || r.is_empty() {
                    self.error = Some("Choose both folders".into());
                    cx.notify();
                    return;
                }
                let _ = self.view.update(cx, |v, cx| v.plan_and_run(l, r, opts, cx));
                window.close_dialog(cx);
            }
        }
    }

    /// Hand the plan to the view, which runs it after the dialog closes.
    fn start(&mut self, plan: SyncPlan, cx: &mut Context<Self>) {
        let checked = std::mem::take(&mut self.checked);
        let _ = self.view.update(cx, |v, cx| v.run_plan(plan, checked, cx));
    }

    fn keep_up_to_date(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (l, r, opts) = self.inputs(cx);
        if l.is_empty() || r.is_empty() {
            self.error = Some("Choose both folders".into());
            cx.notify();
            return;
        }
        let _ = self.view.update(cx, |v, cx| {
            v.sync = None;
            v.start_sync(l, r, opts.excludes, cx);
        });
        window.close_dialog(cx);
    }

    fn render_item(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(it) = self.plan.as_ref().and_then(|p| p.items.get(ix)).cloned() else {
            return div().into_any_element();
        };
        let checked = self.checked.get(ix).copied().unwrap_or(false);
        let color = match it.kind {
            ActionKind::Upload => theme.success,
            ActionKind::Download => theme.info,
            ActionKind::DeleteLocal | ActionKind::DeleteRemote => theme.danger,
            ActionKind::Conflict => theme.warning,
            ActionKind::Skip => theme.muted_foreground,
        };
        let arrow = match it.kind {
            ActionKind::Upload => "→",
            ActionKind::Download => "←",
            ActionKind::DeleteLocal => "✕ local",
            ActionKind::DeleteRemote => "✕ remote",
            ActionKind::Conflict => "⚠",
            ActionKind::Skip => "·",
        };
        let path = if it.is_dir {
            format!("{}/", it.rel_path)
        } else {
            it.rel_path.clone()
        };
        div()
            .id(("plan", ix))
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(26.0))
            .px(px(6.0))
            .text_size(px(12.5))
            .hover(|s| s.bg(theme.list_hover))
            .child(
                Checkbox::new(("plan-check", ix))
                    .checked(checked && it.kind.is_action())
                    .disabled(!it.kind.is_action())
                    .on_click(cx.listener(move |d, on: &bool, _, cx| {
                        if let Some(c) = d.checked.get_mut(ix) {
                            *c = *on;
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .text_color(color)
                    .child(format!("{arrow} {}", it.kind.label())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(path),
            )
            .child(
                div()
                    .w(px(170.0))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(theme.muted_foreground)
                    .child(it.reason.describe()),
            )
            .child(
                div()
                    .w(px(70.0))
                    .flex_none()
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(if it.bytes() > 0 {
                        human_size(it.bytes())
                    } else {
                        String::new()
                    }),
            )
            .when(it.kind == ActionKind::Conflict, |d| {
                d.child(
                    Button::new(("use-local", ix))
                        .xsmall()
                        .label("Use local")
                        .disabled(it.local.is_none())
                        .on_click(
                            cx.listener(move |d, _, _, cx| d.resolve(ix, ActionKind::Upload, cx)),
                        ),
                )
                .child(
                    Button::new(("use-remote", ix))
                        .xsmall()
                        .label("Use remote")
                        .disabled(it.remote.is_none())
                        .on_click(
                            cx.listener(move |d, _, _, cx| d.resolve(ix, ActionKind::Download, cx)),
                        ),
                )
            })
            .into_any_element()
    }

    fn resolve(&mut self, ix: usize, kind: ActionKind, cx: &mut Context<Self>) {
        if let Some(plan) = self.plan.as_mut()
            && let Some(it) = plan.items.get_mut(ix)
            && it.resolve(kind)
            && let Some(c) = self.checked.get_mut(ix)
        {
            *c = true;
        }
        cx.notify();
    }
}

impl Render for SyncDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let dir_button =
            |id: &'static str, label: &'static str, dir: SyncDirection, cur: SyncDirection| {
                Button::new(id)
                    .small()
                    .label(label)
                    .selected(dir == cur)
                    .when(dir == cur, |b| b.primary())
            };
        let cur = self.direction;
        let set_dir = |dir: SyncDirection| {
            cx.listener(move |d: &mut Self, _, _, cx| {
                d.direction = dir;
                d.plan = None;
                cx.notify();
            })
        };
        let label = |s: &'static str| {
            div()
                .w(px(110.0))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(s)
        };
        let summary = self.plan.as_ref().map(|p| {
            let checked = &self.checked;
            let t = p.totals(|i, _| checked.get(i).copied().unwrap_or(false));
            let mut parts = vec![
                format!("{} upload(s), {}", t.uploads, human_size(t.upload_bytes)),
                format!(
                    "{} download(s), {}",
                    t.downloads,
                    human_size(t.download_bytes)
                ),
            ];
            if t.delete_local + t.delete_remote > 0 {
                parts.push(format!("{} deletion(s)", t.delete_local + t.delete_remote));
            }
            if t.conflicts > 0 {
                parts.push(format!(
                    "{} conflict(s) — pick a side or leave them",
                    t.conflicts
                ));
            }
            if p.items.is_empty() {
                "Both folders are already in sync.".to_owned()
            } else {
                format!("Checked: {}", parts.join(" · "))
            }
        });
        let count = self.plan.as_ref().map_or(0, |p| p.items.len());

        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .text_size(px(13.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label("Local folder"))
                    .child(div().flex_1().child(Input::new(&self.local_dir).small())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label("Remote folder"))
                    .child(div().flex_1().child(Input::new(&self.remote_dir).small())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(label("Direction"))
                    .child(
                        dir_button(
                            "dir-up",
                            "Local → remote",
                            SyncDirection::LocalToRemote,
                            cur,
                        )
                        .on_click(set_dir(SyncDirection::LocalToRemote)),
                    )
                    .child(
                        dir_button(
                            "dir-down",
                            "Remote → local",
                            SyncDirection::RemoteToLocal,
                            cur,
                        )
                        .on_click(set_dir(SyncDirection::RemoteToLocal)),
                    )
                    .child(
                        dir_button("dir-both", "Both (newer wins)", SyncDirection::Both, cur)
                            .on_click(set_dir(SyncDirection::Both)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label(""))
                    .child(
                        Checkbox::new("delete")
                            .label("Delete files missing from the source (mirror)")
                            .checked(self.delete && cur != SyncDirection::Both)
                            .disabled(cur == SyncDirection::Both)
                            .on_click(cx.listener(|d, on: &bool, _, cx| {
                                d.delete = *on;
                                d.plan = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(label("Exclude"))
                    .child(div().flex_1().child(Input::new(&self.excludes).small())),
            )
            .when_some(self.error.clone(), |d, e| {
                d.child(div().text_color(theme.danger).child(e))
            })
            .when_some(summary, |d, s| {
                d.child(
                    div()
                        .pt(px(4.0))
                        .text_color(theme.muted_foreground)
                        .child(s),
                )
            })
            .when(count > 0, |d| {
                d.child(
                    div()
                        .h(px(300.0))
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(theme.border)
                        .rounded(px(6.0))
                        .child(
                            uniform_list(
                                "plan",
                                count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    range.map(|ix| this.render_item(ix, cx)).collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .flex_1(),
                        ),
                )
            })
    }
}

impl FilesView {
    /// "Synchronize" without a preview: compare, then run what the plan
    /// checks by default.
    pub(super) fn plan_and_run(
        &mut self,
        local_dir: String,
        remote_dir: String,
        opts: PlanOptions,
        cx: &mut Context<Self>,
    ) {
        let remote = self.remote.fs.clone();
        self.banner = Some((false, "Comparing folders…".into()));
        let job = self.backend.rt.spawn(async move {
            let local = LocalFs::system();
            let remote_abs = remote.canonicalize(&remote_dir).await.unwrap_or(remote_dir);
            build_plan(&local, &local_dir, remote.as_ref(), &remote_abs, opts).await
        });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| match res {
                Ok(Ok(plan)) => {
                    let checked = plan.items.iter().map(PlanItem::default_selected).collect();
                    v.run_plan(plan, checked, cx);
                }
                Ok(Err(e)) => {
                    v.banner = Some((true, format!("Synchronize: {e}")));
                    cx.notify();
                }
                Err(e) => {
                    v.banner = Some((true, format!("internal error: {e}")));
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Run the checked plan items; the transfer panel shows the copies,
    /// the banner the final report.
    pub(super) fn run_plan(&mut self, plan: SyncPlan, checked: Vec<bool>, cx: &mut Context<Self>) {
        let engine = self.backend.engine.clone();
        let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
        let remote = self.remote.fs.clone();
        let cancel = CancellationToken::new();
        self.banner = Some((false, "Synchronizing…".into()));
        let job = self.backend.rt.spawn(async move {
            execute_plan(
                &engine,
                local,
                remote,
                &plan,
                |i, _| checked.get(i).copied().unwrap_or(true),
                &cancel,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |v, cx| {
                v.banner = Some(match res {
                    Ok(r) if r.failed.is_empty() => {
                        (false, format!("Synchronize: {}", r.summary()))
                    }
                    Ok(r) => (
                        true,
                        format!(
                            "Synchronize: {} — {}",
                            r.summary(),
                            r.failed
                                .iter()
                                .take(5)
                                .map(|(p, e)| format!("{p}: {e}"))
                                .collect::<Vec<_>>()
                                .join("; ")
                        ),
                    ),
                    Err(e) => (true, format!("Synchronize failed: {e}")),
                });
                v.refresh(Side::Local, cx);
                v.refresh(Side::Remote, cx);
            });
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn excludes_split_on_commas_and_spaces() {
        assert_eq!(
            super::split_excludes(" .git, *.tmp node_modules;;x "),
            vec![".git", "*.tmp", "node_modules", "x"]
        );
    }
}
