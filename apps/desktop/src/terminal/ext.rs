//! v0.4 additions to the terminal view, kept together so the view itself
//! stays readable: what it knows about its host, finished-command
//! tracking, an in-terminal transfer, clickable output, the status bar.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hostwick_core::{Environment, HostProfile, Protocol};

use gpui_kit::Context;

use super::{TerminalEvent, TerminalView, links, metrics, osc133, transfer};
use crate::backend::Backend;
use crate::settings::Settings;

/// What the terminal needs to know about the host it shows.
#[derive(Debug, Clone, Default)]
pub struct HostInfo {
    pub name: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub protocol: &'static str,
    pub env: Environment,
    pub shell_integration: bool,
    pub id: Option<hostwick_core::ProfileId>,
}

impl HostInfo {
    pub fn from_profile(p: &HostProfile) -> Self {
        Self {
            name: p.name.clone(),
            user: p.user_or_local(),
            host: p.host.clone(),
            port: p.port,
            protocol: match p.protocol {
                Protocol::Ssh => "SSH",
                Protocol::Telnet => "Telnet",
                Protocol::Serial(_) => "Serial",
            },
            env: p.environment.clone(),
            shell_integration: p.shell_integration,
            id: Some(p.id),
        }
    }

    /// `user@host:port` (serial: the device).
    pub fn address(&self) -> String {
        if self.protocol == "Serial" {
            return self.host.clone();
        }
        if self.user.is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// Things the terminal asks the app to do (the app owns tabs, hosts and
/// the file browsers).
#[derive(Debug, Clone)]
pub enum TermExt {
    /// BEL rang.
    Bell,
    /// A command that ran for a while has finished.
    Finished {
        command: Option<String>,
        exit: Option<i32>,
        took: Duration,
    },
    /// Text to paste. The app decides whether it needs a look first: it
    /// knows about broadcast and about the other terminals' environments.
    Paste { text: String },
    /// Files dropped on the terminal.
    DropFiles(Vec<PathBuf>),
    /// Show a remote path in the file browser, or download it.
    OpenPath { path: String, download: bool },
    /// Quick connect to a host or address found in the output.
    Connect(String),
    /// A `file:` link on this computer: show it in the file manager (after
    /// asking).
    RevealFile(String),
    /// "Enable now" for shell integration was pressed.
    EnableIntegration,
}

/// A link under the pointer.
#[derive(Debug, Clone)]
pub struct Hover {
    pub found: links::Found,
    /// Grid line of the first row and the columns covered on each row.
    pub segments: Vec<(i32, std::ops::Range<usize>)>,
}

/// Live numbers for the status bar.
#[derive(Default)]
pub struct Status {
    pub latency: metrics::Smoother,
    pub latency_ms: Option<u32>,
    pub metrics: Option<metrics::Metrics>,
    pub sampler: metrics::Sampler,
    pub poll: super::status::Poll,
    pub task: Option<gpui_kit::Task<()>>,
}

pub struct Ext {
    pub(super) info: HostInfo,
    pub(super) backend: Option<Arc<Backend>>,
    pub(super) tracker: osc133::CommandTracker,
    pub(super) xfer: Option<transfer::XferUi>,
    pub(super) finished: Option<transfer::Finished>,
    pub(super) finished_gen: u64,
    pub(super) download_dir: Option<PathBuf>,
    pub(super) hover: Option<Hover>,
    /// The cell the hover was computed for.
    pub(super) hover_cell: Option<(usize, usize)>,
    /// Ctrl (⌘) held: links underline.
    pub(super) link_mod: bool,
    pub(super) status: Status,
    /// When the view was last drawn (status polling only while visible).
    pub(super) painted: Option<Instant>,
    pub(super) connected_at: Instant,
}

impl Default for Ext {
    fn default() -> Self {
        Self {
            info: HostInfo::default(),
            backend: None,
            tracker: osc133::CommandTracker::default(),
            xfer: None,
            finished: None,
            finished_gen: 0,
            download_dir: None,
            hover: None,
            hover_cell: None,
            link_mod: false,
            status: Status::default(),
            painted: None,
            connected_at: Instant::now(),
        }
    }
}

impl TerminalView {
    /// The host this terminal shows (call when a session is attached).
    pub fn set_host(
        &mut self,
        profile: &HostProfile,
        backend: Arc<Backend>,
        cx: &mut Context<Self>,
    ) {
        self.x.info = HostInfo::from_profile(profile);
        self.x.backend = Some(backend);
        self.reset_status();
        self.start_status(cx);
        cx.notify();
    }

    pub fn host_info(&self) -> &HostInfo {
        &self.x.info
    }

    /// OSC 133 marks from the parser: finished commands.
    pub(super) fn on_marks(&mut self, marks: Vec<osc133::Mark>, cx: &mut Context<Self>) {
        let now = Instant::now();
        let prefs = Settings::get(cx).term.clone();
        for m in marks {
            if let Some(f) = self.x.tracker.mark(m, now)
                && prefs.notify_finished
                && f.took >= Duration::from_secs(u64::from(prefs.finished_after_secs))
            {
                cx.emit(TerminalEvent::Ext(TermExt::Finished {
                    command: f.command,
                    exit: f.exit,
                    took: f.took,
                }));
            }
        }
    }
}

impl TerminalView {
    /// A paste the person confirmed (or "as a single line").
    pub fn paste_confirmed(&mut self, text: String, single_line: bool, cx: &mut Context<Self>) {
        let text = if single_line {
            super::paste::single_line(&text)
        } else {
            text
        };
        let bracketed = self
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE);
        self.send_user(super::keys::paste_bytes(&text, bracketed), cx);
    }

    /// Set up shell integration in the running session. False when it is
    /// not a good moment (a full-screen program is running, the session is
    /// over).
    pub fn inject_integration(&mut self, cx: &mut Context<Self>) -> bool {
        use alacritty_terminal::term::TermMode;
        if self.exited.is_some() || self.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        self.x.info.shell_integration = true;
        self.start_script(Vec::new(), false, true, cx);
        true
    }

    /// The shell has not reported its folder and shell integration is off.
    pub fn can_offer_integration(&self) -> bool {
        self.transport == hostwick_term::TransportKind::Ssh
            && self.cwd.is_none()
            && !self.x.info.shell_integration
            && self.exited.is_none()
    }
}

impl TerminalView {
    /// Files dropped on the terminal.
    pub(super) fn files_dropped(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !files.is_empty() {
            cx.emit(TerminalEvent::Ext(TermExt::DropFiles(files)));
        }
    }

    /// The environment strip above the terminal: a thin line in the
    /// host's colour, and for production a red band with the host's name
    /// that stays put.
    pub(super) fn render_ribbon(
        &self,
        env: &Environment,
        host: &str,
    ) -> Option<gpui_kit::AnyElement> {
        use gpui_kit::accesskit::Role;
        use gpui_kit::*;
        let (r, g, b) = env.rgb()?;
        let color: Hsla =
            gpui_kit::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into();
        if env.is_prod() {
            let text = t!("tx-env-prod-ribbon", host = host.to_owned());
            Some(
                div()
                    .id("env-ribbon")
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .flex_none()
                    .h(px(18.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(color)
                    .text_color(hsla(0.0, 0.0, 1.0, 1.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::BOLD)
                    .child(text)
                    .into_any_element(),
            )
        } else {
            Some(
                div()
                    .id("env-line")
                    .flex_none()
                    .h(px(3.0))
                    .w_full()
                    .bg(color)
                    .into_any_element(),
            )
        }
    }
}
