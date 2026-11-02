//! Terminal tab: an SSH shell channel driving an Alacritty terminal grid,
//! painted with GPUI.
//!
//! Threading (the "no hangs" part):
//! - SSH output is parsed into the grid on a Tokio task, never on the UI
//!   thread. The grid lives behind Alacritty's `FairMutex`, held only briefly
//!   by the parser (per batch) and by the painter (per frame).
//! - The parser wakes the UI at most once per batch via a coalescing flag, so
//!   a flood of output (`cat` of a large file) costs one repaint per frame,
//!   not one per packet.
//! - Keystrokes go out through an unbounded, ordered queue, so typing never
//!   blocks on the network.

pub mod colors;
mod element;
pub mod keys;
mod sessionlog;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use bytes::Bytes;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::LogMode;
use hostwick_ssh::{ShellCommand, ShellEvent, ShellSession};

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
}

#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Bytes the user typed or pasted (used for broadcast input).
    UserInput(Vec<u8>),
    TitleChanged,
    Exited,
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

pub struct TerminalView {
    shared: Arc<Shared>,
    input: UnboundedSender<ShellCommand>,
    focus: FocusHandle,
    pub title: SharedString,
    pub exited: Option<String>,
    size: GridSize,
    layout: Option<Layout>,
    selecting: bool,
    pub(crate) font: Font,
    pub(crate) font_size: Pixels,
    _pump: Task<()>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
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

impl TerminalView {
    /// Wire a freshly opened shell into a new terminal view.
    pub fn new(
        title: impl Into<SharedString>,
        shell: ShellSession,
        initial: GridSize,
        rt: &tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (shell_in, mut shell_out) = shell.split();
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
        });

        // Ordered input pump: UI queue → SSH channel (with backpressure there).
        rt.spawn(async move {
            while let Some(cmd) = input_rx.next().await {
                if shell_in.send(cmd).await.is_err() {
                    break;
                }
            }
        });

        // Parser: SSH output → grid, off the UI thread.
        let parser_shared = shared.clone();
        rt.spawn(async move {
            let mut processor: Processor = Processor::new();
            let mut exit: Option<String> = None;
            let wake = |s: &Shared| {
                if !s.dirty.swap(true, Ordering::AcqRel) {
                    let _ = notes_tx.unbounded_send(Note::Wakeup);
                }
            };
            'outer: while let Some(ev) = shell_out.recv().await {
                let mut pending = Some(ev);
                {
                    let mut term = parser_shared.term.lock();
                    // Drain everything already queued under one lock.
                    while let Some(ev) = pending.take() {
                        match ev {
                            ShellEvent::Stdout(b) | ShellEvent::Stderr(b) => {
                                processor.advance(&mut *term, &b);
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
                wake(&parser_shared);
            }
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
                cx.emit(TerminalEvent::Exited);
                cx.notify();
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.exited.is_none()
    }

    /// Re-read font and scrollback after the settings changed.
    pub fn apply_settings(&mut self, s: &Settings, cx: &mut Context<Self>) {
        self.font.family = if s.font_family.trim().is_empty() {
            monospace_family()
        } else {
            s.font_family.clone().into()
        };
        self.font_size = px(s.font_size);
        self.shared.term.lock().set_options(Config {
            scrolling_history: s.scrollback_lines,
            ..Config::default()
        });
        cx.notify();
    }

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

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let key = ks.key.as_str();
        if self.exited.is_some() {
            // PuTTY-style: Enter on a dead session reconnects.
            if key == "enter" && !m.control && !m.alt && !m.platform {
                cx.emit(TerminalEvent::Reconnect);
                cx.stop_propagation();
            }
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

    fn grid_point(&self, pos: Point<Pixels>) -> Option<(GridPoint, Side)> {
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
        let offset = self.shared.term.lock().grid().display_offset() as i32;
        Some((GridPoint::new(Line(row as i32 - offset), Column(col)), side))
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
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

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.selecting = false;
            // PuTTY habit: selecting copies (configurable).
            if Settings::get(cx).copy_on_select {
                self.copy_selection(cx);
            }
        }
    }

    fn on_right_click(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // PuTTY mode: right-click pastes. Menu mode is handled by the context menu.
        if Settings::get(cx).right_click == RightClick::Paste {
            self.paste(cx);
        }
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = self.layout.map(|l| l.line_height).unwrap_or(px(17.0));
        let lines = (ev.delta.pixel_delta(line_height).y / line_height).round() as i32;
        if lines == 0 {
            return;
        }
        let mode = self.mode();
        if mode.contains(TermMode::ALT_SCREEN) && !mode.intersects(TermMode::MOUSE_MODE) {
            // Full-screen apps (less, vim) without mouse support: send arrows.
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
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let exited = self.exited.clone();
        let menu_mode = Settings::get(cx).right_click == RightClick::Menu;
        let logging = self.log_path().is_some();
        let alive = self.exited.is_none();
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
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
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
        if !menu_mode {
            return body.into_any_element();
        }
        body.context_menu(move |menu, _, _| {
            let v = view.clone();
            let act = move |f: fn(&mut TerminalView, &mut Context<TerminalView>)| {
                let v = v.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = v.update(cx, f);
                }
            };
            menu.item(PopupMenuItem::new("Copy").on_click(act(|t, cx| {
                t.copy_selection(cx);
            })))
            .item(PopupMenuItem::new("Paste").on_click(act(|t, cx| t.paste(cx))))
            .item(PopupMenuItem::new("Select all").on_click(act(|t, cx| t.select_all_visible(cx))))
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
            .separator()
            .item(
                PopupMenuItem::new("Open files (SFTP)")
                    .on_click(act(|_, cx| cx.emit(TerminalEvent::OpenFiles))),
            )
            .item(
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
