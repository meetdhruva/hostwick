//! Preferences dialog. Saving persists to the database, updates the global,
//! and calls `on_saved` so open terminals can pick up the change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::LogMode;
use std::sync::Arc;

use crate::backend::Backend;

use crate::logging::LEVELS;
use crate::settings::{RightClick, Settings, ThemeChoice, apply_theme};

/// A row of mutually exclusive small buttons.
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    state: &Rc<Cell<T>>,
    options: &[(T, &'static str)],
) -> Div {
    let mut row = div().flex().gap(px(4.0)).flex_wrap();
    for (i, (value, label)) in options.iter().copied().enumerate() {
        let st = state.clone();
        row = row.child(
            Button::new((id, i))
                .xsmall()
                .label(label)
                .when(state.get() == value, |b| b.primary())
                .on_click(move |_, window, _| {
                    st.set(value);
                    window.refresh();
                }),
        );
    }
    row
}

pub fn open(
    backend: Arc<Backend>,
    window: &mut Window,
    cx: &mut App,
    on_saved: impl Fn(&Settings, &mut Window, &mut App) + 'static,
) {
    let s = Settings::get(cx).clone();
    let font = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("empty = system monospace")
            .default_value(s.font_family.clone())
    });
    let size = cx.new(|cx| InputState::new(window, cx).default_value(s.font_size.to_string()));
    let scroll =
        cx.new(|cx| InputState::new(window, cx).default_value(s.scrollback_lines.to_string()));
    let default_dir = s.session_log_dir(&backend.paths);
    let logdir = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(default_dir.display().to_string())
            .default_value(s.session_log_dir.clone())
    });
    let theme = Rc::new(Cell::new(s.theme));
    let right = Rc::new(Cell::new(s.right_click));
    let mode = Rc::new(Cell::new(s.session_log_mode));
    let level = Rc::new(Cell::new(
        LEVELS.iter().position(|l| *l == s.log_level).unwrap_or(2),
    ));
    let copy_sel = Rc::new(Cell::new(s.copy_on_select));
    let log_all = Rc::new(Cell::new(s.log_all_sessions));
    let base = Rc::new(RefCell::new(s));
    let on_saved = Rc::new(on_saved);
    let app_logs = backend.paths.logs.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().muted_foreground;
        let label = |t: &'static str| div().w(px(150.0)).text_size(px(13.0)).child(t);
        let row = |t: &'static str, el: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(label(t))
                .child(div().flex_1().child(el))
        };
        let section = |t: &'static str| {
            div()
                .pt(px(8.0))
                .text_size(px(12.0))
                .text_color(muted)
                .child(t.to_uppercase())
        };
        let check = |id: &'static str, text: &'static str, cell: &Rc<Cell<bool>>| {
            let c = cell.clone();
            Checkbox::new(id)
                .label(text)
                .checked(cell.get())
                .on_click(move |v, window, _| {
                    c.set(*v);
                    window.refresh();
                })
        };
        let level_buttons = {
            let mut r = div().flex().gap(px(4.0));
            for (i, l) in LEVELS.iter().enumerate() {
                let lv = level.clone();
                r = r.child(
                    Button::new(("lvl", i))
                        .xsmall()
                        .label(*l)
                        .when(level.get() == i, |b| b.primary())
                        .on_click(move |_, window, _| {
                            lv.set(i);
                            window.refresh();
                        }),
                );
            }
            r
        };
        let dir = app_logs.clone();
        let open_logs = Button::new("open-logs")
            .xsmall()
            .label("Open log folder")
            .on_click(move |_, _, cx| {
                let _ = std::fs::create_dir_all(&dir);
                cx.open_with_system(&dir);
            });

        let body = div()
            .id("settings-form")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .max_h(px(560.0))
            .overflow_y_scroll()
            .child(section("Appearance"))
            .child(row(
                "Theme",
                choice(
                    "theme",
                    &theme,
                    &[(ThemeChoice::Dark, "Dark"), (ThemeChoice::Light, "Light")],
                )
                .into_any_element(),
            ))
            .child(row(
                "Terminal font",
                Input::new(&font).small().into_any_element(),
            ))
            .child(row(
                "Font size",
                Input::new(&size).small().into_any_element(),
            ))
            .child(section("Terminal"))
            .child(row(
                "Scrollback lines",
                Input::new(&scroll).small().into_any_element(),
            ))
            .child(row(
                "Right-click",
                choice(
                    "rc",
                    &right,
                    &[
                        (RightClick::Menu, "Show menu"),
                        (RightClick::Paste, "Paste (PuTTY)"),
                    ],
                )
                .into_any_element(),
            ))
            .child(div().pl(px(158.0)).child(check(
                "copy-sel",
                "Copy selected text automatically",
                &copy_sel,
            )))
            .child(section("Session logging"))
            .child(div().pl(px(158.0)).child(check(
                "log-all",
                "Log every session to a file",
                &log_all,
            )))
            .child(row(
                "Log folder",
                Input::new(&logdir).small().into_any_element(),
            ))
            .child(row(
                "Log content",
                choice(
                    "lm",
                    &mode,
                    &[
                        (LogMode::Printable, "Printable text"),
                        (LogMode::Raw, "Raw (with escape codes)"),
                    ],
                )
                .into_any_element(),
            ))
            .child(
                div()
                    .pl(px(158.0))
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child("A host's own session-log setting (Edit host) overrides these."),
            )
            .child(section("Diagnostics"))
            .child(row("App log level", level_buttons.into_any_element()))
            .child(
                div()
                    .pl(px(158.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(open_logs)
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child(app_logs.display().to_string()),
                    ),
            );

        let (font2, size2, scroll2, logdir2) =
            (font.clone(), size.clone(), scroll.clone(), logdir.clone());
        let (theme2, right2, mode2, level2, copy2, all2) = (
            theme.clone(),
            right.clone(),
            mode.clone(),
            level.clone(),
            copy_sel.clone(),
            log_all.clone(),
        );
        let (base2, store2, saved2) = (base.clone(), backend.clone(), on_saved.clone());
        let submit = Rc::new(move |window: &mut Window, cx: &mut App| {
            let v = |i: &Entity<InputState>| i.read(cx).value().trim().to_owned();
            let Ok(font_size) = v(&size2).parse::<f32>() else {
                window.push_notification(Notification::warning("Font size must be a number"), cx);
                return;
            };
            let Ok(scrollback) = v(&scroll2).parse::<usize>() else {
                window.push_notification(
                    Notification::warning("Scrollback must be a whole number"),
                    cx,
                );
                return;
            };
            let new = Settings {
                font_family: v(&font2),
                font_size,
                theme: theme2.get(),
                scrollback_lines: scrollback,
                right_click: right2.get(),
                copy_on_select: copy2.get(),
                log_level: LEVELS
                    .get(level2.get())
                    .copied()
                    .unwrap_or("info")
                    .to_owned(),
                session_log_dir: v(&logdir2),
                log_all_sessions: all2.get(),
                session_log_mode: mode2.get(),
                ..base2.borrow().clone()
            }
            .sanitized();
            if let Err(e) = new.save(&store2.store) {
                window.push_notification(
                    Notification::error(format!("Cannot save settings: {e}")),
                    cx,
                );
                return;
            }
            if new.log_level != base2.borrow().log_level {
                crate::logging::set_level(&new.log_level);
            }
            if new.theme != base2.borrow().theme {
                apply_theme(new.theme, cx);
            }
            *base2.borrow_mut() = new.clone();
            cx.set_global(new.clone());
            saved2(&new, window, cx);
            window.close_dialog(cx);
            window.refresh();
        });
        let (s1, s2) = (submit.clone(), submit.clone());
        dialog
            .title("Settings")
            .w(px(620.0))
            .child(body)
            .on_ok(move |_, window, cx| {
                s1(window, cx);
                false
            })
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save")
                            .primary()
                            .label("Save")
                            .on_click(move |_, window, cx| s2(window, cx)),
                    ),
            )
    });
}
