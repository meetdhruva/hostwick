//! More than one window. Every window is a full `HostwickApp` (sidebar,
//! tabs, transfer panel) sharing the one `Backend`, the settings, the
//! vault and the connection pools. Tabs move between windows (menu,
//! dragging out of the tab bar); window positions and which tab lives in
//! which window are saved with the open tabs.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use super::{HostwickApp, Init, Tab};
use crate::backend::Backend;
use crate::layout::{SavedBounds, SavedSession, SavedWindow};
use crate::notify::{Toast, ToastExt as _};
use crate::settings::Settings;

/// One open window.
struct Entry {
    key: u64,
    handle: AnyWindowHandle,
    app: WeakEntity<HostwickApp>,
    /// What the window last saved: its tabs and where it is.
    snapshot: Option<SavedWindow>,
}

/// Every open window, oldest first (the first is "the main window").
#[derive(Clone, Default)]
pub(super) struct Windows {
    entries: Rc<RefCell<Vec<Entry>>>,
    /// The last layout written, to skip writing the same again.
    last: Rc<RefCell<String>>,
}

impl Global for Windows {}

/// The whole app is shutting down: windows closing now are not closed by
/// the person, so their tabs stay in the saved layout.
#[derive(Default)]
struct Quitting(bool);

impl Global for Quitting {}

pub(crate) fn mark_quitting(cx: &mut App) {
    cx.set_global(Quitting(true));
}

fn quitting(cx: &App) -> bool {
    cx.try_global::<Quitting>().is_some_and(|q| q.0)
}

fn registry(cx: &App) -> Windows {
    cx.try_global::<Windows>().cloned().unwrap_or_default()
}

fn ensure_registry(cx: &mut App) -> Windows {
    if cx.try_global::<Windows>().is_none() {
        cx.set_global(Windows::default());
    }
    registry(cx)
}

/// The rectangle covering every display (left, top, right, bottom).
pub(crate) fn screens_union(cx: &App) -> Option<(f32, f32, f32, f32)> {
    cx.displays()
        .iter()
        .map(|d| d.bounds())
        .map(|b| {
            (
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.origin.x + b.size.width),
                f32::from(b.origin.y + b.size.height),
            )
        })
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// Move a window's top-left corner so that its title bar stays reachable:
/// at least `GRAB` px of it inside the screens, and never above them.
pub(super) fn keep_reachable(
    origin: (f32, f32),
    size: (f32, f32),
    screens: Option<(f32, f32, f32, f32)>,
) -> (f32, f32) {
    const GRAB: f32 = 160.0;
    let Some((l, t, r, b)) = screens else {
        return origin;
    };
    let x = origin.0.clamp(l + GRAB - size.0, r - GRAB);
    let y = origin.1.clamp(t, (b - 48.0).max(t));
    (x, y)
}

/// Window options for saved bounds (centered when none are usable).
pub(crate) fn options_for(saved: Option<SavedBounds>, cx: &App) -> WindowOptions {
    let bounds = saved
        .and_then(|b| b.usable(screens_union(cx)))
        .map(|b| {
            // GPUI's X11 backend opens a window 2 px to the right of what it
            // is asked for, then reports the real position back: without
            // this, a window would creep right by 2 px at every start.
            let dx = if cfg!(target_os = "linux") { 2.0 } else { 0.0 };
            let r = Bounds::new(point(px(b.x - dx), px(b.y)), size(px(b.w), px(b.h)));
            if b.maximized {
                WindowBounds::Maximized(r)
            } else {
                WindowBounds::Windowed(r)
            }
        })
        .unwrap_or_else(|| WindowBounds::centered(size(px(1280.0), px(800.0)), cx));
    WindowOptions {
        window_bounds: Some(bounds),
        // Matches StartupWMClass in hostwick.desktop (Linux taskbar icon).
        app_id: Some("hostwick".into()),
        titlebar: Some(TitlebarOptions {
            title: Some("Hostwick".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The saved form of a window's current bounds.
pub(super) fn saved_bounds(window: &Window) -> SavedBounds {
    let (b, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
    };
    SavedBounds {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        w: f32::from(b.size.width),
        h: f32::from(b.size.height),
        maximized,
    }
}

/// The windows beyond the first that were open last time. Read before the
/// first window opens: it saves the layout as soon as it exists.
pub(crate) fn saved_extra(backend: &Backend, cx: &App) -> Vec<SavedWindow> {
    if !Settings::get(cx).restore_tabs {
        return Vec::new();
    }
    backend
        .store
        .setting(crate::layout::TABS_KEY)
        .ok()
        .flatten()
        .filter(|j| !j.is_empty())
        .map(|json| SavedSession::from_json(&json).windows)
        .unwrap_or_default()
}

/// Open those windows.
pub(crate) fn restore_extra(backend: &Arc<Backend>, windows: Vec<SavedWindow>, cx: &mut App) {
    for w in windows {
        let options = options_for(w.bounds, cx);
        let backend = backend.clone();
        let init = Init::Extra(w);
        if let Err(e) = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| HostwickApp::new(backend, window, cx, init))
        }) {
            tracing::warn!(error = %e, "cannot reopen a window");
        }
    }
}

/// Route a prompt (host key, password, …) to the window in front.
pub(crate) fn route_prompt(prompt: crate::backend::Prompt, cx: &mut App) {
    let reg = registry(cx);
    let target = {
        let entries = reg.entries.borrow();
        let active = cx.active_window();
        entries
            .iter()
            .find(|e| Some(e.handle) == active)
            .or_else(|| entries.first())
            .map(|e| (e.handle, e.app.clone()))
    };
    let Some((handle, app)) = target else {
        return;
    };
    let _ = handle.update(cx, move |_, window, cx| {
        let _ = app.update(cx, |app, cx| app.enqueue_prompt(prompt, window, cx));
    });
}

impl HostwickApp {
    /// Add this window to the list of windows; called once from `new`.
    pub(super) fn register_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        let reg = ensure_registry(cx);
        reg.entries.borrow_mut().push(Entry {
            key: self.win_key,
            handle: window.window_handle(),
            app: cx.entity().downgrade(),
            snapshot: None,
        });
        let key = self.win_key;
        // The window closes: its tabs are closed tabs now (unless the whole
        // app is quitting, when they stay in the saved layout).
        cx.on_release(move |app, cx| {
            let reg = registry(cx);
            let others = reg.entries.borrow().iter().any(|e| e.key != key);
            if others && !quitting(cx) {
                for t in app.tabs.iter().rev() {
                    app.remember_closed(t, cx);
                }
                reg.entries.borrow_mut().retain(|e| e.key != key);
                write_layout(&app.backend, cx);
            }
        })
        .detach();
    }

    /// Is this the main (oldest) window? It alone runs the background
    /// chores and announces finished transfers.
    pub(super) fn is_primary(&self, cx: &App) -> bool {
        registry(cx)
            .entries
            .borrow()
            .first()
            .is_none_or(|e| e.key == self.win_key)
    }

    /// Remember where this window is (after a pause, while it is dragged
    /// or resized).
    pub(super) fn note_bounds(&mut self, window: &Window, cx: &mut Context<Self>) {
        let b = saved_bounds(window);
        if self.bounds == Some(b) {
            return;
        }
        self.bounds = Some(b);
        self.bounds_gen += 1;
        let generation = self.bounds_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(600))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.bounds_gen == generation {
                    app.persist_tabs(cx);
                }
            });
        })
        .detach();
    }

    /// Store this window's part of the layout and write the whole thing.
    pub(super) fn persist_window(&self, tabs: SavedWindow, cx: &App) {
        let reg = registry(cx);
        if let Some(e) = reg
            .entries
            .borrow_mut()
            .iter_mut()
            .find(|e| e.key == self.win_key)
        {
            e.snapshot = Some(tabs);
        }
        write_layout(&self.backend, cx);
    }

    /// The other windows, for "Move to window".
    pub(super) fn other_windows(&self, cx: &App) -> Vec<(u64, SharedString)> {
        let reg = registry(cx);
        let entries = reg.entries.borrow();
        entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.key != self.win_key)
            .map(|(i, e)| {
                let what = e
                    .app
                    .upgrade()
                    .and_then(|a| {
                        let a = a.read(cx);
                        a.active
                            .and_then(|ix| a.tabs.get(ix))
                            .map(|t| Self::tab_label(t, cx))
                    })
                    .unwrap_or_default();
                let n = i + 1;
                let name: SharedString = if what.is_empty() {
                    t!("window-name", n = n)
                } else {
                    t!("window-name-with-tab", n = n, tab = what)
                };
                (e.key, name)
            })
            .collect()
    }

    /// Take a tab out of this window (to move it elsewhere).
    fn take_tab(&mut self, tab_id: u64, cx: &mut Context<Self>) -> Option<Tab> {
        let ix = self.tabs.iter().position(|t| t.id == tab_id)?;
        let mut tab = self.tabs.remove(ix);
        // Its bell marker belongs to this window's state.
        self.clear_bell(tab_id);
        // A file browser's connection stays with this window's views: it
        // reconnects in the new one, at the same folder.
        for p in tab.root.leaves_mut() {
            p.leave_window(cx);
        }
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a >= self.tabs.len() => Some(self.tabs.len() - 1),
            Some(a) if a > ix => Some(a - 1),
            other => other,
        };
        self.unlink_dead_follows();
        if let Some(a) = self.active {
            self.reveal_active = true;
            let _ = a;
        }
        self.persist_tabs(cx);
        cx.notify();
        Some(tab)
    }

    /// Put a tab from another window into this one and show it.
    pub(super) fn adopt_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        for v in tab.terminals() {
            let sub = cx.subscribe_in(&v, window, Self::on_terminal_event);
            self._subs.push(sub);
            v.update(cx, |v, cx| v.window_changed(cx));
        }
        self.tabs.push(tab);
        let ix = self.tabs.len() - 1;
        self.unlink_dead_follows();
        self.activate_tab(ix, window, cx);
    }

    /// "Move to new window".
    pub(super) fn move_to_new_window(
        &mut self,
        tab_id: u64,
        at: Option<Point<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // The only tab of a window gains nothing by leaving it.
        if self.tabs.len() < 2 {
            return;
        }
        let size = window.bounds().size;
        let origin = at
            .map(|p| point(p.x - size.width / 2.0, p.y - px(18.0)))
            .unwrap_or_else(|| window.bounds().origin + point(px(36.0), px(36.0)));
        // Dropped near an edge: keep the new window's title bar on screen.
        let (x, y) = keep_reachable(
            (f32::from(origin.x), f32::from(origin.y)),
            (f32::from(size.width), f32::from(size.height)),
            screens_union(cx),
        );
        let origin = point(px(x), px(y));
        let bounds = Bounds::new(origin, size);
        let Some(tab) = self.take_tab(tab_id, cx) else {
            return;
        };
        let backend = self.backend.clone();
        let mut options = options_for(None, cx);
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
        let slot = Rc::new(RefCell::new(Some(tab)));
        let slot2 = slot.clone();
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut app = HostwickApp::new(backend, window, cx, Init::Empty);
                if let Some(tab) = slot2.borrow_mut().take() {
                    app.adopt_tab(tab, window, cx);
                }
                app
            })
        });
        if let Err(e) = opened {
            tracing::warn!(error = %e, "cannot open a window");
            // Never lose the tab: it comes back here.
            if let Some(tab) = slot.borrow_mut().take() {
                self.tabs.push(tab);
                self.active = Some(self.tabs.len() - 1);
                cx.notify();
            }
        }
    }

    /// "Move to window ▸ N".
    pub(super) fn move_to_window(
        &mut self,
        tab_id: u64,
        key: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reg = registry(cx);
        let target = reg
            .entries
            .borrow()
            .iter()
            .find(|e| e.key == key)
            .map(|e| (e.handle, e.app.clone()));
        let Some((handle, app)) = target else {
            window.toast(Toast::info(t!("window-gone")), cx);
            return;
        };
        let Some(tab) = self.take_tab(tab_id, cx) else {
            return;
        };
        let slot = Rc::new(RefCell::new(Some(tab)));
        let slot2 = slot.clone();
        let done = handle.update(cx, move |_, window, cx| {
            let _ = app.update(cx, |app, cx| {
                if let Some(tab) = slot2.borrow_mut().take() {
                    app.adopt_tab(tab, window, cx);
                }
            });
            window.activate_window();
        });
        if done.is_err()
            && let Some(tab) = slot.borrow_mut().take()
        {
            self.tabs.push(tab);
            self.active = Some(self.tabs.len() - 1);
            cx.notify();
        }
    }

    /// A tab was released away from its place. Outside every window: move
    /// it to a window of its own; over another window of ours: into that.
    pub(super) fn tab_released(
        &mut self,
        tab_id: u64,
        pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging_tab != Some(tab_id) {
            return;
        }
        let vp = window.viewport_size();
        let inside =
            pos.x >= px(0.0) && pos.y >= px(0.0) && pos.x <= vp.width && pos.y <= vp.height;
        self.dragging_tab = None;
        if inside {
            return;
        }
        let screen = window.bounds().origin + pos;
        let reg = registry(cx);
        let over = reg
            .entries
            .borrow()
            .iter()
            .filter(|e| e.key != self.win_key)
            .find(|e| {
                e.handle
                    .update(cx, |_, w, _| w.bounds().contains(&screen))
                    .unwrap_or(false)
            })
            .map(|e| e.key);
        match over {
            Some(key) => self.move_to_window(tab_id, key, window, cx),
            None => self.move_to_new_window(tab_id, Some(screen), window, cx),
        }
    }
}

/// Compose every window's snapshot into the saved layout and store it.
fn write_layout(backend: &Backend, cx: &App) {
    let reg = registry(cx);
    let json = if Settings::get(cx).restore_tabs {
        let entries = reg.entries.borrow();
        let mut it = entries.iter().filter_map(|e| e.snapshot.clone());
        let first = it.next().unwrap_or_default();
        SavedSession {
            tabs: first.tabs,
            active: first.active,
            bounds: first.bounds,
            windows: it.filter(|w| !w.tabs.is_empty()).collect(),
        }
        .to_json()
    } else {
        String::new()
    };
    if *reg.last.borrow() == json {
        return;
    }
    match backend.store.set_setting(crate::layout::TABS_KEY, &json) {
        Ok(()) => *reg.last.borrow_mut() = json,
        Err(e) => tracing::warn!(error = %e, "could not save open tabs"),
    }
}

#[cfg(test)]
mod tests {
    use super::keep_reachable;

    const SCREEN: Option<(f32, f32, f32, f32)> = Some((0.0, 0.0, 1400.0, 900.0));

    #[test]
    fn a_dropped_window_keeps_its_title_bar_reachable() {
        let size = (1280.0, 800.0);
        // Already fine: untouched.
        assert_eq!(keep_reachable((60.0, 50.0), size, SCREEN), (60.0, 50.0));
        // Dropped near the bottom: pulled up so the title bar stays visible.
        assert_eq!(
            keep_reachable((-618.0, 862.0), size, SCREEN),
            (-618.0, 852.0)
        );
        // Far right and above the screen.
        assert_eq!(
            keep_reachable((5000.0, -300.0), size, SCREEN),
            (1240.0, 0.0)
        );
        // Too far left: at least a strip stays inside.
        assert_eq!(
            keep_reachable((-5000.0, 10.0), size, SCREEN),
            (-1120.0, 10.0)
        );
        // Unknown screens: left alone.
        assert_eq!(keep_reachable((-9.0, -9.0), size, None), (-9.0, -9.0));
    }
}
