//! Accessibility helpers: text that screen readers can read, and context
//! menus opened from the keyboard.
//!
//! GPUI reports an element to the platform accessibility API (UI Automation
//! on Windows, NSAccessibility on macOS, AT-SPI on Linux, all via AccessKit)
//! only when it has an id and a role. Plain strings have neither, so static
//! text that matters (dialog bodies, hints, errors, empty states) goes
//! through [`text`].

use std::cell::Cell;
use std::rc::Rc;

use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Text exposed to assistive technology as a label. Its id is the text
/// itself, so siblings with different texts never collide.
pub fn text(s: impl Into<SharedString>) -> Text {
    let s: SharedString = s.into();
    Text::new(ElementId::Name(s.clone()), s)
}

/// A button in a dialog's section list: left-aligned, and growing to a
/// second line when a translation is longer than the list is wide.
pub fn section_button(
    id: impl Into<ElementId>,
    label: SharedString,
    current: bool,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    Button::new(id)
        .ghost()
        .small()
        .w_full()
        .h_auto()
        .min_h(px(24.0))
        .py(px(3.0))
        .accessibility_label(label.clone())
        .child(div().w_full().text_left().whitespace_normal().child(label))
        .toggled(current)
        .when(current, |b| {
            b.bg(theme.accent).text_color(theme.accent_foreground)
        })
}

/// A labelled checkbox. The library's own label sits in a box exactly one
/// line high that clips, which cuts the tails off g, p and y with the usual
/// UI fonts; a little room underneath keeps them whole. The label is also
/// the accessible name.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
    cx: &App,
) -> Checkbox {
    let label: SharedString = label.into();
    let muted = cx.theme().muted_foreground;
    Checkbox::new(id)
        .accessibility_label(label.clone())
        .disabled(disabled)
        .child(
            div()
                .pb(px(3.0))
                .when(disabled, |d| d.text_color(muted))
                .child(label),
        )
}

/// Where a keyboard-opened context menu should appear: the bounds of the
/// item it is for, recorded while painting.
#[derive(Clone, Default)]
pub struct MenuAnchor(Rc<Cell<Option<Bounds<Pixels>>>>);

impl MenuAnchor {
    /// An invisible child that records its parent's bounds each frame.
    /// The parent must be `relative()`.
    pub fn track(&self) -> impl IntoElement {
        let cell = self.0.clone();
        canvas(move |bounds, _, _| cell.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }

    pub fn clear(&self) {
        self.0.set(None);
    }

    /// Open the context menu of the tracked item, as a right-click on it
    /// would (Shift+F10 or the Menu key).
    pub fn open(&self, window: &mut Window, cx: &mut App) -> bool {
        let Some(b) = self.0.get() else {
            return false;
        };
        // Near the start of the item, where a right-click would land.
        let at = point(
            b.origin.x + (b.size.width / 4.0).min(px(40.0)),
            b.origin.y + b.size.height / 2.0,
        );
        open_context_menu_at(at, window, cx);
        true
    }
}

/// Shift+F10 or the Menu (Apps) key: the keyboard way to a context menu.
pub fn is_menu_key(ks: &Keystroke) -> bool {
    let m = ks.modifiers;
    (ks.key == "menu" && !m.control && !m.alt && !m.platform)
        || (ks.key == "f10" && m.shift && !m.control && !m.alt && !m.platform)
}

/// The library's context menus open on a right mouse button press; send
/// one (after the current event) at `at`.
pub fn open_context_menu_at(at: Point<Pixels>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let modifiers = Modifiers::default();
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Right,
                position: at,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Right,
                position: at,
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    });
}
