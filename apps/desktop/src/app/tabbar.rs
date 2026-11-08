//! The tab bar: tabs you can reorder by dragging (with a drop marker), close
//! with the middle button, scroll when there are many, list with "⌄", and
//! double-click the empty part of to open the command palette.

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{HostwickApp, PaneBody};
use crate::util::keys;
use crate::zoom::px;

/// A tab being dragged: to another place in the bar, onto the edge of a
/// pane (to become a split), or out of the window (to become its own).
#[derive(Clone)]
pub(super) struct DraggedTab {
    pub id: u64,
    pub title: SharedString,
}

/// What follows the pointer while a tab is dragged.
struct TabGhost {
    title: SharedString,
}

impl Render for TabGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(theme.tab_active)
            .text_color(theme.tab_active_foreground)
            .border_1()
            .border_color(theme.primary)
            .text_size(px(13.0))
            .max_w(px(240.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(self.title.clone())
    }
}

/// The gap of the bar a pointer at `x` is over, given each tab's centre.
pub(super) fn slot_at(x: f32, centres: &[f32]) -> usize {
    centres.iter().position(|c| x < *c).unwrap_or(centres.len())
}

impl HostwickApp {
    pub(super) fn render_tabs(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active = self.active;
        let bar_focused = self.tabs_focus.is_focused(window);
        let anchor = self.tab_anchor.clone();
        anchor.clear();
        let count = self.tabs.len();
        let dragging = cx.has_active_drag();
        let drop_slot = if dragging { self.tab_drop } else { None };
        if self.reveal_active {
            self.reveal_active = false;
            if let Some(a) = active {
                self.tab_scroll.scroll_to_item(a);
            }
        }
        let overflowing = f32::from(self.tab_scroll.max_offset().x) > 1.0 || count > 8;
        let entity = cx.entity().downgrade();

        let tabs = self.tabs.iter().enumerate().map(|(ix, t)| {
            let is_active = active == Some(ix);
            let panes = t.panes();
            let alive = panes.iter().all(|p| Self::pane_alive(p, cx));
            let title = Self::tab_label(t, cx);
            let pending = panes.iter().all(|p| matches!(p.body, PaneBody::Pending));
            let tab_id = t.id;
            let state = if pending {
                t!("tab-state-not-connected")
            } else if alive {
                t!("tab-state-connected")
            } else {
                t!("tab-state-disconnected")
            };
            let close_label = t!("tab-close", title = title.clone());
            let env = super::termx::tab_env(t);
            let bell = self.tab_has_bell(t.id, window);
            let mut spoken = t!(
                "tab-spoken",
                title = title.clone(),
                state = state,
                index = ix + 1,
                count = count
            )
            .to_string();
            if let Some(name) = crate::util::environment_name(&env) {
                spoken = format!("{spoken}, {name}");
            }
            if bell {
                spoken = format!("{spoken}, {}", t!("tx-tab-bell"));
            }
            let marker = |left: bool| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(3.0))
                    .bg(theme.primary)
                    .when(left, |d| d.left_0())
                    .when(!left, |d| d.right_0())
            };
            div()
                .id(("tab", t.id))
                .role(Role::Tab)
                .aria_label(spoken)
                .aria_selected(is_active)
                .when(is_active, |d| {
                    d.aria_active_descendant().child(anchor.track())
                })
                .when(is_active && bar_focused, |d| {
                    d.border_2().border_color(theme.ring)
                })
                .relative()
                .flex_none()
                .h_full()
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .max_w(px(240.0))
                .text_size(px(13.0))
                .border_r_1()
                .border_color(theme.border)
                .when(is_active, |d| {
                    d.bg(theme.tab_active)
                        .text_color(theme.tab_active_foreground)
                })
                .when(!is_active, |d| {
                    d.text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.tab_active.opacity(0.5)))
                })
                .on_click(cx.listener(move |app, _, window, cx| {
                    if let Some(ix) = app.tabs.iter().position(|t| t.id == tab_id) {
                        app.activate_tab(ix, window, cx);
                    }
                }))
                // Middle click closes, as in a browser.
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(move |app, _, _, cx| {
                        if let Some(ix) = app.tabs.iter().position(|t| t.id == tab_id) {
                            app.close_tab(ix, cx);
                        }
                    }),
                )
                .on_drag(
                    DraggedTab {
                        id: tab_id,
                        title: title.clone(),
                    },
                    |d, _, _, cx| {
                        cx.new(|_| TabGhost {
                            title: d.title.clone(),
                        })
                    },
                )
                // Released outside the window: a window of its own (or the
                // window it was dropped on).
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(move |app, ev: &MouseUpEvent, window, cx| {
                        app.tab_released(tab_id, ev.position, window, cx);
                    }),
                )
                // The host environment's colour along the top.
                .when_some(crate::util::environment_color(&env), |d, color| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(3.0))
                            .bg(color),
                    )
                })
                .when(drop_slot == Some(ix), |d| d.child(marker(true)))
                .when(drop_slot == Some(count) && ix + 1 == count, |d| {
                    d.child(marker(false))
                })
                .when(bell, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_size(px(12.0))
                            .text_color(theme.warning)
                            .child("🔔"),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .size(px(7.0))
                        .rounded_full()
                        .bg(if pending {
                            theme.muted_foreground
                        } else if alive {
                            theme.success
                        } else {
                            theme.danger
                        }),
                )
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .context_menu(Self::tab_menu(entity.clone(), t.id))
                .child(
                    Button::new(("close", t.id))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tab_stop(false)
                        .accessibility_label(close_label)
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(ix) = app.tabs.iter().position(|t| t.id == tab_id) {
                                app.close_tab(ix, cx);
                            }
                        })),
                )
        });

        let list_entity = entity.clone();
        div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                // Tabs and the empty space after them: one drop target.
                div()
                    .id("tabs-area")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .on_drag_move(cx.listener(|app, ev: &DragMoveEvent<DraggedTab>, _, cx| {
                        if !ev.bounds.contains(&ev.event.position) {
                            if app.tab_drop.take().is_some() {
                                cx.notify();
                            }
                            return;
                        }
                        let centres: Vec<f32> = (0..app.tabs.len())
                            .filter_map(|i| app.tab_scroll.bounds_for_item(i))
                            .map(|b| f32::from(b.center().x))
                            .collect();
                        let slot = slot_at(f32::from(ev.event.position.x), &centres);
                        if app.tab_drop != Some(slot) {
                            app.tab_drop = Some(slot);
                            cx.notify();
                        }
                    }))
                    .on_drop(cx.listener(|app, d: &DraggedTab, _, cx| {
                        if let Some(slot) = app.tab_drop.take() {
                            app.reorder_tab(d.id, slot, cx);
                        }
                    }))
                    .child(
                        div()
                            .id("tabs")
                            .track_focus(&self.tabs_focus)
                            .track_scroll(&self.tab_scroll)
                            .role(Role::TabList)
                            .aria_label(t!("tabs-label"))
                            .aria_description(t!("tabs-keys"))
                            .on_key_down(cx.listener(Self::on_tabs_key))
                            // The mouse wheel scrolls the bar sideways.
                            .on_scroll(cx.listener(|app, ev: &ScrollEvent, window, cx| {
                                let d = ev.delta.pixel_delta(window.line_height());
                                if d.x == px(0.0) && d.y != px(0.0) {
                                    let mut o = app.tab_scroll.offset();
                                    let max = f32::from(app.tab_scroll.max_offset().x).abs();
                                    let x = (f32::from(o.x) + f32::from(d.y)).clamp(-max, 0.0);
                                    o.x = px(x);
                                    app.tab_scroll.set_offset(o);
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            }))
                            .flex_initial()
                            .min_w_0()
                            .flex()
                            .h_full()
                            .overflow_x_scroll()
                            .children(tabs),
                    )
                    .child(
                        div()
                            .id("tabs-filler")
                            .flex_1()
                            .min_w(px(24.0))
                            .h_full()
                            .relative()
                            .when(drop_slot == Some(count) && count == 0, |d| {
                                d.border_l_2().border_color(theme.primary)
                            })
                            .on_click(cx.listener(|app, ev: &ClickEvent, window, cx| {
                                if ev.click_count() >= 2 {
                                    app.open_palette(window, cx);
                                }
                            })),
                    ),
            )
            .when(overflowing && count > 0, |d| {
                d.child(
                    Button::new("tab-list")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronDown)
                        .tooltip(t!("tabs-list"))
                        .accessibility_label(t!("tabs-list"))
                        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
                            let Some(app) = list_entity.upgrade() else {
                                return menu;
                            };
                            let (labels, active) = {
                                let a = app.read(cx);
                                (
                                    a.tabs
                                        .iter()
                                        .map(|t| (t.id, Self::tab_label(t, cx)))
                                        .collect::<Vec<_>>(),
                                    a.active.and_then(|i| a.tabs.get(i)).map(|t| t.id),
                                )
                            };
                            labels.into_iter().fold(menu, |menu, (id, label)| {
                                let weak = list_entity.clone();
                                menu.item(
                                    PopupMenuItem::new(label)
                                        .checked(active == Some(id))
                                        .on_click(move |_, window, cx| {
                                            let _ = weak.update(cx, |a, cx| {
                                                if let Some(ix) =
                                                    a.tabs.iter().position(|t| t.id == id)
                                                {
                                                    a.activate_tab(ix, window, cx);
                                                }
                                            });
                                        }),
                                )
                            })
                        }),
                )
            })
            .child(
                div().px(px(8.0)).child(
                    Button::new("broadcast")
                        .small()
                        .label(if self.broadcast {
                            t!("broadcast-on")
                        } else {
                            t!("broadcast")
                        })
                        .when(self.broadcast, |b| b.warning())
                        .tooltip(t!("broadcast-tooltip", keys = keys("Ctrl+Shift+B", "⌘B")))
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.broadcast = !app.broadcast;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }
}

/// The pane-edge drop zone highlight (drawn over a pane while a tab hovers
/// over one of its sides).
pub(super) fn zone_overlay(edge: crate::layout::Edge) -> AnyElement {
    use crate::layout::Edge;
    // A blue that shows on the dark terminal and on light headers alike
    // (the theme's primary colour is near-black in the light theme).
    let color = hsla(0.6, 0.9, 0.58, 1.0);
    let base = div()
        .absolute()
        .bg(color.opacity(0.28))
        .border_2()
        .border_color(color);
    match edge {
        Edge::Left => base.left_0().top_0().bottom_0().w(relative(0.5)),
        Edge::Right => base.right_0().top_0().bottom_0().w(relative(0.5)),
        Edge::Top => base.top_0().left_0().right_0().h(relative(0.5)),
        Edge::Bottom => base.bottom_0().left_0().right_0().h(relative(0.5)),
    }
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::slot_at;

    #[test]
    fn drop_slot_follows_the_pointer() {
        let centres = [50.0, 150.0, 250.0];
        assert_eq!(slot_at(10.0, &centres), 0);
        assert_eq!(slot_at(49.9, &centres), 0);
        assert_eq!(slot_at(50.0, &centres), 1);
        assert_eq!(slot_at(200.0, &centres), 2);
        assert_eq!(slot_at(900.0, &centres), 3);
        assert_eq!(slot_at(5.0, &[]), 0);
    }
}
