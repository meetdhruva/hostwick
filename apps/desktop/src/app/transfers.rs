//! The transfer panel: every running, paused, failed and finished transfer,
//! with pause and resume (one or all), retry, cancel, "show in folder",
//! speed and ETA, a collapsed one-line summary, and a notification when a
//! batch is done while the person is looking elsewhere.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_transfer::{TransferEvent, TransferId, TransferState};

use super::HostwickApp;
use crate::notify::Toast;
use crate::util::human_size;
use crate::zoom::px;

/// A batch that ran at least this long always gets a notification.
pub(super) const LONG_BATCH: Duration = Duration::from_secs(10);
/// Wait this long after the last transfer ends before calling the batch
/// done: a folder's files are queued one after another.
pub(super) const BATCH_SETTLE: Duration = Duration::from_millis(1500);
/// Rows kept in the panel.
const MAX_ROWS: usize = 200;

pub(super) struct TransferRow {
    pub id: TransferId,
    pub label: String,
    pub done: u64,
    pub total: u64,
    pub rate: f64,
    pub state: Option<TransferState>,
    pub paused: bool,
    /// The engine reported progress: the transfer has really started.
    pub started: bool,
    /// Where a finished download is on this computer.
    pub local_dst: Option<PathBuf>,
}

impl TransferRow {
    fn unfinished(&self) -> bool {
        self.state.is_none()
    }

    fn failed(&self) -> bool {
        matches!(self.state, Some(TransferState::Failed { .. }))
    }

    fn running(&self) -> bool {
        self.unfinished() && self.started && !self.paused
    }
}

/// What one batch of transfers amounted to, once it is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BatchDone {
    pub files: usize,
    pub failed: usize,
    pub bytes: u64,
    pub elapsed: Duration,
}

#[derive(Default)]
struct Batch {
    started: Option<Instant>,
    files: usize,
    failed: usize,
    bytes: u64,
    /// Bumped on every change, so a settle timer knows it is stale.
    generation: u64,
}

#[derive(Default)]
pub(super) struct TransferPanel {
    pub rows: Vec<TransferRow>,
    pub collapsed: bool,
    pub paused_all: bool,
    batch: Batch,
}

/// Counts and speed over all rows, for the summary line.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Totals {
    pub running: usize,
    pub waiting: usize,
    pub paused: usize,
    pub failed: usize,
    pub done: usize,
    /// Bytes per second over the running transfers.
    pub speed: f64,
    /// Bytes still to move in unfinished transfers whose size is known.
    pub remaining: u64,
}

impl TransferPanel {
    pub(super) fn unfinished(&self) -> usize {
        self.rows.iter().filter(|r| r.unfinished()).count()
    }

    pub(super) fn failed_ids(&self) -> Vec<TransferId> {
        self.rows
            .iter()
            .filter(|r| r.failed())
            .map(|r| r.id)
            .collect()
    }

    pub(super) fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for r in &self.rows {
            match &r.state {
                None if r.paused || self.paused_all => t.paused += 1,
                None if r.started => t.running += 1,
                None => t.waiting += 1,
                Some(TransferState::Failed { .. }) => t.failed += 1,
                Some(TransferState::Done { .. } | TransferState::Skipped) => t.done += 1,
                Some(_) => {}
            }
            if r.running() && !self.paused_all {
                t.speed += r.rate;
            }
            if r.unfinished() {
                t.remaining += r.total.saturating_sub(r.done);
            }
        }
        t
    }

    pub(super) fn batch_generation(&self) -> u64 {
        self.batch.generation
    }

    /// Apply an engine event. Returns true when it changed what is shown.
    pub(super) fn on_event(&mut self, ev: TransferEvent) -> bool {
        match ev {
            TransferEvent::Queued {
                id,
                label,
                dst_path,
                dst_local,
                replaces,
                ..
            } => {
                if let Some(old) = replaces {
                    self.rows.retain(|r| r.id != old);
                }
                self.rows.push(TransferRow {
                    id,
                    label,
                    done: 0,
                    total: 0,
                    rate: 0.0,
                    state: None,
                    paused: false,
                    started: false,
                    local_dst: dst_local.then(|| PathBuf::from(dst_path)),
                });
                if self.rows.len() > MAX_ROWS {
                    self.rows.retain(|r| r.unfinished());
                }
                if self.batch.started.is_none() {
                    self.batch.started = Some(Instant::now());
                }
                self.batch.generation += 1;
                true
            }
            TransferEvent::Progress {
                id,
                done,
                total,
                bytes_per_sec,
            } => {
                if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
                    (r.done, r.total, r.rate) = (done, total, bytes_per_sec);
                    r.started = true;
                }
                true
            }
            TransferEvent::Finished { id, state } => {
                if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
                    match &state {
                        TransferState::Done { .. } => {
                            r.done = r.total;
                            self.batch.files += 1;
                            self.batch.bytes += r.total;
                        }
                        TransferState::Skipped => self.batch.files += 1,
                        TransferState::Failed { .. } => self.batch.failed += 1,
                        _ => {}
                    }
                    r.rate = 0.0;
                    r.paused = false;
                    r.state = Some(state);
                }
                self.batch.generation += 1;
                true
            }
            TransferEvent::Paused { id, paused } => {
                if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
                    r.paused = paused;
                }
                true
            }
            TransferEvent::PausedAll { paused } => {
                self.paused_all = paused;
                true
            }
        }
    }

    /// Nothing left to run: the batch is over (call after [`BATCH_SETTLE`]
    /// without new events, passing the generation seen when it started).
    /// Returns what it amounted to, once, and starts a fresh batch.
    pub(super) fn finish_batch(&mut self, generation: u64) -> Option<BatchDone> {
        if generation != self.batch.generation || self.unfinished() > 0 {
            return None;
        }
        let b = std::mem::take(&mut self.batch);
        let started = b.started?;
        (b.files + b.failed > 0).then(|| BatchDone {
            files: b.files,
            failed: b.failed,
            bytes: b.bytes,
            elapsed: started.elapsed(),
        })
    }
}

/// Seconds left at `speed` bytes/s; none when nothing is moving.
pub(super) fn eta_seconds(remaining: u64, speed: f64) -> Option<u64> {
    if remaining == 0 || !speed.is_finite() || speed < 1.0 {
        return None;
    }
    Some((remaining as f64 / speed).ceil() as u64)
}

/// `0:42`, `12:05`, `1:02:03`.
pub(super) fn format_eta(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Should a finished batch be announced? Always when it took a while, and
/// whenever the person is not looking at the window.
pub(super) fn should_announce(elapsed: Duration, window_focused: bool) -> bool {
    elapsed >= LONG_BATCH || !window_focused
}

/// "3 running · 41 MB/s · ETA 0:42 · 1 failed".
pub(super) fn summary(t: &Totals) -> String {
    let mut parts: Vec<String> = Vec::new();
    if t.running > 0 {
        parts.push(t!("transfers-sum-running", count = t.running).to_string());
        if t.speed >= 1.0 {
            parts.push(t!("transfers-sum-speed", rate = human_size(t.speed as u64)).to_string());
        }
        if let Some(eta) = eta_seconds(t.remaining, t.speed) {
            parts.push(t!("transfers-sum-eta", eta = format_eta(eta)).to_string());
        }
    }
    if t.waiting > 0 {
        parts.push(t!("transfers-sum-waiting", count = t.waiting).to_string());
    }
    if t.paused > 0 {
        parts.push(t!("transfers-sum-paused", count = t.paused).to_string());
    }
    if t.failed > 0 {
        parts.push(t!("transfers-sum-failed", count = t.failed).to_string());
    }
    if parts.is_empty() && t.done > 0 {
        parts.push(t!("transfers-sum-done", count = t.done).to_string());
    }
    parts.join(" · ")
}

impl HostwickApp {
    pub(super) fn on_transfer(&mut self, ev: TransferEvent, cx: &mut Context<Self>) {
        let finished = matches!(ev, TransferEvent::Finished { .. });
        self.transfers.on_event(ev);
        if finished && self.transfers.unfinished() == 0 {
            self.schedule_batch_check(cx);
        }
        cx.notify();
    }

    /// Call the batch over once nothing new has started for a moment.
    fn schedule_batch_check(&mut self, cx: &mut Context<Self>) {
        let generation = self.transfers.batch_generation();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BATCH_SETTLE).await;
            let _ = this.update(cx, |app, cx| app.batch_check(generation, cx));
        })
        .detach();
    }

    fn batch_check(&mut self, generation: u64, cx: &mut Context<Self>) {
        let Some(done) = self.transfers.finish_batch(generation) else {
            return;
        };
        // One notification, from the first window.
        if !self.is_primary(cx) {
            return;
        }
        let focused = cx.active_window().is_some();
        if !should_announce(done.elapsed, focused) {
            return;
        }
        let size = human_size(done.bytes);
        let text = if done.failed > 0 {
            t!(
                "transfers-batch-failed",
                done = done.files,
                failed = done.failed
            )
        } else {
            t!("transfers-batch-done", count = done.files, size = size)
        };
        let title = t!("transfers-batch-title");
        crate::notify::push(
            if done.failed > 0 {
                Toast::warning(text.clone()).title(title.clone())
            } else {
                Toast::success(text.clone()).title(title.clone())
            },
            cx,
        );
        cx.refresh_windows();
        if !focused {
            crate::desknotify::send(&title, &text);
        }
    }

    pub(super) fn pause_transfer(&mut self, id: TransferId, pause: bool, cx: &mut Context<Self>) {
        if pause {
            self.backend.engine.pause(id);
        } else {
            self.backend.engine.resume(id);
        }
        cx.notify();
    }

    pub(super) fn toggle_pause_all(&mut self, cx: &mut Context<Self>) {
        if self.backend.engine.is_paused_all() {
            self.backend.engine.resume_all();
            // "Resume all" means all: also those paused one by one.
            for row in self.transfers.rows.iter().filter(|r| r.paused) {
                self.backend.engine.resume(row.id);
            }
        } else {
            self.backend.engine.pause_all();
        }
        cx.notify();
    }

    pub(super) fn cancel_all_transfers(&mut self, cx: &mut Context<Self>) {
        // A paused transfer must not stay paused for ever once cancelled.
        self.backend.engine.cancel_all();
        cx.notify();
    }

    pub(super) fn retry_transfer(&mut self, id: TransferId, cx: &mut Context<Self>) {
        // The replacement arrives as a Queued event that removes this row.
        let _ = self.backend.engine.retry(id);
        cx.notify();
    }

    pub(super) fn retry_failed_transfers(&mut self, cx: &mut Context<Self>) {
        for id in self.transfers.failed_ids() {
            let _ = self.backend.engine.retry(id);
        }
        cx.notify();
    }

    pub(super) fn clear_finished_transfers(&mut self, cx: &mut Context<Self>) {
        self.transfers.rows.retain(|t| t.unfinished());
        self.backend.engine.prune_finished();
        cx.notify();
    }

    pub(super) fn render_transfers(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = &self.transfers;
        if panel.rows.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let totals = panel.totals();
        let active = panel.unfinished();
        let collapsed = panel.collapsed;
        let paused_all = panel.paused_all;
        let any_failed = totals.failed > 0;
        let sum = summary(&totals);

        let rows: Vec<AnyElement> = if collapsed {
            Vec::new()
        } else {
            panel
                .rows
                .iter()
                .rev()
                .take(60)
                .map(|t| self.render_transfer_row(t, paused_all, cx))
                .collect()
        };

        let toggle_label = if collapsed {
            t!("transfers-expand")
        } else {
            t!("transfers-collapse")
        };
        let header = div()
            .id("transfers-header")
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_size(px(12.0))
            .child(
                Button::new("transfers-toggle")
                    .xsmall()
                    .ghost()
                    .icon(if collapsed {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .tooltip(toggle_label.clone())
                    .accessibility_label(toggle_label)
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.transfers.collapsed = !app.transfers.collapsed;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::a11y::text(t!("transfers-title", count = active))),
            )
            .child(
                div()
                    .id("transfers-summary")
                    .role(Role::Status)
                    .aria_label(sum.clone())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(theme.muted_foreground)
                    .child(sum),
            )
            .when(active > 0, |d| {
                d.child(
                    Button::new("pause-all")
                        .xsmall()
                        .ghost()
                        .icon(if paused_all {
                            IconName::Play
                        } else {
                            IconName::Pause
                        })
                        .label(if paused_all {
                            t!("transfers-resume-all")
                        } else {
                            t!("transfers-pause-all")
                        })
                        .on_click(cx.listener(|app, _, _, cx| app.toggle_pause_all(cx))),
                )
                .child(
                    Button::new("cancel-all")
                        .xsmall()
                        .ghost()
                        .label(t!("transfers-cancel-all"))
                        .on_click(cx.listener(|app, _, _, cx| app.cancel_all_transfers(cx))),
                )
            })
            .when(any_failed, |d| {
                d.child(
                    Button::new("retry-failed")
                        .xsmall()
                        .ghost()
                        .icon(IconName::RotateCw)
                        .label(t!("transfers-retry-failed"))
                        .on_click(cx.listener(|app, _, _, cx| app.retry_failed_transfers(cx))),
                )
            })
            .child(
                Button::new("clear-transfers")
                    .xsmall()
                    .ghost()
                    .label(t!("transfers-clear"))
                    .on_click(cx.listener(|app, _, _, cx| app.clear_finished_transfers(cx))),
            );

        Some(
            div()
                .flex_none()
                .border_t_1()
                .border_color(theme.border)
                .p(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(header)
                .when(!collapsed, |d| {
                    d.child(
                        div()
                            .id("transfer-rows")
                            .max_h(px(190.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(rows),
                    )
                })
                .into_any_element(),
        )
    }

    fn render_transfer_row(
        &self,
        t: &TransferRow,
        paused_all: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let pct = if t.total == 0 {
            if t.state.is_some() { 1.0 } else { 0.0 }
        } else {
            t.done as f32 / t.total as f32
        };
        let paused = t.paused || paused_all;
        let (status, color) = match &t.state {
            None if paused => (
                t!(
                    "transfer-paused",
                    done = human_size(t.done),
                    total = human_size(t.total)
                )
                .to_string(),
                theme.muted_foreground,
            ),
            None if !t.started => (t!("transfer-waiting").to_string(), theme.muted_foreground),
            None => {
                let base = t!(
                    "transfer-progress",
                    done = human_size(t.done),
                    total = human_size(t.total),
                    rate = human_size(t.rate as u64)
                )
                .to_string();
                let eta = eta_seconds(t.total.saturating_sub(t.done), t.rate)
                    .map(|s| format!(" · {}", t!("transfers-sum-eta", eta = format_eta(s))))
                    .unwrap_or_default();
                (format!("{base}{eta}"), theme.primary)
            }
            Some(TransferState::Done { .. }) => (
                t!("transfer-done", size = human_size(t.total)).to_string(),
                theme.success,
            ),
            Some(TransferState::Skipped) => {
                (t!("transfer-skipped").to_string(), theme.muted_foreground)
            }
            Some(TransferState::Cancelled) => {
                (t!("transfer-cancelled").to_string(), theme.muted_foreground)
            }
            Some(TransferState::Failed { error }) => (
                t!("transfer-failed", error = error.to_string()).to_string(),
                theme.danger,
            ),
            Some(_) => (String::new(), theme.muted_foreground),
        };
        let id = t.id;
        let running = t.unfinished();
        let failed = t.failed();
        let reveal = matches!(t.state, Some(TransferState::Done { .. }))
            .then(|| t.local_dst.clone())
            .flatten();
        div()
            .id(SharedString::from(format!("transfer-row-{}", id.0)))
            .flex()
            .items_center()
            .gap(px(10.0))
            .text_size(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(crate::a11y::text(t.label.clone())),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(120.0))
                    .h(px(6.0))
                    .rounded(px(3.0))
                    .bg(theme.muted)
                    .child(
                        div()
                            .h_full()
                            .rounded(px(3.0))
                            .bg(color)
                            .w(relative(pct.clamp(0.0, 1.0))),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(280.0))
                    .text_color(theme.muted_foreground)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(crate::a11y::text(status)),
            )
            .when(running, |d| {
                let label = if paused {
                    t!("transfer-resume")
                } else {
                    t!("transfer-pause")
                };
                d.child(
                    Button::new(SharedString::from(format!("pause-{}", id.0)))
                        .xsmall()
                        .ghost()
                        .icon(if paused {
                            IconName::Play
                        } else {
                            IconName::Pause
                        })
                        .tooltip(label.clone())
                        .accessibility_label(label)
                        .on_click(cx.listener(move |app, _, _, cx| {
                            // A row paused only by "pause all" resumes with it.
                            let now_paused = app
                                .transfers
                                .rows
                                .iter()
                                .find(|r| r.id == id)
                                .is_some_and(|r| r.paused);
                            if app.transfers.paused_all && !now_paused {
                                app.toggle_pause_all(cx);
                            } else {
                                app.pause_transfer(id, !now_paused, cx);
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("cancel-{}", id.0)))
                        .xsmall()
                        .ghost()
                        .label(t!("transfer-cancel"))
                        .on_click(cx.listener(move |app, _, _, _| app.backend.engine.cancel(id))),
                )
            })
            .when(failed, |d| {
                d.child(
                    Button::new(SharedString::from(format!("retry-{}", id.0)))
                        .xsmall()
                        .ghost()
                        .icon(IconName::RotateCw)
                        .label(t!("transfer-retry"))
                        .on_click(cx.listener(move |app, _, _, cx| app.retry_transfer(id, cx))),
                )
            })
            .when_some(reveal, |d, path| {
                d.child(
                    Button::new(SharedString::from(format!("reveal-{}", id.0)))
                        .xsmall()
                        .ghost()
                        .icon(IconName::FolderOpen)
                        .label(t!("transfer-show-in-folder"))
                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use hostwick_transfer::{TransferEvent, TransferId, TransferState};

    use super::{
        LONG_BATCH, Totals, TransferPanel, eta_seconds, format_eta, should_announce, summary,
    };

    fn id(n: u128) -> TransferId {
        TransferId(uuid::Uuid::from_u128(n))
    }

    fn queued(n: u128, dst_local: bool, replaces: Option<u128>) -> TransferEvent {
        TransferEvent::Queued {
            id: id(n),
            label: format!("f{n}"),
            dst_path: format!("/tmp/f{n}"),
            dst_local,
            src_path: format!("/srv/f{n}"),
            src_local: false,
            replaces: replaces.map(id),
        }
    }

    fn progress(n: u128, done: u64, total: u64, rate: f64) -> TransferEvent {
        TransferEvent::Progress {
            id: id(n),
            done,
            total,
            bytes_per_sec: rate,
        }
    }

    fn done(n: u128, bytes: u64) -> TransferEvent {
        TransferEvent::Finished {
            id: id(n),
            state: TransferState::Done {
                bytes,
                resumed_from: 0,
            },
        }
    }

    fn failed(n: u128) -> TransferEvent {
        TransferEvent::Finished {
            id: id(n),
            state: TransferState::Failed {
                error: "boom".into(),
            },
        }
    }

    #[test]
    fn eta_and_its_format() {
        assert_eq!(eta_seconds(41_000_000, 1_000_000.0), Some(41));
        assert_eq!(eta_seconds(1, 1000.0), Some(1));
        assert_eq!(eta_seconds(0, 1000.0), None);
        assert_eq!(eta_seconds(100, 0.0), None);
        assert_eq!(eta_seconds(100, f64::NAN), None);
        assert_eq!(format_eta(42), "0:42");
        assert_eq!(format_eta(725), "12:05");
        assert_eq!(format_eta(3723), "1:02:03");
        assert_eq!(format_eta(0), "0:00");
    }

    #[test]
    fn rows_follow_events_and_totals_add_up() {
        let mut p = TransferPanel::default();
        for n in 1..=4 {
            p.on_event(queued(n, n == 1, None));
        }
        assert_eq!(p.unfinished(), 4);
        // Row 1 is a download: its place on this computer is known.
        assert_eq!(p.rows[0].local_dst, Some(PathBuf::from("/tmp/f1")));
        assert_eq!(p.rows[1].local_dst, None);
        p.on_event(progress(1, 100, 1000, 500.0));
        p.on_event(progress(2, 0, 3000, 1500.0));
        let t = p.totals();
        assert_eq!((t.running, t.waiting, t.paused), (2, 2, 0));
        assert_eq!(t.speed, 2000.0);
        assert_eq!(t.remaining, 900 + 3000);
        // Pausing one row moves it out of "running" and out of the speed.
        p.on_event(TransferEvent::Paused {
            id: id(2),
            paused: true,
        });
        let t = p.totals();
        assert_eq!((t.running, t.paused, t.speed as u64), (1, 1, 500));
        // Pause all: everything unfinished counts as paused.
        p.on_event(TransferEvent::PausedAll { paused: true });
        let t = p.totals();
        assert_eq!(
            (t.running, t.waiting, t.paused, t.speed as u64),
            (0, 0, 4, 0)
        );
        p.on_event(TransferEvent::PausedAll { paused: false });
        // Finish one, fail one.
        p.on_event(done(1, 1000));
        p.on_event(failed(3));
        let t = p.totals();
        assert_eq!((t.done, t.failed), (1, 1));
        assert_eq!(p.failed_ids(), vec![id(3)]);
        assert_eq!(p.rows[0].done, p.rows[0].total);
        // A retry replaces the failed row.
        p.on_event(queued(9, false, Some(3)));
        assert!(p.rows.iter().all(|r| r.id != id(3)));
        assert_eq!(p.failed_ids(), Vec::<TransferId>::new());
    }

    #[test]
    fn summary_reads_like_the_spec() {
        crate::i18n::set_language("en");
        let t = Totals {
            running: 3,
            speed: 41.0 * 1024.0 * 1024.0,
            remaining: 42 * 41 * 1024 * 1024,
            ..Default::default()
        };
        let s = summary(&t);
        assert!(s.starts_with("3 running · "), "{s}");
        assert!(s.contains("/s"), "{s}");
        assert!(s.contains("ETA 0:42"), "{s}");
        let t = Totals {
            running: 1,
            failed: 2,
            paused: 1,
            waiting: 4,
            ..Default::default()
        };
        let s = summary(&t);
        assert!(s.contains("1 running") && s.contains("4 waiting"), "{s}");
        assert!(s.contains("1 paused") && s.contains("2 failed"), "{s}");
        assert!(!s.contains("ETA"), "no speed, no ETA: {s}");
        assert_eq!(
            summary(&Totals {
                done: 5,
                ..Default::default()
            }),
            "5 done"
        );
        assert_eq!(summary(&Totals::default()), "");
    }

    #[test]
    fn a_batch_is_announced_once_when_everything_ended() {
        let mut p = TransferPanel::default();
        p.on_event(queued(1, true, None));
        p.on_event(queued(2, true, None));
        p.on_event(progress(1, 10, 10, 1.0));
        p.on_event(done(1, 10));
        // One still running: not over.
        let g = p.batch_generation();
        assert_eq!(p.finish_batch(g), None);
        p.on_event(failed(2));
        let g = p.batch_generation();
        // A newer event after the timer was set makes the check stale.
        assert_eq!(p.finish_batch(g - 1), None);
        let Some(b) = p.finish_batch(g) else {
            panic!("the batch is over");
        };
        assert_eq!((b.files, b.failed, b.bytes), (1, 1, 10));
        // Said once; the next batch starts clean.
        assert_eq!(p.finish_batch(g), None);
        p.on_event(queued(3, true, None));
        p.on_event(TransferEvent::Finished {
            id: id(3),
            state: TransferState::Cancelled,
        });
        // Only cancellations: nothing to announce.
        let g = p.batch_generation();
        assert_eq!(p.finish_batch(g), None);
    }

    #[test]
    fn announcements_follow_duration_and_focus() {
        assert!(should_announce(Duration::from_secs(11), true));
        assert!(should_announce(Duration::from_secs(2), false));
        assert!(!should_announce(Duration::from_secs(2), true));
        assert!(should_announce(LONG_BATCH, true));
    }
}
