//! The shortcut cheat sheet (F1, Ctrl+/, the ? button): every shortcut,
//! grouped, searchable, written the way this platform writes keys.

use crate::zoom::px;
use gpui_kit::accesskit::Role;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, IconName, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::HostwickApp;
use crate::shortcuts::{Group, Sc};

pub(crate) struct ShortcutsView {
    search: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl ShortcutsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("keys-search")));
        let subs = vec![cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                cx.notify();
            }
        })];
        Self {
            search,
            _subs: subs,
        }
    }
}

/// Does a shortcut match every word of the search (label or keys)?
fn matches(label: &str, keys: &str, needle: &str) -> bool {
    let hay = format!("{label} {keys}").to_lowercase();
    needle
        .to_lowercase()
        .split_whitespace()
        .all(|w| hay.contains(w))
}

impl Render for ShortcutsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let needle = self.search.read(cx).value().to_string();
        let mut body = div().flex().flex_col().gap(px(2.0));
        let mut any = false;
        for g in Group::ALL {
            let items: Vec<(SharedString, &'static str)> = Sc::ALL
                .iter()
                .filter(|s| s.group() == g)
                .map(|s| (s.label(), s.keys()))
                .filter(|(l, k)| matches(l, k, &needle))
                .collect();
            if items.is_empty() {
                continue;
            }
            any = true;
            body = body.child(
                div()
                    .pt(px(10.0))
                    .pb(px(2.0))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(crate::a11y::text(g.label())),
            );
            for (i, (label, keys)) in items.into_iter().enumerate() {
                body = body.child(
                    div()
                        .id(SharedString::from(format!("sc-{g:?}-{i}")))
                        .role(Role::Row)
                        .aria_label(format!("{label}: {keys}"))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .py(px(3.0))
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .hover(|s| s.bg(theme.list_hover))
                        .text_size(px(13.0))
                        .child(div().flex_1().min_w_0().whitespace_normal().child(label))
                        .child(
                            div()
                                .flex_none()
                                .max_w(relative(0.5))
                                .text_right()
                                .whitespace_normal()
                                .font_family(crate::terminal::monospace_family())
                                .text_size(px(12.0))
                                .text_color(theme.muted_foreground)
                                .child(keys),
                        ),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                Input::new(&self.search)
                    .prefix(gpui_kit::component::Icon::new(IconName::Search))
                    .cleanable(true)
                    .aria_label(t!("keys-search")),
            )
            .child(
                div()
                    .id("shortcut-list")
                    .role(Role::Table)
                    .aria_label(t!("keys-shortcuts"))
                    .h(px(460.0))
                    .overflow_y_scroll()
                    .pr(px(6.0))
                    .when(any, |d| d.child(body))
                    .when(!any, |d| {
                        d.child(
                            div()
                                .pt(px(20.0))
                                .text_center()
                                .text_color(theme.muted_foreground)
                                .child(crate::a11y::text(t!("palette-nothing"))),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .whitespace_normal()
                    .child(crate::a11y::text(t!("keys-note"))),
            )
    }
}

impl HostwickApp {
    pub(crate) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let view = cx.new(|cx| ShortcutsView::new(window, cx));
        let first = std::rc::Rc::new(std::cell::Cell::new(true));
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let input = view.read(cx).search.clone();
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            dialog
                .title(crate::a11y::text(t!("keys-shortcuts")))
                .w(px(640.0))
                .margin_top(px(40.0))
                .child(view.clone())
                .footer(
                    div().flex().child(
                        Button::new("close")
                            .label(t!("button-close"))
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    ),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn search_matches_labels_and_keys() {
        assert!(matches("Split right", "Ctrl+Shift+D", "split"));
        assert!(matches("Split right", "Ctrl+Shift+D", "ctrl+shift+d"));
        assert!(matches("Split right", "Ctrl+Shift+D", "right ctrl"));
        assert!(!matches("Split right", "Ctrl+Shift+D", "files"));
        assert!(matches("Anything", "F1", ""));
    }
}
