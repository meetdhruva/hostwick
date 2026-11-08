//! The status bar at the bottom of a terminal tab, and the polling behind
//! it: latency (an SSH keepalive round trip) and, when switched on, server
//! metrics (one small `sh` script over the shared connection).
//!
//! Polling happens only while the terminal is actually drawn, only for SSH
//! (telnet and serial never get a command), and never twice at once.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::zoom::px;
use gpui_kit::accesskit::Role;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_term::TransportKind;

use super::ext::HostInfo;
use super::metrics::{self, Metrics};
use super::{Note, TerminalView};
use crate::settings::Settings;

/// Height of the bar.
pub const HEIGHT: f32 = 22.0;
/// How often latency and metrics are read.
const EVERY: Duration = Duration::from_secs(5);
/// Fallback latency (a timed `true`) is at most this often.
const EXEC_LATENCY_EVERY: Duration = Duration::from_secs(30);
/// The view counts as visible when it was drawn this recently.
const VISIBLE_FOR: Duration = Duration::from_millis(2500);

/// Poller state that lives outside the UI thread.
#[derive(Default)]
pub struct Poll {
    pub(super) ping_busy: Arc<AtomicBool>,
    pub(super) metrics_busy: Arc<AtomicBool>,
    pub(super) last_ping: Option<Instant>,
    pub(super) last_metrics: Option<Instant>,
    pub(super) last_exec_latency: Option<Instant>,
    pub(super) ping_unsupported: bool,
}

impl TerminalView {
    /// Start the once-a-second housekeeping (poll decisions, bar refresh).
    pub(super) fn start_status(&mut self, cx: &mut Context<Self>) {
        if self.x.status.task.is_some() {
            return;
        }
        self.x.status.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |v, cx| v.status_tick(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    fn visible(&self) -> bool {
        self.x.painted.is_some_and(|t| t.elapsed() < VISIBLE_FOR)
    }

    fn status_tick(&mut self, cx: &mut Context<Self>) {
        let prefs = Settings::get(cx).term.clone();
        if !self.visible() || !prefs.status_bar || self.exited.is_some() {
            return;
        }
        // The connection time in the bar moves.
        if self.x.connected_at.elapsed().as_secs().is_multiple_of(15) {
            cx.notify();
        }
        if self.transport != TransportKind::Ssh {
            return;
        }
        let (Some(backend), Some(id)) = (self.x.backend.clone(), self.x.info.id) else {
            return;
        };
        let now = Instant::now();
        let poll = &mut self.x.status.poll;
        // Latency.
        if !poll.ping_busy.load(Ordering::Acquire)
            && poll
                .last_ping
                .is_none_or(|t| now.duration_since(t) >= EVERY)
        {
            poll.last_ping = Some(now);
            poll.ping_busy.store(true, Ordering::Release);
            let busy = poll.ping_busy.clone();
            let use_exec = poll.ping_unsupported
                && poll
                    .last_exec_latency
                    .is_none_or(|t| now.duration_since(t) >= EXEC_LATENCY_EVERY);
            if use_exec {
                poll.last_exec_latency = Some(now);
            }
            let skip = poll.ping_unsupported && !use_exec;
            let notes = self.notes.clone();
            let ping_backend = backend.clone();
            self.rt.spawn(async move {
                if !skip && let Some(conn) = ping_backend.pool.get(id).await {
                    let ms = if use_exec {
                        let t = Instant::now();
                        conn.exec(&hostwick_sftp::shell::portable("true"))
                            .await
                            .ok()
                            .map(|_| t.elapsed())
                    } else {
                        tokio::time::timeout(Duration::from_secs(8), conn.ping())
                            .await
                            .ok()
                            .and_then(Result::ok)
                    };
                    let _ = notes.unbounded_send(Note::Latency(
                        ms.map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX)),
                        !use_exec,
                    ));
                }
                busy.store(false, Ordering::Release);
            });
        }
        // Server metrics.
        let poll = &mut self.x.status.poll;
        if prefs.server_metrics
            && !poll.metrics_busy.load(Ordering::Acquire)
            && poll
                .last_metrics
                .is_none_or(|t| now.duration_since(t) >= EVERY)
        {
            poll.last_metrics = Some(now);
            poll.metrics_busy.store(true, Ordering::Release);
            let busy = poll.metrics_busy.clone();
            let notes = self.notes.clone();
            self.rt.spawn(async move {
                if let Some(conn) = backend.pool.get(id).await {
                    let cmd = hostwick_sftp::shell::portable(metrics::SCRIPT);
                    let out = tokio::time::timeout(Duration::from_secs(10), conn.exec(&cmd)).await;
                    if let Ok(Ok(o)) = out {
                        let _ = notes.unbounded_send(Note::Metrics(
                            String::from_utf8_lossy(&o.stdout).into_owned(),
                        ));
                    }
                }
                busy.store(false, Ordering::Release);
            });
        }
    }

    pub(super) fn on_latency(&mut self, ms: Option<u32>, via_ping: bool, cx: &mut Context<Self>) {
        match ms {
            Some(ms) => {
                let v = self.x.status.latency.push(ms as f32);
                self.x.status.latency_ms = Some(v.round() as u32);
            }
            None if via_ping => {
                // The server does not answer keepalives: time a command
                // instead (less often).
                self.x.status.poll.ping_unsupported = true;
                self.x.status.poll.last_ping = None;
            }
            None => self.x.status.latency_ms = None,
        }
        cx.notify();
    }

    pub(super) fn on_metrics(&mut self, raw: String, cx: &mut Context<Self>) {
        let m = self.x.status.sampler.sample(&raw);
        // The first Linux reading has no CPU figure (it needs a second
        // one): ask again soon.
        if m.cpu.is_none() && m.mem.is_some() && self.x.status.metrics.is_none() {
            self.x.status.poll.last_metrics = Some(Instant::now() - EVERY + Duration::from_secs(1));
        }
        self.x.status.metrics = Some(m);
        cx.notify();
    }

    /// Forget numbers of the old session.
    pub(super) fn reset_status(&mut self) {
        self.x.status.latency.reset();
        self.x.status.latency_ms = None;
        self.x.status.metrics = None;
        self.x.status.sampler.reset();
        self.x.status.poll = Poll::default();
        self.x.connected_at = Instant::now();
    }

    pub(super) fn render_status_bar(&self, logging: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let prefs = Settings::get(cx).term.clone();
        let info: &HostInfo = &self.x.info;
        let muted = theme.muted_foreground;
        let item = |id: &'static str, text: String, spoken: String| {
            div()
                .id(id)
                .flex_none()
                .whitespace_nowrap()
                .aria_label(spoken)
                .child(text)
        };
        // Narrow panes (splits) drop the least important items first.
        let cols = self.size.cols;
        let latency = self.x.status.latency_ms.filter(|_| cols >= 45).map(|ms| {
            (
                format!("{ms} ms"),
                t!("tx-status-latency", ms = ms).to_string(),
            )
        });
        let metrics = (prefs.server_metrics && cols >= 100)
            .then(|| self.x.status.metrics.clone())
            .flatten();
        let since = format_age(self.x.connected_at.elapsed());
        let cwd = self.cwd();
        let mut left: Vec<AnyElement> = Vec::new();
        let addr = info.address();
        left.push(
            item(
                "status-addr",
                addr.clone(),
                t!("tx-status-host", host = addr).to_string(),
            )
            .font_weight(FontWeight::MEDIUM)
            .into_any_element(),
        );
        left.push(
            div()
                .id("status-proto")
                .flex_none()
                .px(px(5.0))
                .rounded(px(3.0))
                .bg(theme.secondary)
                .text_color(theme.secondary_foreground)
                .text_size(px(10.0))
                .aria_label(t!("tx-status-protocol", protocol = info.protocol))
                .child(info.protocol)
                .into_any_element(),
        );
        div()
            .id("status-bar")
            .role(Role::Status)
            .flex_none()
            .h(px(HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(10.0))
            .bg(theme.tab_bar)
            .border_t_1()
            .border_color(theme.border)
            .text_size(px(11.0))
            .text_color(muted)
            .overflow_hidden()
            .children(left)
            .when_some(cwd, |d, dir| {
                d.child(
                    div()
                        .id("status-cwd")
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .aria_label(t!("tx-status-folder", folder = dir.clone()))
                        .child(format!("📁 {dir}")),
                )
            })
            .when(self.cwd().is_none(), |d| d.child(div().flex_1()))
            .when(logging, |d| {
                d.child(
                    item(
                        "status-log",
                        format!("● {}", t!("term-log-badge")),
                        t!("tx-status-logging").to_string(),
                    )
                    .text_color(hsla(0.0, 0.7, 0.6, 1.0)),
                )
            })
            .when_some(metrics, |d, m| d.child(metrics_view(&m)))
            .when_some(latency, |d, (text, spoken)| {
                d.child(item("status-latency", text, spoken))
            })
            .when(cols >= 60, |d| {
                d.child(item(
                    "status-since",
                    since.clone(),
                    t!("tx-status-connected", time = since.clone()).to_string(),
                ))
            })
            .into_any_element()
    }
}

fn metrics_view(m: &Metrics) -> AnyElement {
    let na = || "–".to_string();
    let cpu = m.cpu.map_or_else(na, |c| format!("{c:.0}%"));
    let mem = m.mem.map_or_else(na, |(u, t)| {
        format!(
            "{} / {}",
            metrics::format_bytes(u),
            metrics::format_bytes(t)
        )
    });
    let disk = m.disk_pct.map_or_else(na, |d| format!("{d}%"));
    let load = m
        .load
        .map_or_else(na, |l| format!("{:.2} {:.2} {:.2}", l[0], l[1], l[2]));
    let spoken = t!(
        "tx-status-metrics",
        cpu = cpu.clone(),
        mem = mem.clone(),
        disk = disk.clone(),
        load = load.clone()
    )
    .to_string();
    div()
        .id("status-metrics")
        .flex_none()
        .flex()
        .gap(px(10.0))
        .whitespace_nowrap()
        .aria_label(spoken)
        .child(format!("{} {cpu}", t!("tx-status-cpu")))
        .child(format!("{} {mem}", t!("tx-status-mem")))
        .child(format!("{} {disk}", t!("tx-status-disk")))
        .child(format!("{} {load}", t!("tx-status-load")))
        .into_any_element()
}

/// `45s`, `12m`, `3h 05m`.
pub fn format_age(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::format_age;

    #[test]
    fn ages() {
        assert_eq!(format_age(Duration::from_secs(5)), "5s");
        assert_eq!(format_age(Duration::from_secs(125)), "2m");
        assert_eq!(format_age(Duration::from_secs(3 * 3600 + 5 * 60)), "3h 05m");
    }
}
