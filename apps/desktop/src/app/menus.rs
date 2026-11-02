//! Menus: saved hosts, tabs, Tools, Import; deleting and importing hosts.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{ProfileId, ProfileStore as _};

use super::{HostwickApp, PaneBody, TabKind};

impl HostwickApp {
    pub(super) fn delete_profile(
        &mut self,
        id: ProfileId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let app = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (app2, p2) = (app.clone(), p.clone());
            dialog
                .title("Delete host")
                .child(format!("Delete the saved host “{}”?", p.path()))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("del").danger().label("Delete").on_click(
                            move |_, window, cx| {
                                let _ = app2.update(cx, |a, cx| {
                                    let _ = a.backend.store.delete(p2.id);
                                    if a.selected == Some(p2.id) {
                                        a.selected = None;
                                    }
                                    a.reload_profiles();
                                    a.mark_sync_dirty();
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            },
                        )),
                )
        });
    }

    fn import(&mut self, which: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        // "putty"/"winscp" read this PC's registry (Windows); "*-file" asks for a file.
        let from_registry = cfg!(windows) && matches!(which, "putty" | "winscp");
        let which = which.trim_end_matches("-file");
        if which == "ssh" || from_registry {
            let result = match which {
                "ssh" => std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::Path::new(&h).join(".ssh").join("config"))
                    .ok_or_else(|| "no home directory".to_owned())
                    .and_then(|p| {
                        hostwick_import::load_ssh_config(&p)
                            .map_err(|e| format!("{}: {e}", p.display()))
                    })
                    .map(|text| hostwick_import::parse_ssh_config(&text)),
                "putty" => hostwick_import::putty_from_registry().map_err(|e| e.to_string()),
                _ => hostwick_import::winscp_from_registry().map_err(|e| e.to_string()),
            };
            self.apply_import(result, window, cx);
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                match which {
                    "putty" => "PuTTY .reg export",
                    "solar" => "Solar-PuTTY export (.json)",
                    _ => "WinSCP.ini",
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text = match std::fs::read(&path) {
                Ok(b) => hostwick_import::decode_text(&b),
                Err(e) => {
                    let _ =
                        this.update_in(cx, |a, w, cx| a.apply_import(Err(e.to_string()), w, cx));
                    return;
                }
            };
            let result = match which {
                "putty" => hostwick_import::parse_putty_reg(&text),
                "solar" => hostwick_import::parse_solarputty_json(&text),
                _ => hostwick_import::parse_winscp_ini(&text),
            }
            .map_err(|e| e.to_string());
            let _ = this.update_in(cx, |a, w, cx| a.apply_import(result, w, cx));
        })
        .detach();
    }

    fn apply_import(
        &mut self,
        result: Result<hostwick_import::ImportResult, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                window.push_notification(Notification::error(format!("Import failed: {e}")), cx);
                return;
            }
        };
        let (mut added, mut updated) = (Vec::new(), Vec::new());
        let mut failed = Vec::new();
        for mut p in r.profiles {
            let prior = self
                .profiles
                .iter()
                .find(|e| e.source == p.source && p.source != Default::default());
            if let Some(prior) = prior {
                p.id = prior.id;
                // Keep what the person changed locally after the last import.
                p.session_log = prior.session_log.clone();
                p.notes = prior.notes.clone();
            }
            match self.backend.store.upsert(&p) {
                Ok(()) if prior.is_some() => updated.push(p.path()),
                Ok(()) => added.push(p.path()),
                Err(e) => failed.push(format!("{}: {e}", p.path())),
            }
        }
        self.reload_profiles();
        self.mark_sync_dirty();
        for w in &r.warnings {
            tracing::warn!("import: {w}");
        }
        let summary = format!(
            "{} added, {} updated, {} with notes",
            added.len(),
            updated.len(),
            r.warnings.len() + failed.len()
        );
        tracing::info!("import: {summary}");
        if r.warnings.is_empty() && failed.is_empty() {
            window.push_notification(Notification::success(format!("Imported: {summary}")), cx);
            cx.notify();
            return;
        }
        // Something needs attention: show the full report instead of a toast.
        let notes: Vec<String> = failed.into_iter().chain(r.warnings).collect();
        let lists = Rc::new((added, updated, notes));
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let section = |title: String, items: &[String]| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().pt(px(6.0)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .children(items.iter().map(|i| {
                        div()
                            .pl(px(10.0))
                            .text_size(px(12.0))
                            .child(SharedString::from(format!("• {i}")))
                    }))
            };
            let (added, updated, notes) = &*lists;
            dialog
                .title("Import report")
                .w(px(640.0))
                .child(
                    div()
                        .id("import-report")
                        .max_h(px(480.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .when(!notes.is_empty(), |d| {
                            d.child(section(format!("Needs attention ({})", notes.len()), notes))
                        })
                        .when(!added.is_empty(), |d| {
                            d.child(section(format!("Added ({})", added.len()), added))
                        })
                        .when(!updated.is_empty(), |d| {
                            d.child(section(format!("Updated ({})", updated.len()), updated))
                        })
                        .child(
                            div()
                                .pt(px(8.0))
                                .text_size(px(11.0))
                                .text_color(muted)
                                .child("Hosts listed under “Needs attention” were still imported unless the note says skipped. Right-click a host → Edit… to adjust it."),
                        ),
                )
                .footer(
                    Button::new("ok")
                        .primary()
                        .label("OK")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
        });
        cx.notify();
    }

    pub(super) fn import_button(app: WeakEntity<Self>) -> impl IntoElement {
        let item = move |label: &'static str, which: &'static str| {
            let app = app.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let _ = app.update(cx, |a, cx| a.import(which, window, cx));
            })
        };
        Button::new("import")
            .small()
            .label("Import…")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                let m = menu.item(item("OpenSSH ~/.ssh/config", "ssh"));
                let m = if cfg!(windows) {
                    m.item(item("PuTTY (this PC's saved sessions)", "putty"))
                        .item(item("WinSCP (this PC's saved sites)", "winscp"))
                } else {
                    m
                };
                m.item(item("PuTTY .reg export file…", "putty-file"))
                    .item(item("WinSCP.ini file…", "winscp-file"))
                    .item(item("Solar-PuTTY .json export…", "solar"))
            })
    }

    /// Right-click menu for a saved host.
    pub(super) fn host_menu(
        app: WeakEntity<Self>,
        id: ProfileId,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, _| {
            let act = |f: fn(&mut Self, ProfileId, &mut Window, &mut Context<Self>)| {
                let app = app.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| f(a, id, window, cx));
                }
            };
            menu.item(
                PopupMenuItem::new("Open terminal").on_click(act(|a, id, w, cx| {
                    a.open_profile(id, TabKind::Terminal, w, cx)
                })),
            )
            .item(
                PopupMenuItem::new("Open files (SFTP)").on_click(act(|a, id, w, cx| {
                    a.open_profile(id, TabKind::Files, w, cx)
                })),
            )
            .separator()
            .item(PopupMenuItem::new("Edit…").on_click(act(|a, id, w, cx| {
                let p = a.profiles.iter().find(|p| p.id == id).cloned();
                if p.is_some() {
                    a.host_dialog(p, w, cx);
                }
            })))
            .item(
                PopupMenuItem::new("Duplicate")
                    .on_click(act(|a, id, _, cx| a.duplicate_profile(id, cx))),
            )
            .item(
                PopupMenuItem::new("Copy address").on_click(act(|a, id, _, cx| {
                    if let Some(p) = a.profiles.iter().find(|p| p.id == id) {
                        let user = p.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
                        let port = if p.port == 22 {
                            String::new()
                        } else {
                            format!(":{}", p.port)
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(format!(
                            "{user}{}{port}",
                            p.host
                        )));
                    }
                })),
            )
            .separator()
            .item(
                PopupMenuItem::new("Delete…")
                    .on_click(act(|a, id, w, cx| a.delete_profile(id, w, cx))),
            )
        }
    }

    /// Right-click menu for a tab.
    pub(super) fn tab_menu(
        app: WeakEntity<Self>,
        tab_id: u64,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, cx| {
            let act = |f: fn(&mut Self, usize, &mut Window, &mut Context<Self>)| {
                let app = app.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| {
                        if let Some(ix) = a.tabs.iter().position(|t| t.id == tab_id) {
                            f(a, ix, window, cx);
                        }
                    });
                }
            };
            // What the tab is, for the optional entries.
            let (kind, can_break, linked, following) = app
                .upgrade()
                .and_then(|a| {
                    let a = a.read(cx);
                    let t = a.tabs.iter().find(|t| t.id == tab_id)?;
                    let p = t.focused_pane()?;
                    let can_break =
                        matches!(&p.body, PaneBody::Terminal(v) if v.read(cx).can_send_break());
                    let (linked, following) = match &p.body {
                        PaneBody::Files(v) => (p.follows.is_some(), v.read(cx).following()),
                        _ => (false, false),
                    };
                    Some((t.kind, can_break, linked, following))
                })
                .unwrap_or((TabKind::Terminal, false, false, false));
            let mut menu = menu.item(
                PopupMenuItem::new("Reconnect")
                    .on_click(act(|a, ix, w, cx| a.reconnect_focused(ix, w, cx))),
            );
            if kind == TabKind::Terminal {
                menu = menu
                    .item(
                        PopupMenuItem::new("Split right").on_click(act(|a, ix, w, cx| {
                            a.activate_tab(ix, w, cx);
                            a.split_focused(false, w, cx);
                        })),
                    )
                    .item(
                        PopupMenuItem::new("Split down").on_click(act(|a, ix, w, cx| {
                            a.activate_tab(ix, w, cx);
                            a.split_focused(true, w, cx);
                        })),
                    );
            }
            if can_break {
                menu = menu.item(
                    PopupMenuItem::new("Send break").on_click(act(|a, ix, _, cx| {
                        if let Some(PaneBody::Terminal(v)) =
                            a.tabs[ix].focused_pane().map(|p| &p.body)
                        {
                            v.update(cx, |v, cx| v.send_break(cx));
                        }
                    })),
                );
            }
            if linked {
                menu = menu.item(
                    PopupMenuItem::new("Follow terminal folder")
                        .checked(following)
                        .on_click(act(|a, ix, _, cx| {
                            if let Some(PaneBody::Files(v)) =
                                a.tabs[ix].focused_pane().map(|p| &p.body)
                            {
                                v.update(cx, |v, cx| v.toggle_follow(cx));
                            }
                        })),
                );
            }
            menu.separator()
                .item(
                    PopupMenuItem::new("Duplicate").on_click(act(|a, ix, w, cx| {
                        if let Some(p) = a.tabs[ix].focused_pane() {
                            let (profile, quick, kind) =
                                (p.profile.clone(), p.quick.clone(), a.tabs[ix].kind);
                            a.open_tab_quick(profile, quick, kind, w, cx);
                        }
                    })),
                )
                .item(
                    PopupMenuItem::new("Open files (SFTP)").on_click(act(|a, ix, w, cx| {
                        if let Some(p) = a.tabs[ix].focused_pane() {
                            let (profile, quick) = (p.profile.clone(), p.quick.clone());
                            a.open_tab_quick(profile, quick, TabKind::Files, w, cx);
                        }
                    })),
                )
                .item(
                    PopupMenuItem::new("Open terminal").on_click(act(|a, ix, w, cx| {
                        if let Some(p) = a.tabs[ix].focused_pane() {
                            let (profile, quick) = (p.profile.clone(), p.quick.clone());
                            a.open_tab_quick(profile, quick, TabKind::Terminal, w, cx);
                        }
                    })),
                )
                .separator()
                .item(PopupMenuItem::new("Close").on_click(act(|a, ix, _, cx| a.close_tab(ix, cx))))
                .item(
                    PopupMenuItem::new("Close others").on_click(act(|a, ix, _, cx| {
                        let keep = a.tabs.remove(ix);
                        a.tabs = vec![keep];
                        a.active = Some(0);
                        a.unlink_dead_follows();
                        a.persist_tabs(cx);
                        cx.notify();
                    })),
                )
                .item(PopupMenuItem::new("Close all").on_click(act(|a, _, _, cx| {
                    a.tabs.clear();
                    a.active = None;
                    a.persist_tabs(cx);
                    cx.notify();
                })))
        }
    }

    /// The Tools menu in the sidebar header.
    pub(super) fn tools_button(app: WeakEntity<Self>) -> impl IntoElement {
        let item = move |label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let app = app.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let _ = app.update(cx, |a, cx| f(a, window, cx));
            })
        };
        Button::new("tools")
            .ghost()
            .small()
            .icon(gpui_kit::assets::IconName::Wrench)
            .tooltip("Tools")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                menu.item(item("Generate key…", |a, w, cx| {
                    crate::tools::keygen::open(a.backend.clone(), cx.entity().downgrade(), w, cx)
                }))
                .item(item("Inspect or convert key…", |a, w, cx| {
                    crate::tools::keyconv::open(a.backend.clone(), w, cx)
                }))
                .item(item("Known hosts…", |a, w, cx| {
                    crate::tools::knownhosts::open(a.backend.clone(), w, cx)
                }))
                .separator()
                .item(item("Saved passwords…", |a, w, cx| {
                    a.open_settings(Some(crate::settingsform::Section::Passwords), w, cx)
                }))
                .item(item("Settings sync…", |a, w, cx| {
                    a.open_settings(Some(crate::settingsform::Section::Sync), w, cx)
                }))
            })
    }
}
