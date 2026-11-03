//! The command palette (Ctrl+Shift+P / Ctrl+Shift+K, ⌘K / ⌘⇧P): one search
//! box over saved hosts, recent connections, `~/.ssh/config` aliases not
//! saved yet, a typed `user@host:port`, every app action, snippets and
//! open tabs. Fuzzy matched, ranked by how often and how recently things
//! were used.

use std::rc::Rc;

use gpui_kit::accesskit::Role;
use gpui_kit::component::input::{
    Enter, Input, InputEvent, InputState, MoveDown, MovePageDown, MovePageUp, MoveUp,
};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HostProfile, ProfileId, Snippet};

use super::{HostwickApp, PaneBody, TabKind};
use crate::fuzzy::{Frecency, Hit, Prepared, Query, byte_ranges, match_fields};
use crate::hostops::Recent;
use crate::notify::{Toast, ToastExt as _};
use crate::settingsform::Section;
use crate::shortcuts::Sc;

/// Most results shown at once.
const MAX_RESULTS: usize = 50;
const ROW_H: f32 = 30.0;

/// How Enter was pressed (or the row clicked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mods {
    /// Enter: open a terminal / run.
    Plain,
    /// Ctrl/⌘+Enter: open files.
    Secondary,
    /// Shift+Enter: open in a new split.
    Shift,
    /// Alt+Enter: edit (or save).
    Alt,
}

impl Mods {
    pub(crate) fn from_modifiers(m: &Modifiers) -> Self {
        if m.alt {
            Mods::Alt
        } else if secondary(m) {
            Mods::Secondary
        } else if m.shift {
            Mods::Shift
        } else {
            Mods::Plain
        }
    }
}

/// Ctrl on Windows/Linux, ⌘ on macOS.
pub(crate) fn secondary(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    }
}

/// App actions the palette offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    NewHost,
    NewFolder,
    QuickConnect,
    SplitRight,
    SplitDown,
    ClosePane,
    Reconnect,
    DuplicateTab,
    NextTab,
    PrevTab,
    Broadcast,
    Find,
    FilesHere,
    OpenFiles,
    ReadOutput,
    TextView,
    Settings(Section),
    GenerateKey,
    InspectKey,
    KnownHosts,
    LockVault,
    ScanComputer,
    ImportSshConfig,
    ImportPuttyRegistry,
    ImportWinscpRegistry,
    ImportPuttyFile,
    ImportWinscpFile,
    ImportSolar,
    ToggleTheme,
    Shortcuts,
}

impl Act {
    fn all() -> Vec<Act> {
        let mut v = vec![
            Act::NewHost,
            Act::NewFolder,
            Act::QuickConnect,
            Act::SplitRight,
            Act::SplitDown,
            Act::ClosePane,
            Act::Reconnect,
            Act::DuplicateTab,
            Act::NextTab,
            Act::PrevTab,
            Act::Broadcast,
            Act::Find,
            Act::FilesHere,
            Act::OpenFiles,
            Act::ReadOutput,
            Act::TextView,
        ];
        v.extend(Section::ALL.iter().map(|s| Act::Settings(*s)));
        v.extend([
            Act::GenerateKey,
            Act::InspectKey,
            Act::KnownHosts,
            Act::LockVault,
            Act::ScanComputer,
            Act::ImportSshConfig,
        ]);
        if cfg!(windows) {
            v.extend([Act::ImportPuttyRegistry, Act::ImportWinscpRegistry]);
        }
        v.extend([
            Act::ImportPuttyFile,
            Act::ImportWinscpFile,
            Act::ImportSolar,
            Act::ToggleTheme,
            Act::Shortcuts,
        ]);
        v
    }

    fn id(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    fn label(self) -> SharedString {
        match self {
            Act::NewHost => t!("keys-new-host"),
            Act::NewFolder => t!("act-new-folder"),
            Act::QuickConnect => t!("act-quick-connect"),
            Act::SplitRight => t!("menu-split-right"),
            Act::SplitDown => t!("menu-split-down"),
            Act::ClosePane => t!("menu-close-pane"),
            Act::Reconnect => t!("menu-reconnect"),
            Act::DuplicateTab => t!("keys-duplicate-tab"),
            Act::NextTab => t!("keys-next-tab"),
            Act::PrevTab => t!("keys-prev-tab"),
            Act::Broadcast => t!("act-broadcast"),
            Act::Find => t!("keys-find"),
            Act::FilesHere => t!("term-menu-files-here"),
            Act::OpenFiles => t!("menu-open-files"),
            Act::ReadOutput => t!("keys-read-output"),
            Act::TextView => t!("keys-text-view"),
            Act::Settings(s) => t!("act-settings-section", section = s.label()),
            Act::GenerateKey => t!("tools-generate-key"),
            Act::InspectKey => t!("tools-inspect-key"),
            Act::KnownHosts => t!("tools-known-hosts"),
            Act::LockVault => t!("sidebar-vault-lock"),
            Act::ScanComputer => t!("import-menu-scan"),
            Act::ImportSshConfig => t!("import-menu-ssh-config"),
            Act::ImportPuttyRegistry => t!("import-menu-putty-registry"),
            Act::ImportWinscpRegistry => t!("import-menu-winscp-registry"),
            Act::ImportPuttyFile => t!("import-menu-putty-file"),
            Act::ImportWinscpFile => t!("import-menu-winscp-file"),
            Act::ImportSolar => t!("import-menu-solar-file"),
            Act::ToggleTheme => t!("act-toggle-theme"),
            Act::Shortcuts => t!("keys-shortcuts"),
        }
    }

    /// The category, shown after the label and matched too.
    fn detail(self) -> SharedString {
        match self {
            Act::Settings(_) => t!("settings-title"),
            Act::GenerateKey | Act::InspectKey | Act::KnownHosts | Act::LockVault => {
                t!("sidebar-tools")
            }
            Act::ScanComputer
            | Act::ImportSshConfig
            | Act::ImportPuttyRegistry
            | Act::ImportWinscpRegistry
            | Act::ImportPuttyFile
            | Act::ImportWinscpFile
            | Act::ImportSolar => t!("sidebar-import"),
            Act::SplitRight
            | Act::SplitDown
            | Act::ClosePane
            | Act::Reconnect
            | Act::DuplicateTab
            | Act::NextTab
            | Act::PrevTab => t!("keys-group-tabs"),
            Act::Find | Act::FilesHere | Act::ReadOutput | Act::TextView => {
                t!("keys-group-terminal")
            }
            _ => t!("keys-group-app"),
        }
    }

    fn shortcut(self) -> Option<&'static str> {
        let sc = match self {
            Act::NewHost => Sc::NewHost,
            Act::SplitRight => Sc::SplitRight,
            Act::SplitDown => Sc::SplitDown,
            Act::ClosePane => Sc::ClosePane,
            Act::Reconnect => Sc::Reconnect,
            Act::DuplicateTab => Sc::DuplicateTab,
            Act::NextTab => Sc::NextTab,
            Act::PrevTab => Sc::PrevTab,
            Act::Broadcast => Sc::Broadcast,
            Act::Find => Sc::Find,
            Act::ReadOutput => Sc::ReadOutput,
            Act::TextView => Sc::TextView,
            Act::Settings(Section::Appearance) => Sc::Settings,
            Act::Shortcuts => Sc::Shortcuts,
            _ => return None,
        };
        Some(sc.hint())
    }
}

/// What a result does.
#[derive(Clone)]
pub(crate) enum Choice {
    Host(ProfileId),
    /// A typed or recent `user@host:port`.
    Quick(String),
    /// Save a typed `user@host:port` as a host.
    SaveQuick(String),
    /// A `~/.ssh/config` alias not saved yet.
    Alias(Rc<HostProfile>),
    Act(Act),
    Snippet(Snippet),
    Tab(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Connect,
    Recent,
    Hosts,
    SshConfig,
    Tabs,
    Actions,
    Snippets,
}

impl Group {
    fn label(self) -> SharedString {
        match self {
            Group::Connect => t!("palette-group-connect"),
            Group::Recent => t!("sidebar-section-recent"),
            Group::Hosts => t!("sidebar-hosts-label"),
            Group::SshConfig => t!("palette-group-ssh-config"),
            Group::Tabs => t!("palette-group-tabs"),
            Group::Actions => t!("palette-group-actions"),
            Group::Snippets => t!("palette-group-snippets"),
        }
    }
}

/// One searchable entry.
struct Cand {
    choice: Choice,
    group: Group,
    label: SharedString,
    label_p: Prepared,
    detail: SharedString,
    detail_p: Prepared,
    keys: Option<&'static str>,
    /// Frecency key.
    key: String,
    /// Shown when the query is empty.
    idle: bool,
}

impl Cand {
    fn new(
        choice: Choice,
        group: Group,
        label: SharedString,
        detail: SharedString,
        key: String,
    ) -> Self {
        Self {
            label_p: Prepared::new(&label),
            detail_p: Prepared::new(&detail),
            choice,
            group,
            label,
            detail,
            keys: None,
            key,
            idle: false,
        }
    }
}

/// A row of the result list.
#[derive(Clone)]
enum Row {
    Header(Group),
    Item {
        /// Index into `cands`, or None for the dynamic connect rows.
        cand: Option<usize>,
        choice: Choice,
        label: SharedString,
        detail: SharedString,
        keys: Option<&'static str>,
        hit: Hit,
    },
}

pub(crate) struct PaletteView {
    app: WeakEntity<HostwickApp>,
    input: Entity<InputState>,
    cands: Vec<Cand>,
    rows: Rc<Vec<Row>>,
    /// Index into `rows` (always an item when there is one).
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    frecency: Frecency,
    now: u64,
    _subs: Vec<Subscription>,
}

impl PaletteView {
    fn new(
        app: WeakEntity<HostwickApp>,
        cands: Vec<Cand>,
        frecency: Frecency,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t!("palette-placeholder")));
        let subs = vec![
            cx.subscribe(&input, |v: &mut Self, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    v.refilter(cx);
                }
            }),
        ];
        let mut v = Self {
            app,
            input,
            cands,
            rows: Rc::default(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            frecency,
            now: crate::fuzzy::now_secs(),
            _subs: subs,
        };
        v.refilter(cx);
        v
    }

    /// More candidates arrived (`~/.ssh/config` was read).
    fn add_cands(&mut self, more: Vec<Cand>, cx: &mut Context<Self>) {
        self.cands.extend(more);
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_owned();
        let q = Query::new(&text);
        let mut hits: Vec<(i32, usize, Hit)> = Vec::new();
        for (i, c) in self.cands.iter().enumerate() {
            if q.is_empty() {
                if c.idle {
                    let boost = self.frecency.boost(&c.key, self.now);
                    // Keep the given order among equals (recents are newest first).
                    hits.push((boost * 1000 - i as i32, i, Hit::default()));
                }
                continue;
            }
            if let Some(h) = match_fields(&q, &c.label_p, &c.detail_p) {
                let score = h.score + self.frecency.boost(&c.key, self.now);
                hits.push((score, i, h));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        hits.truncate(MAX_RESULTS);

        // Groups ordered by their best result; the typed address first when
        // it is obviously one (has @ or :), else after the hosts.
        let mut groups: Vec<(Group, Vec<Row>)> = Vec::new();
        for (_, i, hit) in hits {
            let c = &self.cands[i];
            let row = Row::Item {
                cand: Some(i),
                choice: c.choice.clone(),
                label: c.label.clone(),
                detail: c.detail.clone(),
                keys: c.keys,
                hit,
            };
            match groups.iter_mut().find(|(g, _)| *g == c.group) {
                Some((_, v)) => v.push(row),
                None => groups.push((c.group, vec![row])),
            }
        }
        if q.is_empty() {
            groups.sort_by_key(|(g, _)| *g);
        }
        if let Some(target) = typed_target(&text) {
            let rows = vec![
                Row::Item {
                    cand: None,
                    choice: Choice::Quick(target.clone()),
                    label: t!("palette-connect-to", target = target.clone()),
                    detail: SharedString::default(),
                    keys: None,
                    hit: Hit::default(),
                },
                Row::Item {
                    cand: None,
                    choice: Choice::SaveQuick(target.clone()),
                    label: t!("palette-save-as-host", target = target.clone()),
                    detail: SharedString::default(),
                    keys: None,
                    hit: Hit::default(),
                },
            ];
            // Obviously an address (user@, :port, a dotted name): first.
            // A bare word: after whatever else matched.
            let explicit = text.contains(['@', ':', '.']) || groups.is_empty();
            let at = if explicit { 0 } else { groups.len() };
            groups.insert(at, (Group::Connect, rows));
        }
        let mut rows = Vec::new();
        for (g, items) in groups {
            rows.push(Row::Header(g));
            rows.extend(items);
        }
        self.selected = rows.iter().position(|r| matches!(r, Row::Item { .. }));
        self.rows = Rc::new(rows);
        if let Some(s) = self.selected {
            self.scroll.scroll_to_item(s, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        let items: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, Row::Item { .. }))
            .map(|(i, _)| i)
            .collect();
        if items.is_empty() {
            return;
        }
        let cur = self
            .selected
            .and_then(|s| items.iter().position(|i| *i == s))
            .unwrap_or(0) as isize;
        let n = items.len() as isize;
        let next = if delta.abs() == 1 {
            (cur + delta).rem_euclid(n)
        } else {
            (cur + delta).clamp(0, n - 1)
        };
        let row = items[next as usize];
        self.selected = Some(row);
        // Keep the group header visible when moving to a group's first item.
        let show = if row > 0 && matches!(self.rows[row - 1], Row::Header(_)) {
            row - 1
        } else {
            row
        };
        self.scroll.scroll_to_item(show, ScrollStrategy::Nearest);
        self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn confirm(
        &mut self,
        row: Option<usize>,
        mods: Mods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Row::Item { choice, cand, .. }) = row
            .or(self.selected)
            .and_then(|r| self.rows.get(r))
            .cloned()
        else {
            return;
        };
        let key = cand
            .and_then(|i| self.cands.get(i))
            .map(|c| c.key.clone())
            .or_else(|| match &choice {
                Choice::Quick(t) => Some(format!("quick:{t}")),
                _ => None,
            });
        window.close_dialog(cx);
        let app = self.app.clone();
        // After the dialog is gone, so what runs can open its own.
        window.defer(cx, move |window, cx| {
            let _ = app.update(cx, |a, cx| {
                if let Some(k) = key {
                    a.note_use(&k);
                }
                a.run_choice(choice, mods, window, cx);
            });
        });
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        match &self.rows[ix] {
            Row::Header(g) => div()
                .id(("pal-h", ix))
                .h(px(ROW_H))
                .px(px(12.0))
                .pt(px(8.0))
                .flex()
                .items_center()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(crate::a11y::text(g.label()))
                .into_any_element(),
            Row::Item {
                label,
                detail,
                keys,
                hit,
                choice,
                ..
            } => {
                let selected = self.selected == Some(ix);
                let hl = HighlightStyle {
                    color: Some(theme.link),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                };
                let label_text = StyledText::new(label.clone())
                    .with_highlights(byte_ranges(label, &hit.label).into_iter().map(|r| (r, hl)));
                let detail_text = StyledText::new(detail.clone()).with_highlights(
                    byte_ranges(detail, &hit.detail)
                        .into_iter()
                        .map(|r| (r, hl)),
                );
                let icon = match choice {
                    Choice::Host(_) | Choice::Alias(_) => IconName::SquareTerminal,
                    Choice::Quick(_) => IconName::ArrowRight,
                    Choice::SaveQuick(_) => IconName::Plus,
                    Choice::Act(_) => IconName::ChevronRight,
                    Choice::Snippet(_) => IconName::FileText,
                    Choice::Tab(_) => IconName::PanelLeft,
                };
                let spoken: SharedString = if detail.is_empty() {
                    label.clone()
                } else {
                    format!("{label}, {detail}").into()
                };
                div()
                    .id(("pal-i", ix))
                    .role(Role::ListBoxOption)
                    .aria_label(spoken)
                    .aria_selected(selected)
                    .when(selected, |d| d.aria_active_descendant())
                    .h(px(ROW_H))
                    .w_full()
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(13.0))
                    .when(selected, |d| {
                        d.bg(theme.accent).text_color(theme.accent_foreground)
                    })
                    .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, ev: &ClickEvent, window, cx| {
                        let mods = Mods::from_modifiers(&ev.modifiers());
                        v.confirm(Some(ix), mods, window, cx);
                    }))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(gpui_kit::component::Icon::new(icon).small()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .when(!detail.is_empty(), |d| d.max_w(relative(0.7)))
                            .when(detail.is_empty(), |d| d.flex_1().min_w_0())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label_text),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.0))
                            .text_color(theme.muted_foreground)
                            .child(detail_text),
                    )
                    .children(keys.map(|k| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(k)
                    }))
                    .into_any_element()
            }
        }
    }
}

/// A single word that reads as `user@host[:port]` or `host[:port]`.
fn typed_target(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    hostwick_core::Target::parse(t).ok().map(|_| t.to_owned())
}

impl Render for PaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = self.rows.clone();
        let count = rows.len();
        let hint = [
            ("↵", t!("palette-hint-open")),
            (crate::util::keys("Ctrl+↵", "⌘↵"), t!("palette-hint-files")),
            (crate::util::keys("Shift+↵", "⇧↵"), t!("palette-hint-split")),
            (crate::util::keys("Alt+↵", "⌥↵"), t!("palette-hint-edit")),
            ("Esc", t!("palette-hint-close")),
        ]
        .iter()
        .map(|(k, what)| format!("{k} {what}"))
        .collect::<Vec<_>>()
        .join("   ");
        div()
            .id("palette")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .capture_action(cx.listener(|v, _: &MoveUp, _, cx| {
                v.move_sel(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|v, _: &MoveDown, _, cx| {
                v.move_sel(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|v, _: &MovePageUp, _, cx| {
                v.move_sel(-8, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|v, _: &MovePageDown, _, cx| {
                v.move_sel(8, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|v, a: &Enter, window, cx| {
                let mods = if a.secondary {
                    Mods::Secondary
                } else if a.shift {
                    Mods::Shift
                } else {
                    Mods::Plain
                };
                v.confirm(None, mods, window, cx);
                cx.stop_propagation();
            }))
            .capture_key_down(cx.listener(|v, ev: &KeyDownEvent, window, cx| {
                let m = ev.keystroke.modifiers;
                if ev.keystroke.key == "enter" && m.alt {
                    v.confirm(None, Mods::Alt, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Input::new(&self.input)
                    .large()
                    .prefix(gpui_kit::component::Icon::new(IconName::Search).small())
                    .aria_label(t!("palette-title")),
            )
            .child(
                div()
                    .id("palette-results")
                    .role(Role::ListBox)
                    .aria_label(t!("palette-results"))
                    .h(px((count.clamp(1, 14) as f32) * ROW_H))
                    .when(count == 0, |d| {
                        d.flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme.muted_foreground)
                            .child(crate::a11y::text(t!("palette-nothing")))
                    })
                    .when(count > 0, |d| {
                        d.child(
                            uniform_list(
                                "palette-rows",
                                count,
                                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                    range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    }),
            )
            .child(
                div()
                    .pt(px(4.0))
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(crate::a11y::text(hint)),
            )
    }
}

impl HostwickApp {
    /// Open the palette (or bring focus back to it).
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let cands = self.palette_candidates(window, cx);
        let app = cx.entity().downgrade();
        let frecency = self.frecency.clone();
        let view = cx.new(|cx| PaletteView::new(app, cands, frecency, window, cx));
        self.palette = Some(view.downgrade());
        let first = Rc::new(std::cell::Cell::new(true));
        let input = view.read(cx).input.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            dialog
                .w(px(680.0))
                .margin_top(px(72.0))
                .close_button(false)
                .child(view.clone())
        });
        self.load_ssh_aliases(cx);
    }

    /// Read `~/.ssh/config` off the UI thread; hosts in it that are not
    /// saved become palette and quick-connect suggestions.
    pub(super) fn load_ssh_aliases(&mut self, cx: &mut Context<Self>) {
        let job = cx.background_executor().spawn(async move {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            let path = std::path::Path::new(&home).join(".ssh").join("config");
            let text = hostwick_import::load_ssh_config(&path).ok()?;
            Some(hostwick_import::parse_ssh_config(&text).profiles)
        });
        cx.spawn(async move |this, cx| {
            let found = job.await.unwrap_or_default();
            let _ = this.update(cx, |app, cx| {
                let saved: std::collections::HashSet<String> = app
                    .profiles
                    .iter()
                    .flat_map(|p| {
                        let alias = match &p.source {
                            hostwick_core::ProfileSource::SshConfig { alias } => {
                                Some(alias.clone())
                            }
                            _ => None,
                        };
                        [Some(p.name.to_lowercase()), alias.map(|a| a.to_lowercase())]
                    })
                    .flatten()
                    .collect();
                let fresh: Vec<Rc<HostProfile>> = found
                    .into_iter()
                    .filter(|p| !saved.contains(&p.name.to_lowercase()))
                    .map(Rc::new)
                    .collect();
                let changed = fresh.len() != app.ssh_aliases.len()
                    || fresh
                        .iter()
                        .zip(app.ssh_aliases.iter())
                        .any(|(a, b)| a.name != b.name);
                if changed {
                    app.ssh_aliases = fresh;
                    if let Some(p) = app.palette.as_ref().and_then(|p| p.upgrade()) {
                        let more = Self::alias_cands(&app.ssh_aliases);
                        p.update(cx, |p, cx| {
                            p.cands.retain(|c| c.group != Group::SshConfig);
                            p.add_cands(more, cx);
                        });
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn alias_cands(aliases: &[Rc<HostProfile>]) -> Vec<Cand> {
        aliases
            .iter()
            .map(|p| {
                Cand::new(
                    Choice::Alias(p.clone()),
                    Group::SshConfig,
                    p.name.clone().into(),
                    address(p).into(),
                    format!("ssh:{}", p.name),
                )
            })
            .collect()
    }

    fn palette_candidates(&self, window: &Window, cx: &App) -> Vec<Cand> {
        let mut v = Vec::with_capacity(self.profiles.len() + 80);
        // Recent first, so they lead when nothing is typed.
        for r in self.recents.iter().take(8) {
            match r {
                Recent::Host { id } => {
                    if let Some(p) = self.profiles.iter().find(|p| p.id == *id) {
                        let mut c = Cand::new(
                            Choice::Host(p.id),
                            Group::Recent,
                            p.path().into(),
                            address(p).into(),
                            format!("host:{}", p.id),
                        );
                        c.idle = true;
                        v.push(c);
                    }
                }
                Recent::Quick { text } => {
                    let mut c = Cand::new(
                        Choice::Quick(text.clone()),
                        Group::Recent,
                        text.clone().into(),
                        t!("palette-quick-detail"),
                        format!("quick:{text}"),
                    );
                    c.idle = true;
                    v.push(c);
                }
            }
        }
        let recent_hosts: std::collections::HashSet<ProfileId> = self
            .recents
            .iter()
            .take(8)
            .filter_map(|r| match r {
                Recent::Host { id } => Some(*id),
                Recent::Quick { .. } => None,
            })
            .collect();
        for p in &self.profiles {
            if recent_hosts.contains(&p.id) {
                continue;
            }
            v.push(Cand::new(
                Choice::Host(p.id),
                Group::Hosts,
                p.path().into(),
                address(p).into(),
                format!("host:{}", p.id),
            ));
        }
        v.extend(Self::alias_cands(&self.ssh_aliases));
        if self.tabs.len() > 1 {
            for (i, t) in self.tabs.iter().enumerate() {
                let title = t
                    .focused_pane()
                    .map(|p| Self::pane_title(p, cx))
                    .unwrap_or_default();
                let kind = match t.kind {
                    TabKind::Terminal => t!("keys-group-terminal"),
                    TabKind::Files => t!("keys-group-files"),
                };
                let mut c = Cand::new(
                    Choice::Tab(t.id),
                    Group::Tabs,
                    title,
                    format!("{kind} · {}", i + 1).into(),
                    String::new(),
                );
                c.idle = true;
                v.push(c);
            }
        }
        for a in Act::all() {
            if !self.act_available(a, window, cx) {
                continue;
            }
            let mut c = Cand::new(
                Choice::Act(a),
                Group::Actions,
                a.label(),
                a.detail(),
                format!("action:{}", a.id()),
            );
            c.keys = a.shortcut();
            c.idle = true;
            v.push(c);
        }
        if let Some(term) = self.focused_terminal() {
            for s in term.read(cx).snippets(cx) {
                let key = format!("snippet:{}", s.name);
                v.push(Cand::new(
                    Choice::Snippet(s.clone()),
                    Group::Snippets,
                    s.name.clone().into(),
                    s.text.lines().next().unwrap_or_default().to_owned().into(),
                    key,
                ));
            }
        }
        v
    }

    /// Whether an action makes sense now (no "Split" without a terminal).
    fn act_available(&self, a: Act, _window: &Window, cx: &App) -> bool {
        let tab = self.active.and_then(|i| self.tabs.get(i));
        let term = self.focused_terminal();
        match a {
            Act::SplitRight | Act::SplitDown => tab.is_some_and(|t| t.kind == TabKind::Terminal),
            Act::ClosePane | Act::Reconnect | Act::DuplicateTab | Act::OpenFiles => tab.is_some(),
            Act::NextTab | Act::PrevTab => self.tabs.len() > 1,
            Act::Find | Act::ReadOutput | Act::TextView => term.is_some(),
            Act::FilesHere => {
                term.is_some_and(|t| t.read(cx).transport() == hostwick_term::TransportKind::Ssh)
            }
            Act::LockVault => self.backend.vault.is_unlocked(),
            _ => true,
        }
    }

    /// The terminal of the active tab's focused pane.
    pub(crate) fn focused_terminal(&self) -> Option<Entity<crate::terminal::TerminalView>> {
        match self
            .active
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.focused_pane())
            .map(|p| &p.body)
        {
            Some(PaneBody::Terminal(v)) => Some(v.clone()),
            _ => None,
        }
    }

    /// Do what a palette result (or quick-connect suggestion) says.
    pub(crate) fn run_choice(
        &mut self,
        choice: Choice,
        mods: Mods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match choice {
            Choice::Host(id) => {
                let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() else {
                    return;
                };
                if self.selected != Some(id) {
                    self.select_host(id, cx);
                }
                if mods == Mods::Alt {
                    self.host_dialog(Some(p), window, cx);
                } else {
                    self.open_with(p, None, mods, window, cx);
                }
            }
            Choice::Quick(text) => {
                if mods == Mods::Alt {
                    self.save_quick_as_host(&text, window, cx);
                    return;
                }
                match crate::backend::quick_profile(&text) {
                    Ok(p) => self.open_with(p, Some(text), mods, window, cx),
                    Err(e) => window.toast(Toast::error(e.to_string()), cx),
                }
            }
            Choice::SaveQuick(text) => self.save_quick_as_host(&text, window, cx),
            Choice::Alias(p) => {
                if mods == Mods::Alt {
                    self.apply_import(
                        Ok(hostwick_import::ImportResult {
                            profiles: vec![(*p).clone()],
                            warnings: Vec::new(),
                        }),
                        window,
                        cx,
                    );
                    self.load_ssh_aliases(cx);
                    return;
                }
                // Restored later from its address (the alias is not saved).
                let quick = alias_target(&p);
                self.open_with((*p).clone(), Some(quick), mods, window, cx);
            }
            Choice::Act(a) => self.run_act(a, window, cx),
            Choice::Snippet(s) => match self.focused_terminal() {
                Some(t) => {
                    t.update(cx, |t, cx| t.send_snippet(&s, cx));
                    t.read(cx).focus_handle(cx).focus(window, cx);
                }
                None => window.toast(Toast::info(t!("palette-no-terminal")), cx),
            },
            Choice::Tab(id) => {
                if let Some(ix) = self.tabs.iter().position(|t| t.id == id) {
                    self.activate_tab(ix, window, cx);
                }
            }
        }
    }

    /// Open a host as a terminal, files (Ctrl/⌘) or a new split (Shift).
    pub(crate) fn open_with(
        &mut self,
        profile: HostProfile,
        quick: Option<String>,
        mods: Mods,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match mods {
            Mods::Secondary => self.open_tab_quick(profile, quick, TabKind::Files, window, cx),
            Mods::Shift => self.split_with(profile, quick, window, cx),
            _ => self.open_tab_quick(profile, quick, TabKind::Terminal, window, cx),
        }
    }

    /// Open `profile` in a new pane right of the focused terminal pane (a
    /// new tab when no terminal tab is active).
    fn split_with(
        &mut self,
        profile: HostProfile,
        quick: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .active
            .and_then(|i| self.tabs.get(i).map(|t| (i, t)))
            .filter(|(_, t)| t.kind == TabKind::Terminal)
            .and_then(|(i, t)| t.focused_pane().map(|p| (i, p.id)));
        let Some((ix, target)) = target else {
            self.open_tab_quick(profile, quick, TabKind::Terminal, window, cx);
            return;
        };
        self.note_recent(&profile, quick.as_deref());
        let pane = self.new_pane(profile, quick);
        let new_id = pane.id;
        let split_id = self.next_id();
        if self.tabs[ix]
            .root
            .split(target, pane, false, split_id)
            .is_ok()
        {
            self.tabs[ix].focused = new_id;
            self.connect_pane(new_id, window, cx);
        }
        self.persist_tabs(cx);
        cx.notify();
    }

    fn save_quick_as_host(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        match crate::backend::quick_profile(text) {
            Ok(mut p) => {
                p.name = text.split('@').next_back().unwrap_or(text).to_owned();
                self.host_dialog_new(p, window, cx);
            }
            Err(e) => window.toast(Toast::error(e.to_string()), cx),
        }
    }

    pub(crate) fn run_act(&mut self, a: Act, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active;
        match a {
            Act::NewHost => self.host_dialog(None, window, cx),
            Act::NewFolder => self.new_folder("", window, cx),
            Act::QuickConnect => {
                self.quick.update(cx, |s, cx| s.focus(window, cx));
            }
            Act::SplitRight => self.split_focused(false, window, cx),
            Act::SplitDown => self.split_focused(true, window, cx),
            Act::ClosePane => self.close_focused_pane(window, cx),
            Act::Reconnect => {
                if let Some(ix) = tab {
                    self.reconnect_focused(ix, window, cx);
                }
            }
            Act::DuplicateTab => self.duplicate_active(window, cx),
            Act::NextTab | Act::PrevTab => {
                let n = self.tabs.len();
                if n > 0 {
                    let cur = tab.unwrap_or(0);
                    let next = if a == Act::NextTab {
                        (cur + 1) % n
                    } else {
                        (cur + n - 1) % n
                    };
                    self.activate_tab(next, window, cx);
                }
            }
            Act::Broadcast => {
                self.broadcast = !self.broadcast;
                cx.notify();
            }
            Act::Find => {
                if let Some(t) = self.focused_terminal() {
                    t.update(cx, |t, cx| t.open_search(window, cx));
                }
            }
            Act::FilesHere => {
                if let Some(p) = tab
                    .and_then(|i| self.tabs.get(i))
                    .and_then(|t| t.focused_pane())
                {
                    let id = p.id;
                    self.open_files_here(id, window, cx);
                }
            }
            Act::OpenFiles => {
                if let Some(p) = tab
                    .and_then(|i| self.tabs.get(i))
                    .and_then(|t| t.focused_pane())
                {
                    let (profile, quick) = (p.profile.clone(), p.quick.clone());
                    self.open_tab_quick(profile, quick, TabKind::Files, window, cx);
                }
            }
            Act::ReadOutput => {
                if let Some(t) = self.focused_terminal() {
                    t.update(cx, |t, cx| t.read_recent_output(cx));
                }
            }
            Act::TextView => {
                if let Some(t) = self.focused_terminal() {
                    t.update(cx, |t, cx| t.open_text_view(window, cx));
                }
            }
            Act::Settings(s) => self.open_settings(Some(s), window, cx),
            Act::GenerateKey => crate::tools::keygen::open(
                self.backend.clone(),
                cx.entity().downgrade(),
                window,
                cx,
            ),
            Act::InspectKey => crate::tools::keyconv::open(self.backend.clone(), window, cx),
            Act::KnownHosts => crate::tools::knownhosts::open(self.backend.clone(), window, cx),
            Act::LockVault => {
                self.backend.vault.lock();
                cx.notify();
            }
            Act::ScanComputer => self.scan_computer(window, cx),
            Act::ImportSshConfig => self.import("ssh", window, cx),
            Act::ImportPuttyRegistry => self.import("putty", window, cx),
            Act::ImportWinscpRegistry => self.import("winscp", window, cx),
            Act::ImportPuttyFile => self.import("putty-file", window, cx),
            Act::ImportWinscpFile => self.import("winscp-file", window, cx),
            Act::ImportSolar => self.import("solar", window, cx),
            Act::ToggleTheme => self.toggle_theme(window, cx),
            Act::Shortcuts => self.open_shortcuts(window, cx),
        }
    }
}

/// `user@host:port` (the port only when it is not the protocol's default).
pub(crate) fn address(p: &HostProfile) -> String {
    match &p.protocol {
        hostwick_core::Protocol::Serial(s) => {
            format!("{} {}", p.host, hostwick_term::serial::summary(s))
        }
        _ => {
            let user = p.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
            if p.port == p.protocol.default_port() {
                format!("{user}{}", p.host)
            } else {
                format!("{user}{}:{}", p.host, p.port)
            }
        }
    }
}

/// Quick-connect text that reaches what an ssh_config alias points at.
fn alias_target(p: &HostProfile) -> String {
    let user = p.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
    if p.port == 22 {
        format!("{user}{}", p.host)
    } else {
        format!("{user}{}:{}", p.host, p.port)
    }
}

#[cfg(test)]
mod tests {
    use super::{Act, Mods, typed_target};
    use gpui_kit::Modifiers;

    #[test]
    fn typed_targets() {
        assert_eq!(
            typed_target("root@10.0.0.5:2222").as_deref(),
            Some("root@10.0.0.5:2222")
        );
        assert_eq!(typed_target(" web01 ").as_deref(), Some("web01"));
        assert_eq!(typed_target("two words"), None);
        assert_eq!(typed_target(""), None);
    }

    #[test]
    fn modifiers_map_to_what_enter_does() {
        let m = |f: fn(&mut Modifiers)| {
            let mut x = Modifiers::default();
            f(&mut x);
            Mods::from_modifiers(&x)
        };
        assert_eq!(m(|_| {}), Mods::Plain);
        assert_eq!(m(|x| x.shift = true), Mods::Shift);
        assert_eq!(m(|x| x.alt = true), Mods::Alt);
        if cfg!(target_os = "macos") {
            assert_eq!(m(|x| x.platform = true), Mods::Secondary);
        } else {
            assert_eq!(m(|x| x.control = true), Mods::Secondary);
        }
        // Every action has a label and a stable id.
        for a in Act::all() {
            assert!(!a.id().is_empty());
        }
    }
}
