//! Notifications ("toasts"), drawn above dialogs.
//!
//! The component library's notification layer paints below its dialogs, so a
//! notice raised while a dialog is open (a failed import, a sync result, a
//! connection notice behind the host editor) ended up partly hidden. These
//! are painted in a deferred layer with a higher priority than dialogs, are
//! announced to screen readers (role "alert", assertive live region) and
//! close by themselves after a while or with their close button.

use std::time::Duration;

use gpui_kit::accesskit::{Live, Role};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Above dialogs (priority 0) and their popups.
const LAYER_PRIORITY: usize = 50;
/// At most this many on screen; older ones make room.
const MAX_VISIBLE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

/// A notification to show.
#[derive(Debug, Clone)]
pub struct Toast {
    level: Level,
    title: Option<SharedString>,
    text: SharedString,
}

impl Toast {
    pub fn info(text: impl Into<SharedString>) -> Self {
        Self::new(Level::Info, text)
    }
    pub fn success(text: impl Into<SharedString>) -> Self {
        Self::new(Level::Success, text)
    }
    pub fn warning(text: impl Into<SharedString>) -> Self {
        Self::new(Level::Warning, text)
    }
    pub fn error(text: impl Into<SharedString>) -> Self {
        Self::new(Level::Error, text)
    }

    fn new(level: Level, text: impl Into<SharedString>) -> Self {
        Self {
            level,
            title: None,
            text: text.into(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How long it stays: errors and warnings longer than confirmations.
    fn lifetime(&self) -> Duration {
        let base = match self.level {
            Level::Info | Level::Success => 6,
            Level::Warning | Level::Error => 12,
        };
        // Long texts take longer to read (and to be read out).
        let extra = (self.text.chars().count() / 40).min(10);
        Duration::from_secs(base + extra as u64)
    }
}

struct Shown {
    id: u64,
    toast: Toast,
}

#[derive(Default)]
struct Toasts {
    shown: Vec<Shown>,
    next: u64,
}

impl Global for Toasts {}

/// `window.toast(Toast::error("…"), cx)`.
pub trait ToastExt {
    fn toast(&mut self, toast: Toast, cx: &mut App);
}

impl ToastExt for Window {
    fn toast(&mut self, toast: Toast, cx: &mut App) {
        push(toast, cx);
        self.refresh();
    }
}

/// Show a notification in every window (there is one).
pub fn push(toast: Toast, cx: &mut App) {
    let lifetime = toast.lifetime();
    let toasts = cx.default_global::<Toasts>();
    // The same text again replaces the old one instead of stacking.
    toasts
        .shown
        .retain(|s| s.toast.text != toast.text || s.toast.level != toast.level);
    let id = toasts.next;
    toasts.next += 1;
    toasts.shown.push(Shown { id, toast });
    let excess = toasts.shown.len().saturating_sub(MAX_VISIBLE);
    toasts.shown.drain(..excess);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(lifetime).await;
        cx.update(|cx| dismiss(id, cx));
    })
    .detach();
}

fn dismiss(id: u64, cx: &mut App) {
    if let Some(t) = cx.try_global::<Toasts>()
        && t.shown.iter().any(|s| s.id == id)
    {
        cx.global_mut::<Toasts>().shown.retain(|s| s.id != id);
        cx.refresh_windows();
    }
}

/// The notification layer; render it once in the root view.
pub fn layer(window: &Window, cx: &App) -> Option<AnyElement> {
    let toasts = cx.try_global::<Toasts>()?;
    if toasts.shown.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let viewport = window.viewport_size();
    let items: Vec<AnyElement> = toasts
        .shown
        .iter()
        .rev()
        .map(|s| {
            let id = s.id;
            let (accent, kind) = match s.toast.level {
                Level::Info => (theme.info, t!("toast-info")),
                Level::Success => (theme.success, t!("toast-success")),
                Level::Warning => (theme.warning, t!("toast-warning")),
                Level::Error => (theme.danger, t!("toast-error")),
            };
            let title = s.toast.title.clone();
            let text = s.toast.text.clone();
            let spoken = match &title {
                Some(t) => format!("{kind}: {t}. {text}"),
                None => format!("{kind}: {text}"),
            };
            let assertive = matches!(s.toast.level, Level::Error | Level::Warning);
            div()
                .id(("toast", id))
                .role(Role::Alert)
                .aria_label(spoken.clone())
                .aria_value(spoken)
                .a11y_synthetic_children(move |b| {
                    b.parent_node().set_live(if assertive {
                        Live::Assertive
                    } else {
                        Live::Polite
                    });
                })
                .occlude()
                .w(px(380.0))
                .flex()
                .gap(px(10.0))
                .p(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_lg()
                .text_size(px(13.0))
                // The colour says what kind of notice it is.
                .child(div().w(px(4.0)).flex_none().rounded(px(2.0)).bg(accent))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .when_some(title, |d, t| {
                            d.child(div().font_weight(FontWeight::SEMIBOLD).child(t))
                        })
                        .child(div().whitespace_normal().child(text)),
                )
                .child(
                    Button::new(("toast-close", id))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .accessibility_label(t!("toast-close"))
                        .on_click(move |_, _, cx| dismiss(id, cx)),
                )
                .into_any_element()
        })
        .collect();
    Some(
        deferred(
            anchored().position(point(px(0.0), px(0.0))).child(
                div()
                    .w(viewport.width)
                    .flex()
                    .justify_end()
                    .pt(px(16.0))
                    .pr(px(16.0))
                    .child(div().flex().flex_col().gap(px(8.0)).children(items)),
            ),
        )
        .with_priority(LAYER_PRIORITY)
        .into_any_element(),
    )
}
