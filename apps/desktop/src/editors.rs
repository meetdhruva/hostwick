//! List editors shared by the settings and host dialogs: snippets and
//! keyword-highlighting rules (with a live preview).

use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{HighlightRule, Snippet};

use crate::terminal::highlight::{Highlighter, parse_color};

// ---- snippets ---------------------------------------------------------------

struct SnippetRow {
    name: Entity<InputState>,
    text: Entity<TextareaState>,
    run: bool,
}

pub struct SnippetEditor {
    rows: Vec<SnippetRow>,
    empty_hint: SharedString,
}

impl SnippetEditor {
    pub fn new(
        snippets: &[Snippet],
        empty_hint: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut s = Self {
            rows: Vec::new(),
            empty_hint,
        };
        for snip in snippets {
            s.push(snip, window, cx);
        }
        s
    }

    fn push(&mut self, snip: &Snippet, window: &mut Window, cx: &mut Context<Self>) {
        let (name, text) = (snip.name.clone(), snip.text.clone());
        self.rows.push(SnippetRow {
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("snippet-label-placeholder"))
                    .default_value(name)
            }),
            text: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(2)
                    .placeholder(t!("snippet-text-placeholder"))
                    .default_value(text)
            }),
            run: snip.run,
        });
    }

    /// The snippets as edited (rows without text are dropped).
    pub fn values(&self, cx: &App) -> Vec<Snippet> {
        self.rows
            .iter()
            .filter_map(|r| {
                let text = r.text.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return None;
                }
                Some(Snippet {
                    name: r.name.read(cx).value().trim().to_owned(),
                    text,
                    run: r.run,
                })
            })
            .collect()
    }
}

impl Render for SnippetEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let n = self.rows.len();
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when(n == 0, |d| {
                d.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.muted_foreground)
                        .child(self.empty_hint.clone()),
                )
            })
            .children(self.rows.iter().enumerate().map(|(i, r)| {
                div()
                    .flex()
                    .gap(px(6.0))
                    .items_start()
                    .p(px(6.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .w(px(150.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(Input::new(&r.name).small())
                            .child(
                                crate::a11y::checkbox(
                                    ("snip-run", i),
                                    t!("snippet-press-enter"),
                                    false,
                                    cx,
                                )
                                .checked(r.run)
                                .on_click(cx.listener(
                                    move |e, v: &bool, _, cx| {
                                        if let Some(r) = e.rows.get_mut(i) {
                                            r.run = *v;
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(div().flex_1().child(Textarea::new(&r.text).small()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                Button::new(("snip-up", i))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::ChevronUp)
                                    .tooltip(t!("snippet-move-up"))
                                    .accessibility_label(t!("snippet-move-up"))
                                    .disabled(i == 0)
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        e.rows.swap(i, i - 1);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(("snip-down", i))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::ChevronDown)
                                    .tooltip(t!("snippet-move-down"))
                                    .accessibility_label(t!("snippet-move-down"))
                                    .disabled(i + 1 >= n)
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        e.rows.swap(i, i + 1);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(("snip-del", i))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip(t!("editor-remove"))
                                    .accessibility_label(t!("editor-remove"))
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        e.rows.remove(i);
                                        cx.notify();
                                    })),
                            ),
                    )
            }))
            .child(
                div().flex().child(
                    Button::new("snip-add")
                        .small()
                        .icon(IconName::Plus)
                        .label(t!("snippet-add"))
                        .on_click(cx.listener(|e, _, window, cx| {
                            e.push(
                                &Snippet {
                                    name: String::new(),
                                    text: String::new(),
                                    run: true,
                                },
                                window,
                                cx,
                            );
                            cx.notify();
                        })),
                ),
            )
    }
}

// ---- highlighting rules -------------------------------------------------------

struct RuleRow {
    pattern: Entity<InputState>,
    color: Entity<InputState>,
    regex: bool,
    enabled: bool,
}

pub struct RuleEditor {
    rows: Vec<RuleRow>,
    _subs: Vec<Subscription>,
}

/// Text shown under the rules, coloured by them.
const PREVIEW: &[&str] = &[
    "nginx.service - A high performance web server",
    "   Active: active (running) since Mon 2026-11-02 09:12:01 UTC",
    "Nov 02 09:12:03 web-1 app[812]: WARNING: deprecated option 'gzip_vary'",
    "Nov 02 09:12:04 web-1 sshd[901]: error: Permission denied (publickey)",
    "Nov 02 09:12:05 web-1 deploy[77]: build succeeded, tests passed: OK",
];

/// Problems with a rule, for the inline message (None = fine).
pub fn rule_problem(r: &HighlightRule) -> Option<String> {
    if r.pattern.trim().is_empty() {
        return Some(t!("rule-error-empty").into());
    }
    if parse_color(&r.color).is_none() {
        return Some(t!("rule-error-color", color = &r.color).into());
    }
    if r.regex
        && let Err(e) = regex::Regex::new(&r.pattern)
    {
        let first = e.to_string();
        let fallback = t!("rule-error-regex");
        let line = first.lines().last().unwrap_or(&fallback);
        return Some(line.trim().to_owned());
    }
    None
}

impl RuleEditor {
    pub fn new(rules: &[HighlightRule], window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut e = Self {
            rows: Vec::new(),
            _subs: Vec::new(),
        };
        for r in rules {
            e.push(r, window, cx);
        }
        e
    }

    fn push(&mut self, r: &HighlightRule, window: &mut Window, cx: &mut Context<Self>) {
        let (pattern, color) = (r.pattern.clone(), r.color.clone());
        let pattern = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("rule-pattern-placeholder"))
                .default_value(pattern)
        });
        let color = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("#rrggbb")
                .default_value(color)
        });
        for i in [&pattern, &color] {
            self._subs
                .push(cx.subscribe(i, |_, _, ev: &InputEvent, cx| {
                    if let InputEvent::Change = ev {
                        cx.notify();
                    }
                }));
        }
        self.rows.push(RuleRow {
            pattern,
            color,
            regex: r.regex,
            enabled: r.enabled,
        });
    }

    pub fn values(&self, cx: &App) -> Vec<HighlightRule> {
        self.rows
            .iter()
            .map(|r| HighlightRule {
                pattern: r.pattern.read(cx).value().to_string(),
                regex: r.regex,
                color: r.color.read(cx).value().trim().to_owned(),
                enabled: r.enabled,
            })
            .filter(|r| !r.pattern.trim().is_empty())
            .collect()
    }

    /// First problem among enabled rules, for blocking Save.
    pub fn problem(&self, cx: &App) -> Option<String> {
        self.values(cx).iter().filter(|r| r.enabled).find_map(|r| {
            rule_problem(r).map(|p| t!("rule-error", pattern = &r.pattern, error = p).to_string())
        })
    }
}

impl Render for RuleEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rules = self.values(cx);
        let (hl, _) = Highlighter::compile(&rules);
        let preview_lines: Vec<AnyElement> = PREVIEW
            .iter()
            .map(|line| {
                let spans = hl.spans(line);
                // Columns → byte ranges (the preview is ASCII).
                let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
                for s in spans.iter().rev() {
                    let range = s.start.min(line.len())..s.end.min(line.len());
                    if range.is_empty()
                        || runs
                            .iter()
                            .any(|(r, _)| r.start < range.end && range.start < r.end)
                    {
                        continue;
                    }
                    runs.push((
                        range,
                        HighlightStyle {
                            color: Some(crate::terminal::colors::to_hsla(s.color)),
                            ..Default::default()
                        },
                    ));
                }
                runs.sort_by_key(|(r, _)| r.start);
                StyledText::new(SharedString::from(*line))
                    .with_highlights(runs)
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(self.rows.iter().enumerate().map(|(i, r)| {
                let rule = HighlightRule {
                    pattern: r.pattern.read(cx).value().to_string(),
                    regex: r.regex,
                    color: r.color.read(cx).value().to_string(),
                    enabled: r.enabled,
                };
                let swatch = parse_color(&rule.color)
                    .map(crate::terminal::colors::to_hsla)
                    .unwrap_or(theme.muted);
                let problem = if rule.enabled {
                    rule_problem(&rule)
                } else {
                    None
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                Checkbox::new(("rule-on", i))
                                    .checked(r.enabled)
                                    .tooltip(t!("rule-enabled"))
                                    .on_click(cx.listener(move |e, v: &bool, _, cx| {
                                        if let Some(r) = e.rows.get_mut(i) {
                                            r.enabled = *v;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(div().flex_1().child(Input::new(&r.pattern).small()))
                            .child(
                                Button::new(("rule-re", i))
                                    .xsmall()
                                    .label(".*")
                                    .when(r.regex, |b| b.primary())
                                    .tooltip(if r.regex {
                                        t!("rule-regex-on")
                                    } else {
                                        t!("rule-regex-off")
                                    })
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        if let Some(r) = e.rows.get_mut(i) {
                                            r.regex = !r.regex;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .size(px(18.0))
                                    .flex_none()
                                    .rounded(px(3.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(swatch),
                            )
                            .child(
                                div().w(px(92.0)).child(
                                    Input::new(&r.color).small().aria_label(t!("rule-color")),
                                ),
                            )
                            .child(
                                Button::new(("rule-del", i))
                                    .xsmall()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip(t!("editor-remove"))
                                    .accessibility_label(t!("editor-remove"))
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        e.rows.remove(i);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when_some(problem, |d, p| {
                        d.child(
                            div()
                                .pl(px(28.0))
                                .text_size(px(11.0))
                                .text_color(theme.danger)
                                .child(p),
                        )
                    })
            }))
            .child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        Button::new("rule-add")
                            .small()
                            .icon(IconName::Plus)
                            .label(t!("rule-add"))
                            .on_click(cx.listener(|e, _, window, cx| {
                                e.push(
                                    &HighlightRule {
                                        pattern: String::new(),
                                        regex: false,
                                        color: "#5fafff".into(),
                                        enabled: true,
                                    },
                                    window,
                                    cx,
                                );
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("rule-defaults")
                            .small()
                            .ghost()
                            .label(t!("rule-defaults"))
                            .on_click(cx.listener(|e, _, window, cx| {
                                e.rows.clear();
                                for r in HighlightRule::defaults() {
                                    e.push(&r, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .p(px(8.0))
                    .rounded(px(4.0))
                    .bg(crate::terminal::colors::to_hsla(
                        crate::terminal::colors::BACKGROUND,
                    ))
                    .text_color(hsla(0.0, 0.0, 0.85, 1.0))
                    .font_family(crate::terminal::monospace_family())
                    .text_size(px(12.0))
                    .flex()
                    .flex_col()
                    .children(preview_lines),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::rule_problem;
    use hostwick_core::HighlightRule;

    #[test]
    fn rule_validation() {
        let r = |p: &str, re: bool, c: &str| HighlightRule {
            pattern: p.into(),
            regex: re,
            color: c.into(),
            enabled: true,
        };
        assert_eq!(rule_problem(&r("error", false, "#ff0000")), None);
        assert!(rule_problem(&r("", false, "#ff0000")).is_some());
        assert!(rule_problem(&r("x", false, "red")).is_some());
        assert!(rule_problem(&r("(unclosed", true, "#00ff00")).is_some());
        assert_eq!(rule_problem(&r("(unclosed", false, "#00ff00")), None);
    }
}
