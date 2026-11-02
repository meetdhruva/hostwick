//! Terminal tab: a transport (SSH shell, telnet or serial port) driving an
//! Alacritty terminal grid, painted with GPUI.
//!
//! Threading (the "no hangs" part):
//! - Output is parsed into the grid on a Tokio task, never on the UI
//!   thread. The grid lives behind Alacritty's `FairMutex`, held only briefly
//!   by the parser (per batch) and by the painter (per frame).
//! - The parser wakes the UI at most once per batch via a coalescing flag, so
//!   a flood of output (`cat` of a large file) costs one repaint per frame,
//!   not one per packet.
//! - Keystrokes go out through an unbounded, ordered queue, so typing never
//!   blocks on the network.
//! - Login scripts run on the Tokio runtime and wait on the parser's output
//!   watch; the scrollback match count runs on a blocking thread in short
//!   locked chunks.

pub mod colors;
mod element;
pub mod highlight;
pub mod integration;
pub mod keys;
pub mod mouse;
pub mod osc7;
pub mod script;
mod search;
mod sessionlog;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::search::{Match, RegexSearch};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use bytes::Bytes;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, LogMode, Protocol, ScriptStep, Snippet};
use hostwick_term::{Control, ShellCommand, ShellEvent, TerminalSession, TransportKind};

use crate::settings::{RightClick, Settings};
pub use sessionlog::SessionLogger;

pub use element::TerminalElement;

/// Grid size handed to Alacritty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

enum Note {
    Wakeup,
    Title(String),
    Bell,
    Exited(Option<String>),
    /// OSC 7 from the shell.
    Cwd(String),
    /// Something to show briefly at the bottom of the terminal.
    Notice(String),
    ScriptDone,
    /// Background match count finished (generation, result).
    SearchCount(u64, search::Count),
}

/// Receives terminal-side events from the parser (answers to queries such as
/// "where is the cursor?" must go back to the remote program).
#[derive(Clone)]
pub struct Listener {
    input: UnboundedSender<ShellCommand>,
    notes: UnboundedSender<Note>,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::PtyWrite(s) => {
                let _ = self
                    .input
                    .unbounded_send(ShellCommand::Data(Bytes::from(s.into_bytes())));
            }
            TermEvent::Title(t) => {
                let _ = self.notes.unbounded_send(Note::Title(t));
            }
            TermEvent::Bell => {
                let _ = self.notes.unbounded_send(Note::Bell);
            }
            _ => {}
        }
    }
}

struct Shared {
    term: FairMutex<Term<Listener>>,
    dirty: AtomicBool,
    /// Session log, written by the parser task alongside the grid.
    log: Mutex<Option<SessionLogger>>,
    /// Recent output text, for login-script `@wait`s.
    watch: script::Watch,
}

#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Bytes the user typed or pasted (used for broadcast input).
    UserInput(Vec<u8>),
    TitleChanged,
    Exited,
    /// The shell reported a new current folder (OSC 7). The file browser
    /// can follow it.
    #[allow(dead_code)]
    CwdChanged(String),
    /// Menu requests the app handles (it owns tabs and profiles).
    Reconnect,
    Duplicate,
    OpenFiles,
    LoggingChanged,
}

/// Geometry of the last paint, for mapping mouse positions to cells.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
}

/// Scrollback search bar state.
struct SearchBar {
    input: Entity<InputState>,
    case_sensitive: bool,
    regex: bool,
    compiled: Option<RegexSearch>,
    error: Option<String>,
    focused: Option<Match>,
    count: Option<search::Count>,
    counting: bool,
    generation: Arc<AtomicU64>,
    _sub: Subscription,
}

/// Mouse buttons currently held and reported to the remote program.
#[derive(Default)]
struct MouseReport {
    pressed: Option<mouse::Button>,
    last_cell: Option<(usize, usize)>,
}

/// What the painter needs from the search bar for one frame.
pub(crate) struct SearchPaint {
    pub matches: Vec<Match>,
    pub focused: Option<Match>,
}

pub struct TerminalView {
    shared: Arc<Shared>,
    input: UnboundedSender<ShellCommand>,
    notes: UnboundedSender<Note>,
    control: Option<tokio::sync::mpsc::Sender<Control>>,
    transport: TransportKind,
    rt: tokio::runtime::Handle,
    focus: FocusHandle,
    pub title: SharedString,
    pub exited: Option<String>,
    size: GridSize,
    layout: Option<Layout>,
    selecting: bool,
    pub(crate) font: Font,
    pub(crate) font_size: Pixels,
    cwd: Option<String>,
    /// The host's own snippets (the global ones come from Settings).
    host_snippets: Vec<Snippet>,
    script: Option<tokio::task::JoinHandle<()>>,
    notice: Option<SharedString>,
    notice_gen: u64,
    search: Option<SearchBar>,
    pub(crate) highlighter: Arc<highlight::Highlighter>,
    pub(crate) row_cache: highlight::RowCache,
    mouse: MouseReport,
    _pump: Task<()>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for TerminalView {
    /// Closing the tab (or replacing it on reconnect) hangs up: the SSH
    /// channel closes, the telnet socket shuts down, the serial port is
    /// released for the next session.
    fn drop(&mut self) {
        let _ = self.input.unbounded_send(ShellCommand::Close);
        if let Some(t) = self.script.take() {
            t.abort();
        }
        if let Some(s) = &self.search {
            s.generation.fetch_add(1, Ordering::AcqRel);
        }
    }
}

pub fn monospace_family() -> SharedString {
    if let Ok(f) = std::env::var("HOSTWICK_FONT") {
        return f.into();
    }
    if cfg!(target_os = "windows") {
        "Consolas".into()
    } else if cfg!(target_os = "macos") {
        "Menlo".into()
    } else {
        "DejaVu Sans Mono".into()
    }
}

/// Tab title for a host: its name, plus the protocol for telnet and the
/// device and line settings for serial ports.
pub fn tab_title(profile: &HostProfile) -> String {
    match &profile.protocol {
        Protocol::Ssh => profile.name.clone(),
        Protocol::Telnet => format!("{} (telnet)", profile.name),
        Protocol::Serial(s) => {
            let line = hostwick_term::serial::summary(s);
            if profile.name.trim() == profile.host.trim() || profile.name.trim().is_empty() {
                format!("{} {line}", profile.host.trim())
            } else {
                format!("{} ({} {line})", profile.name, profile.host.trim())
            }
        }
    }
}

impl TerminalView {
    /// Wire a freshly opened session (SSH, telnet or serial) into a new
    /// terminal view.
    pub fn new(
        title: impl Into<SharedString>,
        session: TerminalSession,
        initial: GridSize,
        rt: &tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let parts = session.into_parts();
        let (shell_in, mut shell_out) = (parts.input, parts.output);
        let (input_tx, mut input_rx) = unbounded::<ShellCommand>();
        let (notes_tx, notes_rx) = unbounded::<Note>();

        let listener = Listener {
            input: input_tx.clone(),
            notes: notes_tx.clone(),
        };
        let settings = Settings::get(cx).clone();
        let config = Config {
            scrolling_history: settings.scrollback_lines,
            ..Config::default()
        };
        let shared = Arc::new(Shared {
            term: FairMutex::new(Term::new(config, &initial, listener)),
            dirty: AtomicBool::new(false),
            log: Mutex::new(None),
            watch: script::Watch::default(),
        });

        // Ordered input pump: UI queue → transport (with backpressure there).
        rt.spawn(async move {
            while let Some(cmd) = input_rx.next().await {
                if shell_in.send(cmd).await.is_err() {
                    break;
                }
            }
        });

        // Parser: output → grid, off the UI thread.
        let parser_shared = shared.clone();
        let parser_notes = notes_tx.clone();
        rt.spawn(async move {
            let notes_tx = parser_notes;
            let mut processor: Processor = Processor::new();
            let mut osc7 = osc7::Osc7Scanner::default();
            let mut exit: Option<String> = None;
            let wake = |s: &Shared| {
                if !s.dirty.swap(true, Ordering::AcqRel) {
                    let _ = notes_tx.unbounded_send(Note::Wakeup);
                }
            };
            'outer: while let Some(ev) = shell_out.recv().await {
                let mut pending = Some(ev);
                let mut cwd = None;
                {
                    let mut term = parser_shared.term.lock();
                    // Drain everything already queued under one lock.
                    while let Some(ev) = pending.take() {
                        match ev {
                            ShellEvent::Stdout(b) | ShellEvent::Stderr(b) => {
                                if let Some(dir) = osc7.feed(&b) {
                                    cwd = Some(dir);
                                }
                                processor.advance(&mut *term, &b);
                                parser_shared.watch.push(&b);
                                if let Ok(mut log) = parser_shared.log.lock()
                                    && let Some(l) = log.as_mut()
                                    && let Err(e) = l.write(&b)
                                {
                                    tracing::warn!(error = %e, "session log write failed; logging stopped");
                                    *log = None;
                                }
                            }
                            ShellEvent::Exit(code) => {
                                exit = Some(format!("exited with status {code}"))
                            }
                            ShellEvent::ExitSignal(sig) => {
                                exit = Some(format!("killed by signal {sig}"))
                            }
                            ShellEvent::Closed => break 'outer,
                        }
                        pending = shell_out.try_recv().ok();
                    }
                }
                if let Some(dir) = cwd {
                    let _ = notes_tx.unbounded_send(Note::Cwd(dir));
                }
                wake(&parser_shared);
            }
            parser_shared.watch.close();
            wake(&parser_shared);
            let _ = notes_tx.unbounded_send(Note::Exited(
                exit.or_else(|| Some("connection closed".into())),
            ));
        });

        let pump = cx.spawn_in(window, async move |this, cx| {
            let mut notes: UnboundedReceiver<Note> = notes_rx;
            while let Some(note) = notes.next().await {
                let alive = this.update(cx, |view, cx| view.on_note(note, cx)).is_ok();
                if !alive {
                    break;
                }
            }
        });

        Self {
            shared,
            input: input_tx,
            notes: notes_tx,
            control: parts.control,
            transport: parts.kind,
            rt: rt.clone(),
            focus: cx.focus_handle(),
            title: title.into(),
            exited: None,
            size: initial,
            layout: None,
            selecting: false,
            font: Font {
                family: if settings.font_family.trim().is_empty() {
                    monospace_family()
                } else {
                    settings.font_family.clone().into()
                },
                ..Font::default()
            },
            font_size: px(settings.font_size),
            cwd: None,
            host_snippets: Vec::new(),
            script: None,
            notice: None,
            notice_gen: 0,
            search: None,
            highlighter: highlight::shared(&settings.highlight_rules),
            row_cache: highlight::RowCache::default(),
            mouse: MouseReport::default(),
            _pump: pump,
        }
    }

    fn on_note(&mut self, note: Note, cx: &mut Context<Self>) {
        match note {
            Note::Wakeup => {
                self.shared.dirty.store(false, Ordering::Release);
                cx.notify();
            }
            Note::Title(t) => {
                self.title = t.into();
                cx.emit(TerminalEvent::TitleChanged);
                cx.notify();
            }
            Note::Bell => {}
            Note::Exited(reason) => {
                self.exited = reason;
                self.mouse = MouseReport::default();
                cx.emit(TerminalEvent::Exited);
                cx.notify();
            }
            Note::Cwd(dir) => {
                if self.cwd.as_deref() != Some(dir.as_str()) {
                    self.cwd = Some(dir.clone());
                    cx.emit(TerminalEvent::CwdChanged(dir));
                }
            }
            Note::Notice(text) => self.show_notice(text, cx),
            Note::ScriptDone => {
                self.script = None;
                cx.notify();
            }
            Note::SearchCount(generation, count) => {
                if let Some(s) = self.search.as_mut()
                    && s.generation.load(Ordering::Acquire) == generation
                {
                    s.counting = false;
                    s.count = Some(count);
                    if s.focused.is_none() {
                        // Nothing near the bottom: start from the newest
                        // match anywhere.
                        self.step_search(true, cx);
                    }
                    cx.notify();
                }
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.exited.is_none()
    }

    /// Which transport this terminal runs over.
    pub fn transport(&self) -> TransportKind {
        self.transport
    }

    /// The shell's current folder, when it reports one (OSC 7; see
    /// `HostProfile::shell_integration`).
    pub fn cwd(&self) -> Option<String> {
        self.cwd.clone()
    }

    /// Whether "Send Break" does anything (telnet, serial).
    pub fn can_send_break(&self) -> bool {
        self.control.is_some() && self.exited.is_none()
    }

    /// Send a BREAK (serial line break, telnet `IAC BRK`).
    pub fn send_break(&mut self, cx: &mut Context<Self>) {
        match &self.control {
            Some(tx) if self.exited.is_none() => {
                if tx.try_send(Control::Break).is_err() {
                    self.show_notice("Could not send break".into(), cx);
                }
            }
            _ => self.show_notice(
                format!("{} sessions have no break signal", self.transport.label()),
                cx,
            ),
        }
    }

    /// Re-read font, scrollback and highlighting after the settings changed.
    pub fn apply_settings(&mut self, s: &Settings, cx: &mut Context<Self>) {
        self.font.family = if s.font_family.trim().is_empty() {
            monospace_family()
        } else {
            s.font_family.clone().into()
        };
        self.font_size = px(s.font_size);
        self.highlighter = highlight::shared(&s.highlight_rules);
        self.shared.term.lock().set_options(Config {
            scrolling_history: s.scrollback_lines,
            ..Config::default()
        });
        cx.notify();
    }

    // ---- profile automation: snippets, login script, shell integration ----

    /// Take the host's snippets, then run its login script and shell
    /// integration (in that order: a script that switches user with
    /// `sudo -i` gets the integration in the final shell). Call right after
    /// the session opens.
    pub fn attach_profile(&mut self, profile: &HostProfile, cx: &mut Context<Self>) {
        self.host_snippets = profile.snippets.clone();
        let steps = if profile.login_script.trim().is_empty() {
            Vec::new()
        } else {
            match hostwick_core::parse_login_script(&profile.login_script) {
                Ok(steps) => steps,
                Err(e) => {
                    self.show_notice(format!("Login script not run: {e}"), cx);
                    Vec::new()
                }
            }
        };
        if !steps.is_empty() || profile.shell_integration {
            self.start_script(steps, true, profile.shell_integration, cx);
        }
        cx.notify();
    }

    /// Run script steps now (cancelling any script still running): `Send`
    /// types a line, `WaitFor` waits for output text, `Sleep` pauses. A
    /// failure (e.g. a wait timing out) shows a notice; the script stops
    /// quietly when the session ends.
    #[allow(dead_code)] // API for the app ("Run script…", automation)
    pub fn run_script(&mut self, steps: Vec<ScriptStep>, cx: &mut Context<Self>) {
        self.start_script(steps, false, false, cx);
    }

    pub fn is_script_running(&self) -> bool {
        self.script.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub fn cancel_script(&mut self, cx: &mut Context<Self>) {
        if let Some(t) = self.script.take() {
            t.abort();
            cx.notify();
        }
    }

    fn start_script(
        &mut self,
        steps: Vec<ScriptStep>,
        from_start: bool,
        integration: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(t) = self.script.take() {
            t.abort();
        }
        if self.exited.is_some() {
            return;
        }
        let shared = self.shared.clone();
        let input = self.input.clone();
        let notes = self.notes.clone();
        let task = self.rt.spawn(async move {
            let send = |b: Vec<u8>| {
                input
                    .unbounded_send(ShellCommand::Data(Bytes::from(b)))
                    .is_ok()
            };
            if !steps.is_empty() {
                match script::run(steps, &shared.watch, from_start, send).await {
                    Ok(()) => {}
                    Err(script::ScriptError::Ended) => return,
                    Err(e) => {
                        let _ = notes.unbounded_send(Note::Notice(format!("Script stopped: {e}")));
                        let _ = notes.unbounded_send(Note::ScriptDone);
                        return;
                    }
                }
            }
            if integration {
                // Let the shell print its prompt first.
                if !shared
                    .watch
                    .settle(Duration::from_millis(400), Duration::from_secs(10))
                    .await
                {
                    return;
                }
                let (col, cols) = {
                    let t = shared.term.lock();
                    (t.grid().cursor.point.column.0, t.columns())
                };
                let mut cmd = integration::setup_command(col, cols).into_bytes();
                cmd.push(b'\r');
                send(cmd);
            }
            let _ = notes.unbounded_send(Note::ScriptDone);
        });
        self.script = Some(task);
        cx.notify();
    }

    /// Snippets for this terminal: the global ones, then the host's.
    pub fn snippets(&self, cx: &App) -> Vec<Snippet> {
        let mut v = Settings::get(cx).snippets.clone();
        v.extend(self.host_snippets.iter().cloned());
        v
    }

    /// Type a snippet: as a bracketed paste when the program enabled it
    /// (so multi-line text is not run line by line), then Enter if the
    /// snippet says so. Counts as user input (broadcast).
    pub fn send_snippet(&mut self, snippet: &Snippet, cx: &mut Context<Self>) {
        if self.exited.is_some() {
            return;
        }
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        let mut bytes = keys::paste_bytes(&snippet.text, bracketed);
        if snippet.run {
            bytes.push(b'\r');
        }
        self.shared.term.lock().selection = None;
        self.send_user(bytes, cx);
    }

    fn show_notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = Some(text.into());
        self.notice_gen += 1;
        let gen_now = self.notice_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(12))
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.notice_gen == gen_now {
                    v.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // ---- session log ---------------------------------------------------------

    pub fn start_log(
        &mut self,
        path: &Path,
        mode: LogMode,
        append: bool,
        header: &str,
        cx: &mut Context<Self>,
    ) -> std::io::Result<()> {
        let logger = SessionLogger::open(path, mode, append, header)?;
        if let Ok(mut l) = self.shared.log.lock() {
            *l = Some(logger);
        }
        cx.emit(TerminalEvent::LoggingChanged);
        cx.notify();
        Ok(())
    }

    pub fn stop_log(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut l) = self.shared.log.lock() {
            *l = None;
        }
        cx.emit(TerminalEvent::LoggingChanged);
        cx.notify();
    }

    pub fn log_path(&self) -> Option<PathBuf> {
        self.shared
            .log
            .lock()
            .ok()?
            .as_ref()
            .map(|l| l.path().to_owned())
    }

    pub fn clear_scrollback(&mut self, cx: &mut Context<Self>) {
        self.shared.term.lock().grid_mut().clear_history();
        if let Some(s) = self.search.as_mut() {
            s.focused = None;
        }
        self.refresh_search(cx);
        cx.notify();
    }

    fn select_all_visible(&mut self, cx: &mut Context<Self>) {
        let mut term = self.shared.term.lock();
        let (rows, cols) = (term.screen_lines(), term.columns());
        let start = GridPoint::new(Line(-(term.grid().history_size() as i32)), Column(0));
        let end = GridPoint::new(Line(rows as i32 - 1), Column(cols.saturating_sub(1)));
        let mut sel = Selection::new(SelectionType::Simple, start, Side::Left);
        sel.update(end, Side::Right);
        term.selection = Some(sel);
        drop(term);
        cx.notify();
    }

    /// Ask where to save, then start logging there.
    fn prompt_start_log(&mut self, cx: &mut Context<Self>) {
        let dir = Settings::get(cx).session_log_dir(&hostwick_store::AppPaths::detect());
        let _ = std::fs::create_dir_all(&dir);
        let name = format!(
            "{}-{}.log",
            sanitize(&self.title),
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        let mode = Settings::get(cx).session_log_mode;
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |v, cx| {
                let header = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                if let Err(e) = v.start_log(&path, mode, true, &header, cx) {
                    tracing::warn!(error = %e, path = %path.display(), "cannot start session log");
                }
            });
        })
        .detach();
    }

    fn mode(&self) -> TermMode {
        *self.shared.term.lock().mode()
    }

    /// Send bytes typed by the user here; also emitted for broadcast.
    fn send_user(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.scroll_to_bottom();
        self.write_raw(&bytes);
        cx.emit(TerminalEvent::UserInput(bytes));
    }

    /// Send bytes without emitting (used when replaying broadcast input).
    pub fn write_raw(&self, bytes: &[u8]) {
        if self.exited.is_none() {
            let _ = self
                .input
                .unbounded_send(ShellCommand::Data(Bytes::copy_from_slice(bytes)));
        }
    }

    fn scroll_to_bottom(&self) {
        let mut term = self.shared.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    /// Called from layout when the available area changes.
    pub(crate) fn set_size(&mut self, size: GridSize, layout: Layout) {
        self.layout = Some(layout);
        if size != self.size && size.cols > 1 && size.rows > 0 {
            self.size = size;
            self.shared.term.lock().resize(size);
            let _ = self.input.unbounded_send(ShellCommand::Resize {
                cols: u32::try_from(size.cols).unwrap_or(80),
                rows: u32::try_from(size.rows).unwrap_or(24),
            });
        }
    }

    pub fn copy_selection(&self, cx: &mut App) -> bool {
        match self.shared.term.lock().selection_to_string() {
            Some(text) if !text.is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                true
            }
            _ => false,
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
            self.send_user(keys::paste_bytes(&text, bracketed), cx);
        }
    }

    // ---- search ----------------------------------------------------------------

    /// Open (or focus) the scrollback search bar.
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &self.search {
            let input = s.input.clone();
            input.update(cx, |i, cx| i.focus(window, cx));
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in scrollback"));
        // Start with the selection, if it is one short line.
        if let Some(sel) = self.shared.term.lock().selection_to_string()
            && !sel.is_empty()
            && !sel.contains('\n')
            && sel.len() <= 200
        {
            input.update(cx, |i, cx| i.set_value(sel, window, cx));
        }
        let sub = cx.subscribe_in(&input, window, |view, _, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::Change => view.refresh_search(cx),
                InputEvent::PressEnter { shift, .. } => view.step_search(!*shift, cx),
                _ => {}
            }
            let _ = window;
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self.search = Some(SearchBar {
            input,
            case_sensitive: false,
            regex: false,
            compiled: None,
            error: None,
            focused: None,
            count: None,
            counting: false,
            generation: Arc::new(AtomicU64::new(0)),
            _sub: sub,
        });
        self.refresh_search(cx);
    }

    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.search.take() {
            s.generation.fetch_add(1, Ordering::AcqRel);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The query or options changed: recompile, find the newest match and
    /// start counting in the background.
    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let generation = s.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let query = s.input.read(cx).value().to_string();
        s.focused = None;
        s.count = None;
        s.counting = false;
        s.error = None;
        s.compiled = None;
        if query.is_empty() {
            cx.notify();
            return;
        }
        match search::compile(&query, s.case_sensitive, s.regex) {
            Ok(mut regex) => {
                let first = {
                    let term = self.shared.term.lock();
                    search::first(&term, &mut regex)
                };
                if let Some(m) = &first {
                    self.shared.term.lock().scroll_to_point(*m.start());
                }
                s.focused = first;
                s.counting = true;
                let (shared, gen_arc, notes) = (
                    self.shared.clone(),
                    s.generation.clone(),
                    self.notes.clone(),
                );
                let counter = regex.clone();
                self.rt.spawn_blocking(move || {
                    if let Some(c) = search::count(&shared.term, counter, &gen_arc, generation) {
                        let _ = notes.unbounded_send(Note::SearchCount(generation, c));
                    }
                });
                s.compiled = Some(regex);
            }
            Err(e) => s.error = Some(e),
        }
        cx.notify();
    }

    /// Next older (`up`) or newer match.
    fn step_search(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        let Some(regex) = s.compiled.as_mut() else {
            return;
        };
        let mut term = self.shared.term.lock();
        let next = match &s.focused {
            Some(m) => search::step(&term, regex, m, up),
            None => search::first(&term, regex),
        }
        .or_else(|| {
            s.count
                .as_ref()
                .and_then(|c| search::step_counted(&term, regex, c, s.focused.as_ref(), up))
        });
        if let Some(m) = next {
            term.scroll_to_point(*m.start());
            s.focused = Some(m);
        }
        drop(term);
        cx.notify();
    }

    fn toggle_search_option(&mut self, regex: bool, cx: &mut Context<Self>) {
        if let Some(s) = self.search.as_mut() {
            if regex {
                s.regex = !s.regex;
            } else {
                s.case_sensitive = !s.case_sensitive;
            }
        }
        self.refresh_search(cx);
    }

    /// Matches to paint this frame (visible rows only).
    pub(crate) fn search_paint(&mut self) -> Option<SearchPaint> {
        let s = self.search.as_mut()?;
        let regex = s.compiled.as_mut()?;
        let term = self.shared.term.lock();
        let matches = search::visible(&term, regex);
        Some(SearchPaint {
            matches,
            focused: s.focused.clone(),
        })
    }

    fn search_label(&self) -> String {
        let Some(s) = &self.search else {
            return String::new();
        };
        if let Some(e) = &s.error {
            return e.clone();
        }
        if s.compiled.is_none() {
            return String::new();
        }
        match (&s.count, &s.focused) {
            (Some(c), _) if c.starts.is_empty() => "No results".into(),
            (Some(c), Some(m)) => {
                let hist = self.shared.term.lock().history_size();
                let total = if c.capped {
                    format!("{}+", c.starts.len())
                } else {
                    c.starts.len().to_string()
                };
                match c.index_of(m, hist) {
                    Some(i) => format!("{i} of {total}"),
                    None => format!("? of {total}"),
                }
            }
            (Some(c), None) => format!("{} matches", c.starts.len()),
            (None, Some(_)) => "…".into(),
            (None, None) if s.counting => "…".into(),
            (None, None) => "No results".into(),
        }
    }

    // ---- keyboard ----------------------------------------------------------------

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let key = ks.key.as_str();
        let find = if cfg!(target_os = "macos") {
            m.platform && key == "f"
        } else {
            m.control && m.shift && key == "f"
        };
        if find {
            self.open_search(window, cx);
            cx.stop_propagation();
            return;
        }
        if key == "f3" && self.search.is_some() && !m.control && !m.alt {
            self.step_search(!m.shift, cx);
            cx.stop_propagation();
            return;
        }
        if self.exited.is_some() {
            // PuTTY-style: Enter on a dead session reconnects.
            if key == "enter" && !m.control && !m.alt && !m.platform {
                cx.emit(TerminalEvent::Reconnect);
                cx.stop_propagation();
            }
            return;
        }
        if m.control && matches!(key, "pause" | "break" | "cancel") && self.control.is_some() {
            self.send_break(cx);
            cx.stop_propagation();
            return;
        }
        let copy = (m.control && m.shift && key == "c") || (m.platform && key == "c");
        let paste = (m.control && m.shift && key == "v")
            || (m.platform && key == "v")
            || (m.shift && key == "insert");
        if copy {
            self.copy_selection(cx);
            cx.stop_propagation();
            return;
        }
        if paste {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }
        if m.shift && !m.control && (key == "pageup" || key == "pagedown") {
            let rows = self.size.rows as i32;
            self.shared
                .term
                .lock()
                .scroll_display(Scroll::Delta(if key == "pageup" { rows } else { -rows }));
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let mods = keys::Mods {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            platform: m.platform,
        };
        let app_cursor = self.mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = keys::encode(key, mods, app_cursor) {
            self.shared.term.lock().selection = None;
            self.send_user(bytes, cx);
            cx.stop_propagation();
        }
    }

    // ---- mouse -------------------------------------------------------------------

    /// Viewport cell under `pos` (column, row), clamped to the grid.
    fn cell_at(&self, pos: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let l = self.layout?;
        let x = (pos.x - l.origin.x).max(px(0.0));
        let y = (pos.y - l.origin.y).max(px(0.0));
        let col_f = x / l.cell_width;
        let col = (col_f.floor() as usize).min(self.size.cols.saturating_sub(1));
        let row = ((y / l.line_height).floor() as usize).min(self.size.rows.saturating_sub(1));
        let side = if col_f.fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        Some((col, row, side))
    }

    fn grid_point(&self, pos: Point<Pixels>) -> Option<(GridPoint, Side)> {
        let (col, row, side) = self.cell_at(pos)?;
        let offset = self.shared.term.lock().grid().display_offset() as i32;
        Some((GridPoint::new(Line(row as i32 - offset), Column(col)), side))
    }

    /// The terminal mode, if mouse events should go to the remote program
    /// now: it asked for them, the setting allows it, and Shift (which
    /// forces local selection, like xterm) is not held.
    fn reporting(&self, shift: bool, cx: &App) -> Option<TermMode> {
        if shift || self.exited.is_some() || !Settings::get(cx).mouse_reporting {
            return None;
        }
        let mode = self.mode();
        mode.intersects(TermMode::MOUSE_MODE).then_some(mode)
    }

    fn report_mouse(
        &mut self,
        mode: TermMode,
        kind: mouse::Kind,
        button: Option<mouse::Button>,
        m: &Modifiers,
        pos: Point<Pixels>,
    ) {
        let Some((col, row, _)) = self.cell_at(pos) else {
            return;
        };
        if kind == mouse::Kind::Motion && self.mouse.last_cell == Some((col, row)) {
            return;
        }
        self.mouse.last_cell = Some((col, row));
        if !mouse::wants(mode, kind, button) {
            return;
        }
        let mods = mouse::MouseMods {
            shift: m.shift,
            alt: m.alt,
            ctrl: m.control,
        };
        if let Some(bytes) = mouse::encode(mode, kind, button, mods, col, row) {
            self.write_raw(&bytes);
        }
    }

    fn mouse_button(b: MouseButton) -> Option<mouse::Button> {
        match b {
            MouseButton::Left => Some(mouse::Button::Left),
            MouseButton::Middle => Some(mouse::Button::Middle),
            MouseButton::Right => Some(mouse::Button::Right),
            _ => None,
        }
    }

    /// Press of any button while the program wants the mouse. True if
    /// reported (the caller then skips local handling).
    fn report_press(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(mode) = self.reporting(ev.modifiers.shift, cx) else {
            return false;
        };
        let Some(button) = Self::mouse_button(ev.button) else {
            return false;
        };
        self.mouse.pressed = Some(button);
        self.mouse.last_cell = None;
        self.report_mouse(
            mode,
            mouse::Kind::Press,
            Some(button),
            &ev.modifiers,
            ev.position,
        );
        true
    }

    fn report_release(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) -> bool {
        let Some(button) = Self::mouse_button(ev.button) else {
            return false;
        };
        if self.mouse.pressed != Some(button) {
            return false;
        }
        self.mouse.pressed = None;
        let Some(mode) = self.reporting(false, cx) else {
            return true;
        };
        self.mouse.last_cell = None;
        self.report_mouse(
            mode,
            mouse::Kind::Release,
            Some(button),
            &ev.modifiers,
            ev.position,
        );
        true
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.report_press(ev, cx) {
            self.shared.term.lock().selection = None;
            cx.notify();
            return;
        }
        if let Some((point, side)) = self.grid_point(ev.position) {
            let kind = match ev.click_count {
                2 => SelectionType::Semantic,
                n if n >= 3 => SelectionType::Lines,
                _ => SelectionType::Simple,
            };
            self.shared.term.lock().selection = Some(Selection::new(kind, point, side));
            self.selecting = true;
            cx.notify();
        }
    }

    fn on_middle_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.report_press(ev, cx) {
            cx.stop_propagation();
        }
    }

    fn on_any_mouse_up(&mut self, ev: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.report_release(ev, cx);
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(held) = self.mouse.pressed {
            // Released outside the window: finish the press.
            let still = ev.pressed_button.and_then(Self::mouse_button) == Some(held);
            if !still {
                self.mouse.pressed = None;
                if let Some(mode) = self.reporting(false, cx) {
                    self.mouse.last_cell = None;
                    self.report_mouse(
                        mode,
                        mouse::Kind::Release,
                        Some(held),
                        &ev.modifiers,
                        ev.position,
                    );
                }
                return;
            }
        }
        if let Some(mode) = self.reporting(ev.modifiers.shift, cx)
            && !self.selecting
        {
            let button = self.mouse.pressed;
            self.report_mouse(
                mode,
                mouse::Kind::Motion,
                button,
                &ev.modifiers,
                ev.position,
            );
            return;
        }
        if !self.selecting || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some((point, side)) = self.grid_point(ev.position) {
            if let Some(sel) = self.shared.term.lock().selection.as_mut() {
                sel.update(point, side);
            }
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, ev: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.report_release(ev, cx) {
            return;
        }
        if self.selecting {
            self.selecting = false;
            // PuTTY habit: selecting copies (configurable).
            if Settings::get(cx).copy_on_select {
                self.copy_selection(cx);
            }
        }
    }

    fn on_right_click(&mut self, ev: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.report_press(ev, cx) {
            // The program gets the click; no menu (Shift+right-click for it).
            cx.stop_propagation();
            return;
        }
        // PuTTY mode: right-click pastes. Menu mode is handled by the context menu.
        if Settings::get(cx).right_click == RightClick::Paste {
            self.paste(cx);
        }
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self.layout.map(|l| l.line_height).unwrap_or(px(17.0));
        let delta = ev.delta.pixel_delta(line_height);
        let lines = (delta.y / line_height).round() as i32;
        let cols = (delta.x / line_height).round() as i32;
        if lines == 0 && cols == 0 {
            return;
        }
        if let Some(mode) = self.reporting(ev.modifiers.shift, cx) {
            let (button, n) = if lines != 0 {
                let b = if lines > 0 {
                    mouse::Button::WheelUp
                } else {
                    mouse::Button::WheelDown
                };
                (b, lines.unsigned_abs())
            } else {
                let b = if cols > 0 {
                    mouse::Button::WheelLeft
                } else {
                    mouse::Button::WheelRight
                };
                (b, cols.unsigned_abs())
            };
            for _ in 0..n.min(10) {
                self.mouse.last_cell = None;
                self.report_mouse(
                    mode,
                    mouse::Kind::Press,
                    Some(button),
                    &ev.modifiers,
                    ev.position,
                );
            }
            return;
        }
        if lines == 0 {
            return;
        }
        let mode = self.mode();
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !ev.modifiers.shift {
            // Full-screen apps (less, man) without mouse support: arrows.
            let app_cursor = mode.contains(TermMode::APP_CURSOR);
            let key = if lines > 0 { "up" } else { "down" };
            for _ in 0..lines.unsigned_abs().min(10) {
                if let Some(b) = keys::encode(key, keys::Mods::default(), app_cursor) {
                    self.write_raw(&b);
                }
            }
        } else {
            self.shared.term.lock().scroll_display(Scroll::Delta(lines));
            cx.notify();
        }
    }

    // ---- rendering -------------------------------------------------------------

    fn render_search_bar(
        &mut self,
        below_badge: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = self.search_label();
        let s = self.search.as_ref()?;
        let bad = s.error.is_some();
        Some(
            div()
                .id("search-bar")
                .occlude()
                .absolute()
                .top(px(if below_badge { 30.0 } else { 6.0 }))
                .right(px(16.0))
                .w(px(420.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(6.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .bg(hsla(0.0, 0.0, 0.16, 0.97))
                .border_1()
                .border_color(hsla(0.0, 0.0, 0.35, 1.0))
                .shadow_md()
                .text_color(hsla(0.0, 0.0, 0.9, 1.0))
                .capture_key_down(cx.listener(|v, ev: &KeyDownEvent, window, cx| {
                    let k = ev.keystroke.key.as_str();
                    let m = ev.keystroke.modifiers;
                    if k == "escape" {
                        v.close_search(window, cx);
                        cx.stop_propagation();
                    } else if k == "f3" {
                        v.step_search(!m.shift, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&s.input).small().cleanable(false)),
                )
                .child(
                    Button::new("case")
                        .xsmall()
                        .compact()
                        .label("Aa")
                        .selected(s.case_sensitive)
                        .when(s.case_sensitive, |b| b.primary())
                        .tooltip("Match case")
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_search_option(false, cx))),
                )
                .child(
                    Button::new("regex")
                        .xsmall()
                        .compact()
                        .label(".*")
                        .selected(s.regex)
                        .when(s.regex, |b| b.primary())
                        .tooltip("Regular expression")
                        .on_click(cx.listener(|v, _, _, cx| v.toggle_search_option(true, cx))),
                )
                .child(
                    div()
                        .min_w(px(78.0))
                        .text_size(px(12.0))
                        .when(bad, |d| d.text_color(hsla(0.0, 0.8, 0.65, 1.0)))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(label),
                )
                .child(
                    Button::new("older")
                        .xsmall()
                        .ghost()
                        .icon(IconName::ChevronUp)
                        .tooltip("Previous (older) match — Enter, F3")
                        .on_click(cx.listener(|v, _, _, cx| v.step_search(true, cx))),
                )
                .child(
                    Button::new("newer")
                        .xsmall()
                        .ghost()
                        .icon(IconName::ChevronDown)
                        .tooltip("Next (newer) match — Shift+Enter, Shift+F3")
                        .on_click(cx.listener(|v, _, _, cx| v.step_search(false, cx))),
                )
                .child(
                    Button::new("close-search")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .tooltip("Close (Esc)")
                        .on_click(cx.listener(|v, _, window, cx| v.close_search(window, cx))),
                )
                .into_any_element(),
        )
    }

    fn render_button_bar(&self, snippets: &[Snippet], cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("button-bar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.0))
            .h(px(26.0))
            .px(px(6.0))
            .overflow_x_scroll()
            .bg(hsla(0.0, 0.0, 0.13, 1.0))
            .border_b_1()
            .border_color(hsla(0.0, 0.0, 0.25, 1.0))
            .children(snippets.iter().enumerate().map(|(i, snip)| {
                let s = snip.clone();
                let mut tip: String = snip.text.chars().take(300).collect();
                if snip.text.chars().count() > 300 {
                    tip.push('…');
                }
                if snip.run {
                    tip.push_str("  ⏎");
                }
                Button::new(("snippet", i))
                    .xsmall()
                    .ghost()
                    .label(if snip.name.trim().is_empty() {
                        snip.text.chars().take(20).collect::<String>()
                    } else {
                        snip.name.clone()
                    })
                    .tooltip(tip)
                    .on_click(cx.listener(move |v, _, window, cx| {
                        v.send_snippet(&s, cx);
                        window.focus(&v.focus, cx);
                    }))
            }))
            .into_any_element()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = Settings::get(cx);
        let menu_mode = settings.right_click == RightClick::Menu;
        let show_bar = settings.show_button_bar;
        let exited = self.exited.clone();
        let logging = self.log_path().is_some();
        let alive = self.exited.is_none();
        let can_break = self.can_send_break();
        let is_ssh = self.transport() == TransportKind::Ssh;
        let cwd = self.cwd();
        let script_running = self.is_script_running();
        let snippets = self.snippets(cx);
        let notice = self.notice.clone();
        let view = cx.entity().downgrade();
        let body = div()
            .id("terminal")
            .size_full()
            .relative()
            .bg(colors::to_hsla(colors::BACKGROUND))
            .track_focus(&self.focus)
            .key_context("Terminal")
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_any_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_any_mouse_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .p(px(6.0))
            .child(TerminalElement::new(cx.entity()))
            .when(logging, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(6.0))
                        .right(px(12.0))
                        .px(px(8.0))
                        .rounded(px(4.0))
                        .bg(hsla(0.0, 0.7, 0.35, 0.85))
                        .text_color(hsla(0.0, 0.0, 1.0, 1.0))
                        .text_size(px(11.0))
                        .child("● LOG"),
                )
            })
            .when_some(notice, |d, text| {
                d.child(
                    div()
                        .id("notice")
                        .absolute()
                        .bottom(px(36.0))
                        .left(px(12.0))
                        .max_w(px(640.0))
                        .flex()
                        .gap(px(8.0))
                        .items_center()
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(4.0))
                        .bg(hsla(0.11, 0.6, 0.25, 0.95))
                        .text_color(hsla(0.0, 0.0, 0.95, 1.0))
                        .text_size(px(12.0))
                        .child(div().whitespace_normal().child(text))
                        .child(
                            div()
                                .id("dismiss-notice")
                                .cursor_pointer()
                                .px(px(4.0))
                                .child("✕")
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.notice = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(exited, |d, reason| {
                d.child(
                    div()
                        .id("ended")
                        .absolute()
                        .bottom(px(8.0))
                        .right(px(12.0))
                        .flex()
                        .gap(px(10.0))
                        .items_center()
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(4.0))
                        .bg(hsla(0.0, 0.0, 0.2, 0.9))
                        .text_color(hsla(0.0, 0.0, 0.9, 1.0))
                        .text_size(px(12.0))
                        .child(format!("Session ended — {reason}. Press Enter to"))
                        .child(
                            div()
                                .id("reconnect")
                                .px(px(8.0))
                                .rounded(px(3.0))
                                .bg(hsla(0.58, 0.6, 0.45, 1.0))
                                .cursor_pointer()
                                .child("Reconnect")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(TerminalEvent::Reconnect)),
                                ),
                        ),
                )
            });
        let body = if menu_mode {
            let menu_snippets = snippets.clone();
            body.context_menu(move |menu, window, cx| {
                let v = view.clone();
                let act = move |f: fn(&mut TerminalView, &mut Context<TerminalView>)| {
                    let v = v.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = v.update(cx, f);
                    }
                };
                let find_view = view.clone();
                let mut menu = menu
                    .item(PopupMenuItem::new("Copy").on_click(act(|t, cx| {
                        t.copy_selection(cx);
                    })))
                    .item(PopupMenuItem::new("Paste").on_click(act(|t, cx| t.paste(cx))))
                    .item(
                        PopupMenuItem::new("Select all")
                            .on_click(act(|t, cx| t.select_all_visible(cx))),
                    )
                    .item(PopupMenuItem::new("Find…").on_click(
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            let _ = find_view.update(cx, |t, cx| t.open_search(window, cx));
                        },
                    ));
                if !menu_snippets.is_empty() {
                    let snips = menu_snippets.clone();
                    let sv = view.clone();
                    menu = menu.submenu("Snippets", window, cx, move |mut sub, _, _| {
                        for s in &snips {
                            let (s, sv) = (s.clone(), sv.clone());
                            let label = if s.name.trim().is_empty() {
                                s.text.chars().take(30).collect::<String>()
                            } else {
                                s.name.clone()
                            };
                            sub = sub.item(PopupMenuItem::new(label).on_click(
                                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                    let _ = sv.update(cx, |t, cx| t.send_snippet(&s, cx));
                                },
                            ));
                        }
                        sub
                    });
                }
                if let Some(dir) = cwd.clone() {
                    menu = menu.item(PopupMenuItem::new("Copy current folder").on_click(
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            cx.write_to_clipboard(ClipboardItem::new_string(dir.clone()));
                        },
                    ));
                }
                if can_break {
                    menu = menu.item(
                        PopupMenuItem::new("Send break").on_click(act(|t, cx| t.send_break(cx))),
                    );
                }
                if script_running {
                    menu = menu.item(
                        PopupMenuItem::new("Stop script")
                            .on_click(act(|t, cx| t.cancel_script(cx))),
                    );
                }
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("Clear scrollback")
                            .on_click(act(|t, cx| t.clear_scrollback(cx))),
                    )
                    .item(if logging {
                        PopupMenuItem::new("Stop session log").on_click(act(|t, cx| t.stop_log(cx)))
                    } else {
                        PopupMenuItem::new("Log session to file…")
                            .on_click(act(|t, cx| t.prompt_start_log(cx)))
                    })
                    .separator();
                if is_ssh {
                    menu = menu.item(
                        PopupMenuItem::new("Open files (SFTP)")
                            .on_click(act(|_, cx| cx.emit(TerminalEvent::OpenFiles))),
                    );
                }
                menu.item(
                    PopupMenuItem::new("Duplicate session")
                        .on_click(act(|_, cx| cx.emit(TerminalEvent::Duplicate))),
                )
                .item(
                    PopupMenuItem::new(if alive {
                        "Reconnect"
                    } else {
                        "Reconnect (session ended)"
                    })
                    .on_click(act(|_, cx| cx.emit(TerminalEvent::Reconnect))),
                )
            })
            .into_any_element()
        } else {
            body.into_any_element()
        };
        let search_bar = self.render_search_bar(logging, cx);
        let bar = (show_bar && !snippets.is_empty()).then(|| self.render_button_bar(&snippets, cx));
        div()
            .id("terminal-root")
            .size_full()
            .flex()
            .flex_col()
            .children(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(body)
                    .children(search_bar),
            )
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    /// Committed text from the keyboard or an input method.
    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !text.is_empty() {
            self.shared.term.lock().selection = None;
            self.send_user(text.as_bytes().to_vec(), cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        // Composition in progress; the IME shows its own preedit window.
    }

    /// Where the IME candidate window should appear: at the cursor.
    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let l = self.layout?;
        let cursor = self.shared.term.lock().grid().cursor.point;
        let x = element_bounds.origin.x + l.cell_width * cursor.column.0 as f32;
        let y = element_bounds.origin.y + l.line_height * cursor.line.0.max(0) as f32;
        Some(Bounds::new(point(x, y), size(l.cell_width, l.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

pub fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if t.is_empty() { "session".into() } else { t }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would bring in GPUI's `test` attribute.
    use super::tab_title;
    use hostwick_core::{HostProfile, Protocol};

    #[test]
    fn tab_titles_name_the_transport() {
        let mut p = HostProfile::new("router", "10.0.0.1");
        assert_eq!(tab_title(&p), "router");
        p.protocol = Protocol::Telnet;
        assert_eq!(tab_title(&p), "router (telnet)");
        p.protocol = Protocol::Serial(hostwick_core::SerialSettings {
            baud: 115_200,
            ..Default::default()
        });
        p.host = "/dev/ttyUSB0".into();
        assert_eq!(tab_title(&p), "router (/dev/ttyUSB0 115200 8N1)");
        p.name = "/dev/ttyUSB0".into();
        assert_eq!(tab_title(&p), "/dev/ttyUSB0 115200 8N1");
    }
}
