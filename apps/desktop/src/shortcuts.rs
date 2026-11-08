//! Every keyboard shortcut in one place, written the way this platform
//! writes it: for the cheat sheet, the command palette and the hints in
//! context menus.

use crate::zoom::px;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::*;

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
            Sc::TabNumber => keys("Ctrl+1…8 · Ctrl+9", "⌘1…8 · ⌘9"),
            Sc::ZoomIn => keys("Ctrl+= · Ctrl++ · Ctrl+wheel", "⌘= · ⌘+ · ⌘-wheel"),
            Sc::ZoomOut => keys("Ctrl+-", "⌘-"),
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

#[cfg(test)]
mod tests {
    use super::{Group, Sc};
    use crate::util::keys;

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
