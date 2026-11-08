//! What can be done with tabs beyond opening and closing them: jump by
//! number, reorder, drop one into another's split, bring a closed one back
//! (with its layout and scrollback), and zoom the terminal text.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::*;

use super::{HostwickApp, Pane, PaneBody, Tab, TabKind};
use crate::layout::{Edge, SavedNode};
use crate::notify::{Toast, ToastExt as _};

/// Ids of tabs, panes and splits are unique across every window, so a tab
/// can move from one window to another without clashing.
static IDS: AtomicU64 = AtomicU64::new(1);

pub(super) fn fresh_id() -> u64 {
    IDS.fetch_add(1, Ordering::Relaxed)
}

/// How many closed tabs "Reopen closed tab" remembers.
pub(super) const KEEP_CLOSED: usize = 10;

/// A tab that was closed: enough to open it again where it was.
#[derive(Clone)]
pub(super) struct ClosedTab {
    pub kind: TabKind,
    pub root: SavedNode,
    pub zoom: u32,
    /// Saved pane id → what its terminal showed (the scrollback), as text.
    pub history: HashMap<u64, String>,
}

/// The closed tabs of every window, newest last.
#[derive(Default)]
pub(super) struct ClosedTabs(pub Vec<ClosedTab>);

impl Global for ClosedTabs {}

/// Remember `tab`, forgetting the oldest beyond [`KEEP_CLOSED`].
pub(super) fn push_closed(stack: &mut Vec<ClosedTab>, tab: ClosedTab) {
    stack.push(tab);
    let extra = stack.len().saturating_sub(KEEP_CLOSED);
    stack.drain(..extra);
}

/// Where a tab-number shortcut (Ctrl+Alt+N, ⌘N) goes: 1..=8 are tabs 1 to 8, 9 is the last one.
pub(super) fn tab_for_number(n: usize, count: usize) -> Option<usize> {
    match n {
        1..=8 if n <= count => Some(n - 1),
        9 => count.checked_sub(1),
        _ => None,
    }
}

impl HostwickApp {
    /// The text on a tab.
    pub(super) fn tab_label(t: &Tab, cx: &App) -> SharedString {
        let panes = t.panes();
        let base = t
            .focused_pane()
            .map(|p| Self::pane_title(p, cx))
            .unwrap_or_default();
        match (t.kind, panes.len()) {
            (TabKind::Files, _) => {
                let linked = panes.iter().any(|p| p.follows.is_some());
                let name = t.profile().map(|p| p.name.clone()).unwrap_or_default();
                let title = t!("tab-files-title", name = name);
                if linked {
                    format!("{title} ⇄").into()
                } else {
                    title
                }
            }
            (_, 1) => base,
            (_, n) => format!("{base} +{}", n - 1).into(),
        }
    }

    /// Ctrl+Alt/⌘+1…8 go to that tab, 9 to the last one.
    pub(super) fn jump_to_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = tab_for_number(n, self.tabs.len()) {
            self.activate_tab(ix, window, cx);
        }
    }

    /// Move a tab into gap `slot` of the bar (0 = first).
    pub(super) fn reorder_tab(&mut self, tab_id: u64, slot: usize, cx: &mut Context<Self>) {
        let Some(from) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let active = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id);
        if crate::layout::reorder(&mut self.tabs, from, slot).is_some() {
            self.active = active.and_then(|id| self.tabs.iter().position(|t| t.id == id));
            self.reveal_active = true;
            self.persist_tabs(cx);
            cx.notify();
        }
    }

    /// Shift+Ctrl+←/→ on the tab bar: one place left or right.
    pub(super) fn nudge_tab(&mut self, tab_id: u64, delta: i32, cx: &mut Context<Self>) {
        let Some(from) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let slot = if delta < 0 {
            from.saturating_sub(1)
        } else {
            from + 2
        };
        self.reorder_tab(tab_id, slot, cx);
    }

    /// Can tab `src` be dropped into a split of the tab holding `target`?
    pub(super) fn can_merge_tab(&self, src: u64, target: u64) -> bool {
        let Some(s) = self.tabs.iter().find(|t| t.id == src) else {
            return false;
        };
        let Some(d) = self.tabs.iter().find(|t| t.root.find(target).is_some()) else {
            return false;
        };
        s.id != d.id && s.kind == TabKind::Terminal && d.kind == TabKind::Terminal
    }

    /// A tab was dropped on the `edge` of pane `target`: its panes join
    /// that tab as a split there.
    pub(super) fn merge_tab_into(
        &mut self,
        tab_id: u64,
        target: u64,
        edge: Edge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_merge_tab(tab_id, target) {
            return;
        }
        let Some(src_ix) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let Some(dst_ix) = self.tabs.iter().position(|t| t.root.find(target).is_some()) else {
            return;
        };
        let src = self.tabs.remove(src_ix);
        let dst_ix = if src_ix < dst_ix { dst_ix - 1 } else { dst_ix };
        let focus = src.focused;
        match self.tabs[dst_ix]
            .root
            .insert_at(target, edge, src.root, fresh_id())
        {
            Ok(()) => {
                self.tabs[dst_ix].focused = focus;
                // Zoom is per tab: the joined panes follow the tab they joined.
                let zoom = self.tabs[dst_ix].zoom;
                for v in self.tabs[dst_ix].terminals() {
                    v.update(cx, |v, cx| v.set_zoom(zoom, cx));
                }
                self.activate_tab(dst_ix, window, cx);
            }
            Err(root) => {
                // Cannot happen (the target was found above); never lose a tab.
                self.tabs.insert(src_ix, Tab { root, ..src });
                self.persist_tabs(cx);
                cx.notify();
            }
        }
    }

    // ---- closed tabs ------------------------------------------------------------

    /// Keep what a closing tab showed so Ctrl+Shift+T can bring it back.
    pub(super) fn remember_closed(&self, tab: &Tab, cx: &mut App) {
        let root = crate::layout::save_tree(&tab.root, &|p| Self::saved_pane(p, cx));
        if !root.is_usable() {
            return;
        }
        let mut history = HashMap::new();
        for p in tab.panes() {
            if let PaneBody::Terminal(v) = &p.body {
                let text = v.read(cx).history_text();
                if !text.trim().is_empty() {
                    history.insert(p.id, text);
                }
            }
        }
        let closed = ClosedTab {
            kind: tab.kind,
            root,
            zoom: tab.zoom,
            history,
        };
        push_closed(&mut cx.default_global::<ClosedTabs>().0, closed);
    }

    pub(super) fn has_closed_tabs(cx: &App) -> bool {
        cx.try_global::<ClosedTabs>()
            .is_some_and(|c| !c.0.is_empty())
    }

    /// Ctrl+Shift+T: the most recently closed tab, as it was, reconnected,
    /// with its old output above a "reconnected" line.
    pub(super) fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(closed) = cx.default_global::<ClosedTabs>().0.pop() else {
            window.toast(Toast::info(t!("tab-nothing-to-reopen")), cx);
            return;
        };
        let profiles = self.profiles.clone();
        let exists = |p: &crate::layout::SavedPane| match (&p.profile, &p.quick) {
            (Some(id), _) => profiles.iter().any(|x| x.id == *id),
            (None, Some(q)) => crate::backend::quick_profile(q).is_ok(),
            _ => false,
        };
        let Some(root) = closed.root.prune(&exists) else {
            // The host was deleted since: try the one before.
            window.toast(Toast::info(t!("tab-reopen-host-gone")), cx);
            return;
        };
        let mut ids = HashMap::new();
        let mut history = closed.history;
        let root = self.build_tree(root, &mut ids, &mut history);
        let focused = root.leaves().first().map(|p| p.id).unwrap_or(0);
        self.tabs.push(Tab {
            id: fresh_id(),
            kind: closed.kind,
            root,
            focused,
            zoom: closed.zoom,
        });
        let new_ix = self.tabs.len() - 1;
        for p in self.tabs[new_ix].root.leaves_mut() {
            p.follows = p.follows.and_then(|old| ids.get(&old).copied());
        }
        self.activate_tab(new_ix, window, cx);
    }

    // ---- zoom ---------------------------------------------------------------------

    /// Zoom shortcuts (`dir` 0 resets): the active terminal tab's text
    /// size. Returns whether a terminal tab took it.
    pub(super) fn zoom_active(&mut self, dir: i32, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.active else {
            return false;
        };
        self.zoom_tab(ix, dir, cx)
    }

    pub(super) fn zoom_tab(&mut self, ix: usize, dir: i32, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get_mut(ix) else {
            return false;
        };
        if tab.kind != TabKind::Terminal {
            return false;
        }
        let pct = if dir == 0 {
            crate::zoom::DEFAULT_ZOOM
        } else {
            crate::zoom::step_zoom(tab.zoom, dir)
        };
        tab.zoom = pct;
        for v in tab.terminals() {
            v.update(cx, |v, cx| v.set_zoom(pct, cx));
        }
        self.show_zoom_hud(pct, cx);
        self.persist_tabs(cx);
        true
    }

    /// Briefly show "120%" over the session.
    fn show_zoom_hud(&mut self, pct: u32, cx: &mut Context<Self>) {
        self.hud_gen += 1;
        let generation = self.hud_gen;
        self.zoom_hud = Some(pct);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1300))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.hud_gen == generation {
                    app.zoom_hud = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Ctrl+wheel inside a terminal.
    pub(super) fn zoom_terminal(&mut self, view: EntityId, dir: i32, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_of_terminal(view) else {
            return;
        };
        if let Some(ix) = self.tabs.iter().position(|t| t.root.find(pane).is_some()) {
            self.zoom_tab(ix, dir, cx);
        }
    }

    /// Default zoom of a new terminal in a tab with this zoom.
    pub(super) fn tab_zoom_of(&self, pane_id: u64) -> u32 {
        self.locate(pane_id)
            .map(|(ix, _)| self.tabs[ix].zoom)
            .unwrap_or(crate::zoom::DEFAULT_ZOOM)
    }
}

impl Tab {
    /// The terminals of every pane in this tab.
    pub(super) fn terminals(&self) -> Vec<Entity<crate::terminal::TerminalView>> {
        self.panes()
            .into_iter()
            .filter_map(|p: &Pane| match &p.body {
                PaneBody::Terminal(v) => Some(v.clone()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ClosedTab, KEEP_CLOSED, push_closed, tab_for_number};
    use crate::app::TabKind;
    use crate::layout::{SavedNode, SavedPane};

    fn closed(n: u64) -> ClosedTab {
        ClosedTab {
            kind: TabKind::Terminal,
            root: SavedNode::Pane(SavedPane {
                quick: Some(format!("u@h{n}")),
                id: Some(n),
                ..Default::default()
            }),
            zoom: 100,
            history: HashMap::from([(n, format!("out {n}"))]),
        }
    }

    #[test]
    fn closed_tabs_come_back_newest_first_and_are_capped() {
        let mut s = Vec::new();
        for n in 0..15 {
            push_closed(&mut s, closed(n));
        }
        assert_eq!(s.len(), KEEP_CLOSED);
        // The five oldest were forgotten.
        assert_eq!(
            s.first()
                .and_then(|c| c.history.get(&5))
                .map(String::as_str),
            Some("out 5")
        );
        let last = s.pop();
        assert_eq!(
            last.as_ref()
                .and_then(|c| c.history.get(&14))
                .map(String::as_str),
            Some("out 14")
        );
        assert_eq!(s.len(), KEEP_CLOSED - 1);
    }

    #[test]
    fn numbered_shortcuts_pick_tabs() {
        // 1..8 are tabs 1..8; 9 is always the last one.
        assert_eq!(tab_for_number(1, 5), Some(0));
        assert_eq!(tab_for_number(5, 5), Some(4));
        assert_eq!(tab_for_number(6, 5), None);
        assert_eq!(tab_for_number(9, 5), Some(4));
        assert_eq!(tab_for_number(9, 12), Some(11));
        assert_eq!(tab_for_number(9, 0), None);
        assert_eq!(tab_for_number(8, 12), Some(7));
        assert_eq!(tab_for_number(0, 3), None);
        assert_eq!(crate::shortcuts::digit("3"), Some(3));
        assert_eq!(crate::shortcuts::digit("a"), None);
        assert_eq!(crate::shortcuts::digit("10"), None);
        assert_eq!(crate::shortcuts::digit("0"), None);
    }
}
