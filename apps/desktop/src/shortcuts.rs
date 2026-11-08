//! Every keyboard shortcut in one place, written the way this platform
//! writes it: for the cheat sheet, the command palette and the hints in
//! context menus.

use crate::zoom::px;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::*;

use crate::terminal::keys::Mods;
use crate::util::keys;

/// Where a shortcut works (the cheat sheet's groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    App,
    Tabs,
    Hosts,
    Terminal,
    Files,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::App,
        Group::Tabs,
        Group::Hosts,
        Group::Terminal,
        Group::Files,
    ];

    pub fn label(self) -> SharedString {
        match self {
            Group::App => t!("keys-group-app"),
            Group::Tabs => t!("keys-group-tabs"),
            Group::Hosts => t!("keys-group-hosts"),
            Group::Terminal => t!("keys-group-terminal"),
            Group::Files => t!("keys-group-files"),
        }
    }
}

/// A shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sc {
    Palette,
    Shortcuts,
    NewHost,
    Settings,
    Broadcast,
    CycleFocus,
    ContextMenu,
    DuplicateTab,
    ReopenTab,
    TabNumber,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ClosePane,
    Reconnect,
    NextTab,
    PrevTab,
    SplitRight,
    SplitDown,
    MovePane,
    ResizePane,
    HostOpen,
    HostFiles,
    HostSplit,
    HostEdit,
    HostRename,
    HostDelete,
    HostSelectAll,
    HostMultiSelect,
    HostRangeSelect,
    Copy,
    Paste,
    Find,
    FindNext,
    ScrollBack,
    SelectInApp,
    SendBreak,
    ReadOutput,
    TextView,
    FilesOpen,
    FilesUp,
    FilesEdit,
    FilesCopy,
    FilesMove,
    FilesRename,
    FilesNewFolder,
    FilesDelete,
    FilesRefresh,
    FilesSwitchSide,
    FilesProperties,
    FilesSelectAll,
    FilesToggleRow,
    FilesPreview,
    FilesPath,
    FilesBookmark,
    FilesFilter,
}

impl Sc {
    pub const ALL: [Sc; 56] = [
        Sc::Palette,
        Sc::Shortcuts,
        Sc::NewHost,
        Sc::Settings,
        Sc::Broadcast,
        Sc::CycleFocus,
        Sc::ContextMenu,
        Sc::DuplicateTab,
        Sc::ReopenTab,
        Sc::TabNumber,
        Sc::ZoomIn,
        Sc::ZoomOut,
        Sc::ZoomReset,
        Sc::ClosePane,
        Sc::Reconnect,
        Sc::NextTab,
        Sc::PrevTab,
        Sc::SplitRight,
        Sc::SplitDown,
        Sc::MovePane,
        Sc::ResizePane,
        Sc::HostOpen,
        Sc::HostFiles,
        Sc::HostSplit,
        Sc::HostEdit,
        Sc::HostRename,
        Sc::HostDelete,
        Sc::HostSelectAll,
        Sc::HostMultiSelect,
        Sc::HostRangeSelect,
        Sc::Copy,
        Sc::Paste,
        Sc::Find,
        Sc::FindNext,
        Sc::ScrollBack,
        Sc::SelectInApp,
        Sc::SendBreak,
        Sc::ReadOutput,
        Sc::TextView,
        Sc::FilesOpen,
        Sc::FilesUp,
        Sc::FilesEdit,
        Sc::FilesCopy,
        Sc::FilesMove,
        Sc::FilesRename,
        Sc::FilesNewFolder,
        Sc::FilesDelete,
        Sc::FilesRefresh,
        Sc::FilesSwitchSide,
        Sc::FilesProperties,
        Sc::FilesSelectAll,
        Sc::FilesToggleRow,
        Sc::FilesPreview,
        Sc::FilesPath,
        Sc::FilesBookmark,
        Sc::FilesFilter,
    ];

    /// The keys, as written on this platform.
    pub fn keys(self) -> &'static str {
        match self {
            Sc::Palette => keys("Ctrl+Shift+P · Ctrl+Shift+K", "⌘K · ⌘⇧P"),
            Sc::Shortcuts => keys("F1 · Ctrl+/ · Ctrl+Shift+/", "F1 · ⌘/ · ⌘⇧/"),
            Sc::NewHost => keys("Ctrl+Shift+N", "⌘N"),
            Sc::Settings => keys("Ctrl+,", "⌘,"),
            Sc::Broadcast => keys("Ctrl+Shift+B", "⌘B"),
            Sc::CycleFocus => keys("Ctrl+F6 · Ctrl+Shift+F6", "⌃F6 · ⌃⇧F6"),
            Sc::ContextMenu => keys("Shift+F10 · Menu", "⇧F10"),
            Sc::DuplicateTab => keys("Ctrl+Shift+U", "⌘T"),
            Sc::ReopenTab => keys("Ctrl+Shift+T", "⌘⇧T"),
            Sc::TabNumber => keys("Ctrl+Alt+1…8 · Ctrl+Alt+9", "⌘1…8 · ⌘9"),
            Sc::ZoomIn => keys("Ctrl+= · Ctrl++ · Ctrl+wheel", "⌘= · ⌘+ · ⌘-wheel"),
            Sc::ZoomOut => keys("Ctrl+Alt+- · Ctrl+wheel", "⌘- · ⌘-wheel"),
            Sc::ZoomReset => keys("Ctrl+0", "⌘0"),
            Sc::ClosePane => keys("Ctrl+Shift+W", "⌘W"),
            Sc::Reconnect => keys("Ctrl+Shift+R", "⌘R"),
            Sc::NextTab => keys("Ctrl+Tab", "⌃Tab"),
            Sc::PrevTab => keys("Ctrl+Shift+Tab", "⌃⇧Tab"),
            Sc::SplitRight => keys("Ctrl+Shift+D", "⌘D"),
            Sc::SplitDown => keys("Ctrl+Shift+E", "⌘⇧D"),
            Sc::MovePane => keys("Alt+←→↑↓", "⌥←→↑↓"),
            Sc::ResizePane => keys("Alt+Shift+←→↑↓", "⌥⇧←→↑↓"),
            Sc::HostOpen => "Enter",
            Sc::HostFiles => keys("Ctrl+Enter", "⌘↩"),
            Sc::HostSplit => keys("Shift+Enter", "⇧↩"),
            Sc::HostEdit => keys("Alt+Enter", "⌥↩"),
            Sc::HostRename => "F2",
            Sc::HostDelete => keys("Delete", "⌦"),
            Sc::HostSelectAll => keys("Ctrl+A", "⌘A"),
            Sc::HostMultiSelect => keys("Ctrl+click · Shift+↑↓", "⌘-click · ⇧↑↓"),
            Sc::HostRangeSelect => keys("Shift+click", "⇧-click"),
            Sc::Copy => keys("Ctrl+Shift+C", "⌘C"),
            Sc::Paste => keys("Ctrl+Shift+V · Shift+Insert", "⌘V"),
            Sc::Find => keys("Ctrl+Shift+F", "⌘F"),
            Sc::FindNext => keys("Enter · Shift+Enter · F3", "↩ · ⇧↩ · F3"),
            Sc::ScrollBack => keys("Shift+PageUp / PageDown", "⇧PageUp / PageDown"),
            Sc::SelectInApp => keys("Shift+drag", "⇧-drag"),
            Sc::SendBreak => keys("Ctrl+Break", "⌃Break"),
            Sc::ReadOutput => keys("Ctrl+Shift+O", "⌘⇧O"),
            Sc::TextView => keys("Ctrl+Shift+A", "⌘⇧A"),
            Sc::FilesOpen => "Enter",
            Sc::FilesUp => keys("Backspace", "⌫"),
            Sc::FilesEdit => "F4",
            Sc::FilesCopy => "F5",
            Sc::FilesMove => "F6",
            Sc::FilesRename => "F2",
            Sc::FilesNewFolder => "F7",
            Sc::FilesDelete => keys("Delete", "⌦"),
            Sc::FilesRefresh => keys("Ctrl+R", "⌘R"),
            Sc::FilesSwitchSide => "Tab",
            Sc::FilesProperties => keys("Alt+Enter", "⌥↩"),
            Sc::FilesSelectAll => keys("Ctrl+A", "⌘A"),
            Sc::FilesToggleRow => keys("Ctrl+Space · Insert", "⌘Space · Insert"),
            Sc::FilesPreview => "Space",
            Sc::FilesPath => keys("Ctrl+L", "⌘L"),
            Sc::FilesBookmark => keys("Ctrl+D", "⌘D"),
            Sc::FilesFilter => keys("type · Ctrl+F", "type · ⌘F"),
        }
    }

    /// The short form shown next to a menu item (the first alternative).
    pub fn hint(self) -> &'static str {
        let k = self.keys();
        k.split(" · ").next().unwrap_or(k)
    }

    pub fn group(self) -> Group {
        use Sc::*;
        match self {
            Palette | Shortcuts | NewHost | Settings | Broadcast | CycleFocus | ContextMenu => {
                Group::App
            }
            DuplicateTab | ReopenTab | TabNumber | ZoomIn | ZoomOut | ZoomReset | ClosePane
            | Reconnect | NextTab | PrevTab | SplitRight | SplitDown | MovePane | ResizePane => {
                Group::Tabs
            }
            HostOpen | HostFiles | HostSplit | HostEdit | HostRename | HostDelete
            | HostSelectAll | HostMultiSelect | HostRangeSelect => Group::Hosts,
            Copy | Paste | Find | FindNext | ScrollBack | SelectInApp | SendBreak | ReadOutput
            | TextView => Group::Terminal,
            _ => Group::Files,
        }
    }

    pub fn label(self) -> SharedString {
        match self {
            Sc::Palette => t!("keys-palette"),
            Sc::Shortcuts => t!("keys-shortcuts"),
            Sc::NewHost => t!("keys-new-host"),
            Sc::Settings => t!("keys-settings"),
            Sc::Broadcast => t!("keys-broadcast"),
            Sc::CycleFocus => t!("keys-cycle-focus"),
            Sc::ContextMenu => t!("keys-context-menu"),
            Sc::DuplicateTab => t!("keys-duplicate-tab"),
            Sc::ReopenTab => t!("keys-reopen-tab"),
            Sc::TabNumber => t!("keys-tab-number"),
            Sc::ZoomIn => t!("keys-zoom-in"),
            Sc::ZoomOut => t!("keys-zoom-out"),
            Sc::ZoomReset => t!("keys-zoom-reset"),
            Sc::ClosePane => t!("keys-close-pane"),
            Sc::Reconnect => t!("keys-reconnect"),
            Sc::NextTab => t!("keys-next-tab"),
            Sc::PrevTab => t!("keys-prev-tab"),
            Sc::SplitRight => t!("keys-split-right"),
            Sc::SplitDown => t!("keys-split-down"),
            Sc::MovePane => t!("keys-move-pane"),
            Sc::ResizePane => t!("keys-resize-pane"),
            Sc::HostOpen => t!("keys-host-open"),
            Sc::HostFiles => t!("keys-host-files"),
            Sc::HostSplit => t!("keys-host-split"),
            Sc::HostEdit => t!("keys-host-edit"),
            Sc::HostRename => t!("keys-host-rename"),
            Sc::HostDelete => t!("keys-host-delete"),
            Sc::HostSelectAll => t!("keys-host-select-all"),
            Sc::HostMultiSelect => t!("keys-host-multi"),
            Sc::HostRangeSelect => t!("keys-host-range"),
            Sc::Copy => t!("keys-copy"),
            Sc::Paste => t!("keys-paste"),
            Sc::Find => t!("keys-find"),
            Sc::FindNext => t!("keys-find-next"),
            Sc::ScrollBack => t!("keys-scroll-back"),
            Sc::SelectInApp => t!("keys-select-in-app"),
            Sc::SendBreak => t!("keys-send-break"),
            Sc::ReadOutput => t!("keys-read-output"),
            Sc::TextView => t!("keys-text-view"),
            Sc::FilesOpen => t!("keys-files-open"),
            Sc::FilesUp => t!("keys-files-up"),
            Sc::FilesEdit => t!("keys-files-edit"),
            Sc::FilesCopy => t!("keys-files-copy"),
            Sc::FilesMove => t!("keys-files-move"),
            Sc::FilesRename => t!("keys-files-rename"),
            Sc::FilesNewFolder => t!("keys-files-new-folder"),
            Sc::FilesDelete => t!("keys-files-delete"),
            Sc::FilesRefresh => t!("keys-files-refresh"),
            Sc::FilesSwitchSide => t!("keys-files-switch"),
            Sc::FilesProperties => t!("keys-files-properties"),
            Sc::FilesSelectAll => t!("keys-files-select-all"),
            Sc::FilesToggleRow => t!("keys-files-toggle-row"),
            Sc::FilesPreview => t!("keys-files-preview"),
            Sc::FilesPath => t!("keys-files-path"),
            Sc::FilesBookmark => t!("keys-files-bookmark"),
            Sc::FilesFilter => t!("keys-files-filter"),
        }
    }
}

/// A context-menu item with its shortcut shown, muted, on the right.
pub fn menu_item(label: impl Into<SharedString>, sc: Option<Sc>) -> PopupMenuItem {
    let label: SharedString = label.into();
    match sc {
        None => PopupMenuItem::new(label),
        Some(sc) => menu_item_keys(label, sc.hint()),
    }
}

/// A context-menu item with `keys` shown on the right.
pub fn menu_item_keys(label: impl Into<SharedString>, keys: &'static str) -> PopupMenuItem {
    let label: SharedString = label.into();
    PopupMenuItem::element(move |_, cx| {
        let muted = cx.theme().muted_foreground;
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .child(crate::a11y::text(label.clone()))
            .child(div().flex_none().text_xs().text_color(muted).child(keys))
    })
}

// ---- what the keyboard does while a terminal has focus -------------------------

/// What a key press means to the app itself, decided before the terminal
/// sees it. Whether the action then happens (is there a second pane? that
/// many tabs?) is up to the caller; keys that do nothing there go on to the
/// shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// F6 moves between the hosts list, the tab bar and the session.
    CycleRegion {
        forward: bool,
    },
    Palette,
    Help,
    SplitRight,
    SplitDown,
    /// 1..=9: tab N (9 is the last one).
    TabNumber(usize),
    /// +1 in, -1 out, 0 reset.
    Zoom(i32),
    /// Next (`false`) or previous tab.
    CycleTab {
        back: bool,
    },
    /// Move focus (or, with Shift, resize) between panes.
    Pane {
        resize: bool,
    },
    ReopenTab,
    DuplicateTab,
    ClosePane,
    Reconnect,
    NewHost,
    Settings,
    Broadcast,
}

/// The one rule for app shortcuts: **while a terminal has the keyboard, the
/// app never takes a key that `terminal::keys::encode` sends to the shell**
/// (Ctrl+2…8, Ctrl+-, Alt+digit, … are control characters and readline
/// arguments there). Outside terminals (file browser, host list) the plain
/// Ctrl variants work as well, since nothing is listening for them.
///
/// `typed` is the text the key press would insert (AltGr layouts type `{`
/// on Ctrl+Alt+7: that is text, not a shortcut). `in_terminal` says whether
/// a terminal has focus. `mac` is a parameter so both platforms are tested.
pub fn classify(
    key: &str,
    m: Mods,
    mac: bool,
    typed: Option<&str>,
    in_terminal: bool,
) -> Option<Chord> {
    use Chord::*;
    if key == "f6" && m.ctrl && !m.alt && !m.platform {
        return Some(CycleRegion { forward: !m.shift });
    }
    let cmd = if mac { m.platform } else { m.ctrl && m.shift };
    // Command palette: Ctrl+Shift+P / Ctrl+Shift+K; ⌘K / ⌘⇧P.
    let palette = if mac {
        m.platform && !m.alt && !m.ctrl && (key == "k" || (key == "p" && m.shift))
    } else {
        m.ctrl && m.shift && !m.alt && (key == "p" || key == "k")
    };
    if palette {
        return Some(Palette);
    }
    // Cheat sheet: Ctrl+Shift+/ (⌘⇧/) anywhere; F1 and Ctrl+/ (⌘/) unless a
    // terminal has the keys (programs use them).
    let slash = key == "/" || key == "?";
    let app_mod = if mac { m.platform } else { m.ctrl };
    if slash && app_mod && m.shift && !m.alt {
        return Some(Help);
    }
    let modified = m.ctrl || m.alt || m.shift || m.platform;
    if !in_terminal && ((key == "f1" && !modified) || (slash && app_mod && !m.shift && !m.alt)) {
        return Some(Help);
    }
    // Split right: Ctrl+Shift+D / ⌘D. Split down: Ctrl+Shift+E / ⌘⇧D.
    if mac {
        if m.platform && key == "d" {
            return Some(if m.shift { SplitDown } else { SplitRight });
        }
    } else if m.ctrl && m.shift && !m.alt {
        match key {
            "d" => return Some(SplitRight),
            "e" => return Some(SplitDown),
            _ => {}
        }
    }
    // Text this key press would type, other than the key itself: it is
    // input, not a shortcut.
    let types_text = typed.is_some_and(|t| !t.is_empty() && t != key);
    // Tabs by number: ⌘1…9 on macOS, Ctrl+Alt+1…9 elsewhere (Ctrl+1…9 as
    // well outside terminals).
    if let Some(n) = digit(key) {
        let hit = if mac {
            m.platform && !m.ctrl && !m.alt && !m.shift
        } else {
            m.ctrl
                && !m.shift
                && !m.platform
                && match (m.alt, in_terminal) {
                    (true, _) => !types_text,
                    (false, in_t) => !in_t,
                }
        };
        if hit {
            return Some(TabNumber(n));
        }
    }
    // Zoom: Ctrl/⌘ + = (or +), 0 and, outside terminals, Ctrl+-; in a
    // terminal Ctrl+- is the shell's undo, so zoom out is Ctrl+Alt+- there.
    let zoom_dir = match key {
        "=" | "+" | "plus" => Some(1),
        "-" | "minus" if !m.shift => Some(-1),
        "0" if !m.shift => Some(0),
        _ => None,
    };
    if let Some(dir) = zoom_dir {
        let hit = if mac {
            m.platform && !m.ctrl && !m.alt
        } else if m.ctrl && !m.platform {
            match (m.alt, dir) {
                (false, -1) => !in_terminal,
                (false, _) => true,
                (true, -1) => !types_text,
                (true, _) => false,
            }
        } else {
            false
        };
        if hit {
            return Some(Zoom(dir));
        }
    }
    match key {
        "left" | "right" | "up" | "down" if m.alt && !m.ctrl && !m.platform => {
            Some(Pane { resize: m.shift })
        }
        "tab" if m.ctrl => Some(CycleTab { back: m.shift }),
        "t" if cmd && m.shift => Some(ReopenTab),
        "t" if cmd && mac => Some(DuplicateTab),
        "u" if cmd && !mac => Some(DuplicateTab),
        "w" if cmd => Some(ClosePane),
        "r" if cmd => Some(Reconnect),
        "n" if cmd => Some(NewHost),
        "," if m.ctrl || m.platform => Some(Settings),
        "b" if cmd => Some(Broadcast),
        _ => None,
    }
}

/// The digit of a tab-number shortcut.
pub fn digit(key: &str) -> Option<usize> {
    match key.as_bytes() {
        [d @ b'1'..=b'9'] => Some(usize::from(d - b'0')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Chord, Group, Sc, classify};
    use crate::terminal::keys::{self as term_keys, Mods};
    use crate::util::keys;

    fn mods(ctrl: bool, alt: bool, shift: bool, platform: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            platform,
        }
    }

    /// Every key name the terminal or the app cares about.
    fn all_keys() -> Vec<String> {
        let mut v: Vec<String> = (0x20u8..0x7f).map(|b| char::from(b).to_string()).collect();
        for n in [
            "enter",
            "tab",
            "escape",
            "backspace",
            "space",
            "up",
            "down",
            "left",
            "right",
            "home",
            "end",
            "insert",
            "delete",
            "pageup",
            "pagedown",
            "plus",
            "minus",
        ] {
            v.push(n.to_owned());
        }
        for n in 1..=12 {
            v.push(format!("f{n}"));
        }
        v
    }

    /// Shortcuts that are allowed to share a key with the shell because they
    /// are escape hatches, not conveniences: Ctrl+F6 leaves a terminal
    /// without the mouse (Tab is the shell's), and Alt+arrows move between
    /// panes only when there is a pane that way (the caller passes the key on
    /// otherwise, so `Alt+←` still reaches zsh in a single pane).
    fn is_escape_hatch(c: Chord) -> bool {
        matches!(c, Chord::CycleRegion { .. } | Chord::Pane { .. })
    }

    /// The rule: with a terminal focused the app takes no key the terminal
    /// encodes for the shell. Walks every key with every modifier
    /// combination.
    #[test]
    fn app_shortcuts_in_a_terminal_never_take_keys_the_shell_gets() {
        let mut checked = 0;
        for mac in [false, true] {
            for key in all_keys() {
                for bits in 0..16u8 {
                    let m = mods(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
                    let Some(c) = classify(&key, m, mac, None, true) else {
                        continue;
                    };
                    checked += 1;
                    if is_escape_hatch(c) {
                        continue;
                    }
                    for app_cursor in [false, true] {
                        assert_eq!(
                            term_keys::encode(&key, m, app_cursor),
                            None,
                            "{c:?} ({key} with {m:?}, mac={mac}) takes a key the shell gets"
                        );
                    }
                }
            }
        }
        assert!(checked > 60, "only {checked} chords walked");
    }

    #[test]
    fn the_keys_the_shell_needs_are_not_app_shortcuts_in_a_terminal() {
        let ctrl = mods(true, false, false, false);
        // Ctrl+2…8 (NUL ESC FS GS RS US DEL) and Ctrl+- (US, readline undo).
        for k in ["2", "3", "4", "5", "6", "7", "8", "-"] {
            assert!(term_keys::encode(k, ctrl, false).is_some(), "{k}");
            assert_eq!(classify(k, ctrl, false, None, true), None, "Ctrl+{k}");
        }
        // Alt+digit is readline's numeric argument (and tmux M-1 bindings).
        let alt = mods(false, true, false, false);
        for d in 1..=9 {
            let k = d.to_string();
            assert!(term_keys::encode(&k, alt, false).is_some() || cfg!(target_os = "macos"));
            assert_eq!(classify(&k, alt, false, None, true), None, "Alt+{k}");
        }
        // Ctrl+Tab with the terminal focused was Tab for the shell; now it
        // is the tab switch and nothing is sent.
        assert!(matches!(
            classify("tab", ctrl, false, None, true),
            Some(Chord::CycleTab { back: false })
        ));
    }

    #[test]
    fn tab_jump_and_zoom_chords() {
        let ca = mods(true, true, false, false);
        let ctrl = mods(true, false, false, false);
        for n in 1..=9usize {
            let k = n.to_string();
            assert_eq!(
                classify(&k, ca, false, None, true),
                Some(Chord::TabNumber(n))
            );
            assert_eq!(
                classify(&k, ca, false, None, false),
                Some(Chord::TabNumber(n))
            );
            // Plain Ctrl+N only where no terminal listens.
            assert_eq!(classify(&k, ctrl, false, None, true), None, "{k}");
            assert_eq!(
                classify(&k, ctrl, false, None, false),
                Some(Chord::TabNumber(n))
            );
            assert_eq!(
                classify(&k, mods(false, false, false, true), true, None, true),
                Some(Chord::TabNumber(n))
            );
        }
        assert_eq!(classify("0", ca, false, None, true), None);
        // Zoom.
        assert_eq!(classify("=", ctrl, false, None, true), Some(Chord::Zoom(1)));
        assert_eq!(classify("0", ctrl, false, None, true), Some(Chord::Zoom(0)));
        assert_eq!(classify("-", ctrl, false, None, true), None);
        assert_eq!(
            classify("-", ctrl, false, None, false),
            Some(Chord::Zoom(-1))
        );
        assert_eq!(classify("-", ca, false, None, true), Some(Chord::Zoom(-1)));
        assert_eq!(
            classify("-", mods(false, false, false, true), true, None, true),
            Some(Chord::Zoom(-1))
        );
    }

    #[test]
    fn text_typed_with_ctrl_alt_is_not_a_shortcut() {
        // AltGr layouts report Ctrl+Alt and type { [ ] } on 7 8 9 0.
        let ca = mods(true, true, false, false);
        assert_eq!(classify("7", ca, false, Some("{"), true), None);
        assert_eq!(classify("-", ca, false, Some("\u{ad}"), true), None);
        assert_eq!(
            classify("7", ca, false, Some("7"), true),
            Some(Chord::TabNumber(7))
        );
        assert_eq!(
            classify("7", ca, false, Some(""), true),
            Some(Chord::TabNumber(7))
        );
    }

    #[test]
    fn the_cheat_sheet_names_the_chords_that_work() {
        if cfg!(not(target_os = "macos")) {
            assert_eq!(Sc::TabNumber.keys(), "Ctrl+Alt+1…8 · Ctrl+Alt+9");
            assert_eq!(Sc::ZoomOut.hint(), "Ctrl+Alt+-");
            assert!(Sc::ZoomIn.keys().contains("Ctrl+="));
            assert_eq!(Sc::ZoomReset.keys(), "Ctrl+0");
        }
    }

    #[test]
    fn every_shortcut_has_keys_and_a_group() {
        for sc in Sc::ALL {
            assert!(!sc.keys().is_empty());
            assert!(!sc.hint().contains(" · "));
            let _ = sc.group();
        }
        // Every group has something in it.
        for g in Group::ALL {
            assert!(Sc::ALL.iter().any(|s| s.group() == g), "{g:?}");
        }
        assert_eq!(Sc::Palette.hint(), keys("Ctrl+Shift+P", "⌘K"));
    }
}
