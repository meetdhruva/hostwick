//! Menus: saved hosts, tabs, Tools, Import; deleting and importing hosts.

use std::rc::Rc;

use crate::notify::{Toast, ToastExt as _};
use crate::zoom::px;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{ProfileId, ProfileStore as _};

use super::hosts::GroupOpen;
use super::palette::{Choice, Mods};
use super::sidebar::{RowId, Sec};
use super::{HostwickApp, PaneBody, TabKind};
use crate::shortcuts::{Sc, menu_item};

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
                .title(crate::a11y::text(t!("host-delete-title")))
                .child(crate::a11y::text(t!(
                    "host-delete-confirm",
                    name = p.path()
                )))
                .footer(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("cancel")
                                .label(t!("button-cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("del")
                                .danger()
                                .label(t!("button-delete"))
                                .on_click(move |_, window, cx| {
                                    let _ =
                                        app2.update(cx, |a, cx| a.do_delete_hosts(vec![p2.id], cx));
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    pub(super) fn import(
        &mut self,
        which: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // "putty"/"winscp" read this PC's registry (Windows); "*-file" asks for a file.
        let from_registry = cfg!(windows) && matches!(which, "putty" | "winscp");
        let which = which.trim_end_matches("-file");
        if which == "ssh" || from_registry {
            let result = match which {
                "ssh" => std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::Path::new(&h).join(".ssh").join("config"))
                    .ok_or_else(|| t!("import-no-home").to_string())
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
            prompt: Some(match which {
                "putty" => t!("import-pick-putty"),
                "solar" => t!("import-pick-solar"),
                _ => t!("import-pick-winscp"),
            }),
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

    pub(super) fn apply_import(
        &mut self,
        result: Result<hostwick_import::ImportResult, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                window.toast(Toast::error(t!("import-failed", error = e)), cx);
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
        let summary = t!(
            "import-summary",
            added = added.len(),
            updated = updated.len(),
            notes = r.warnings.len() + failed.len()
        );
        tracing::info!(
            added = added.len(),
            updated = updated.len(),
            notes = r.warnings.len() + failed.len(),
            "import"
        );
        if r.warnings.is_empty() && failed.is_empty() {
            window.toast(Toast::success(t!("import-done", summary = summary)), cx);
            cx.notify();
            return;
        }
        // Something needs attention: show the full report instead of a toast.
        let notes: Vec<String> = failed.into_iter().chain(r.warnings).collect();
        let lists = Rc::new((added, updated, notes));
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let section = |title: SharedString, items: &[String]| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .pt(px(6.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .children(items.iter().map(|i| {
                        div()
                            .pl(px(10.0))
                            .text_size(px(12.0))
                            .child(SharedString::from(format!("• {i}")))
                    }))
            };
            let (added, updated, notes) = &*lists;
            dialog
                .title(crate::a11y::text(t!("import-report-title")))
                .w(px(640.0))
                .child(
                    div()
                        .id("import-report")
                        .max_h(px(480.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .when(!notes.is_empty(), |d| {
                            d.child(section(
                                t!("import-report-attention", count = notes.len()),
                                notes,
                            ))
                        })
                        .when(!added.is_empty(), |d| {
                            d.child(section(
                                t!("import-report-added", count = added.len()),
                                added,
                            ))
                        })
                        .when(!updated.is_empty(), |d| {
                            d.child(section(
                                t!("import-report-updated", count = updated.len()),
                                updated,
                            ))
                        })
                        .child(
                            div()
                                .pt(px(8.0))
                                .text_size(px(11.0))
                                .text_color(muted)
                                .child(crate::a11y::text(t!("import-report-note"))),
                        ),
                )
                .footer(
                    Button::new("ok")
                        .primary()
                        .label(t!("button-ok"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
        });
        cx.notify();
    }

    pub(super) fn import_button(app: WeakEntity<Self>) -> impl IntoElement {
        let scan_app = app.clone();
        let item = move |label: SharedString, which: &'static str| {
            let app = app.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let _ = app.update(cx, |a, cx| a.import(which, window, cx));
            })
        };
        Button::new("import")
            .small()
            .label(t!("sidebar-import"))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                let scan_app = scan_app.clone();
                let m = menu
                    .item(PopupMenuItem::new(t!("import-menu-scan")).on_click(
                        move |_, window, cx| {
                            let _ = scan_app.update(cx, |a, cx| a.scan_computer(window, cx));
                        },
                    ))
                    .separator()
                    .item(item(t!("import-menu-ssh-config"), "ssh"));
                let m = if cfg!(windows) {
                    m.item(item(t!("import-menu-putty-registry"), "putty"))
                        .item(item(t!("import-menu-winscp-registry"), "winscp"))
                } else {
                    m
                };
                m.item(item(t!("import-menu-putty-file"), "putty-file"))
                    .item(item(t!("import-menu-winscp-file"), "winscp-file"))
                    .item(item(t!("import-menu-solar-file"), "solar"))
            })
    }

    /// Right-click menu for a saved host (or the multi-selection it is in).
    pub(super) fn host_menu(
        app: WeakEntity<Self>,
        id: ProfileId,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, cx| {
            let act = |f: fn(&mut Self, ProfileId, &mut Window, &mut Context<Self>)| {
                let app = app.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| f(a, id, window, cx));
                }
            };
            let (sel, fav, terminal_tab) = app
                .upgrade()
                .map(|a| {
                    let a = a.read(cx);
                    let sel = a.selection();
                    let sel = if sel.contains(&id) { sel } else { vec![id] };
                    let fav = sel.iter().all(|s| {
                        a.profiles
                            .iter()
                            .find(|p| p.id == *s)
                            .is_some_and(crate::hostops::is_favourite)
                    });
                    let terminal_tab = a
                        .active
                        .and_then(|i| a.tabs.get(i))
                        .is_some_and(|t| t.kind == TabKind::Terminal);
                    (sel, fav, terminal_tab)
                })
                .unwrap_or_else(|| (vec![id], false, false));
            let fav_label = if fav {
                t!("menu-unfavourite")
            } else {
                t!("menu-favourite")
            };
            if sel.len() > 1 {
                let n = sel.len();
                let group = |mode: GroupOpen| {
                    let (app, sel) = (app.clone(), sel.clone());
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let _ = app.update(cx, |a, cx| a.open_group(sel.clone(), mode, window, cx));
                    }
                };
                let with_sel =
                    |f: fn(&mut Self, Vec<ProfileId>, &mut Window, &mut Context<Self>)| {
                        let (app, sel) = (app.clone(), sel.clone());
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            let _ = app.update(cx, |a, cx| f(a, sel.clone(), window, cx));
                        }
                    };
                return menu
                    .label(t!("sidebar-selected-count", count = n))
                    .item(
                        menu_item(t!("menu-open-all-tabs"), Some(Sc::HostOpen))
                            .on_click(group(GroupOpen::Tabs)),
                    )
                    .item(
                        PopupMenuItem::new(t!("menu-open-all-grid"))
                            .on_click(group(GroupOpen::Grid)),
                    )
                    .item(
                        PopupMenuItem::new(t!("menu-open-all-grid-broadcast"))
                            .on_click(group(GroupOpen::GridBroadcast)),
                    )
                    .item(
                        menu_item(t!("menu-open-all-files"), Some(Sc::HostFiles))
                            .on_click(group(GroupOpen::Files)),
                    )
                    .separator()
                    .item(
                        menu_item(t!("menu-bulk-edit", count = n), Some(Sc::HostEdit))
                            .on_click(with_sel(|a, s, w, cx| a.bulk_edit(s, w, cx))),
                    )
                    .item(
                        PopupMenuItem::new(fav_label)
                            .on_click(with_sel(|a, s, _, cx| a.toggle_favourite(&s, cx))),
                    )
                    .separator()
                    .item(
                        menu_item(t!("menu-delete-hosts", count = n), Some(Sc::HostDelete))
                            .on_click(with_sel(|a, s, w, cx| a.delete_hosts(s, w, cx))),
                    );
            }
            let menu = menu
                .item(
                    menu_item(t!("menu-open-terminal"), Some(Sc::HostOpen)).on_click(act(
                        |a, id, w, cx| {
                            a.note_use(&format!("host:{id}"));
                            a.open_profile(id, TabKind::Terminal, w, cx)
                        },
                    )),
                )
                .item(
                    menu_item(t!("menu-open-files"), Some(Sc::HostFiles)).on_click(act(
                        |a, id, w, cx| a.open_profile(id, TabKind::Files, w, cx),
                    )),
                );
            let menu = if terminal_tab {
                menu.item(
                    menu_item(t!("menu-open-split"), Some(Sc::HostSplit)).on_click(act(
                        |a, id, w, cx| a.run_choice(Choice::Host(id), Mods::Shift, w, cx),
                    )),
                )
            } else {
                menu
            };
            menu.separator()
                .item(menu_item(t!("menu-edit"), Some(Sc::HostEdit)).on_click(act(
                    |a, id, w, cx| {
                        let p = a.profiles.iter().find(|p| p.id == id).cloned();
                        if p.is_some() {
                            a.host_dialog(p, w, cx);
                        }
                    },
                )))
                .item(
                    menu_item(t!("menu-rename"), Some(Sc::HostRename)).on_click(act(
                        |a, id, w, cx| {
                            a.select_host(id, cx);
                            a.start_rename(w, cx);
                        },
                    )),
                )
                .item(
                    PopupMenuItem::new(t!("menu-duplicate"))
                        .on_click(act(|a, id, _, cx| a.duplicate_profile(id, cx))),
                )
                .item(
                    PopupMenuItem::new(fav_label)
                        .on_click(act(|a, id, _, cx| a.toggle_favourite(&[id], cx))),
                )
                .item(
                    PopupMenuItem::new(t!("menu-copy-address")).on_click(act(|a, id, _, cx| {
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
                    menu_item(t!("menu-delete"), Some(Sc::HostDelete))
                        .on_click(act(|a, id, w, cx| a.delete_profile(id, w, cx))),
                )
        }
    }

    /// Right-click menu for a folder.
    pub(super) fn folder_menu(
        app: WeakEntity<Self>,
        path: String,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, cx| {
            let ids = app
                .upgrade()
                .map(|a| a.read(cx).hosts_in(&path))
                .unwrap_or_default();
            let empty = ids.is_empty();
            let group = |mode: GroupOpen| {
                let (app, ids) = (app.clone(), ids.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| a.open_group(ids.clone(), mode, window, cx));
                }
            };
            let on_path = |f: fn(&mut Self, String, &mut Window, &mut Context<Self>)| {
                let (app, path) = (app.clone(), path.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = app.update(cx, |a, cx| f(a, path.clone(), window, cx));
                }
            };
            menu.item(
                PopupMenuItem::new(t!("menu-open-all-tabs-count", count = ids.len()))
                    .disabled(empty)
                    .on_click(group(GroupOpen::Tabs)),
            )
            .item(
                PopupMenuItem::new(t!("menu-open-all-grid"))
                    .disabled(empty)
                    .on_click(group(GroupOpen::Grid)),
            )
            .item(
                PopupMenuItem::new(t!("menu-open-all-grid-broadcast"))
                    .disabled(empty)
                    .on_click(group(GroupOpen::GridBroadcast)),
            )
            .separator()
            .item(
                PopupMenuItem::new(t!("menu-new-subfolder"))
                    .on_click(on_path(|a, p, w, cx| a.new_folder(&p, w, cx))),
            )
            .item(
                menu_item(t!("menu-rename"), Some(Sc::HostRename)).on_click(on_path(
                    |a, p, w, cx| {
                        a.set_cursor(Some(RowId::Folder(p)));
                        a.start_rename(w, cx);
                    },
                )),
            )
            .item(
                menu_item(t!("menu-delete-folder"), Some(Sc::HostDelete))
                    .on_click(on_path(|a, p, w, cx| a.delete_folder(p, w, cx))),
            )
        }
    }

    /// Right-click menu for a sidebar section header.
    pub(super) fn section_menu(
        app: WeakEntity<Self>,
        sec: Sec,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _, cx| {
            let app = app.clone();
            match sec {
                Sec::Hosts => menu.item(PopupMenuItem::new(t!("act-new-folder")).on_click(
                    move |_, window, cx| {
                        let _ = app.update(cx, |a, cx| a.new_folder("", window, cx));
                    },
                )),
                Sec::Favourites => {
                    let ids = app
                        .upgrade()
                        .map(|a| a.read(cx).favourite_ids())
                        .unwrap_or_default();
                    let group = |mode: GroupOpen| {
                        let (app, ids) = (app.clone(), ids.clone());
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            let _ =
                                app.update(cx, |a, cx| a.open_group(ids.clone(), mode, window, cx));
                        }
                    };
                    menu.item(
                        PopupMenuItem::new(t!("menu-open-all-tabs-count", count = ids.len()))
                            .on_click(group(GroupOpen::Tabs)),
                    )
                    .item(
                        PopupMenuItem::new(t!("menu-open-all-grid"))
                            .on_click(group(GroupOpen::Grid)),
                    )
                    .item(
                        PopupMenuItem::new(t!("menu-open-all-grid-broadcast"))
                            .on_click(group(GroupOpen::GridBroadcast)),
                    )
                }
                Sec::Recent => menu.item(PopupMenuItem::new(t!("menu-clear-recent")).on_click(
                    move |_, _, cx| {
                        let _ = app.update(cx, |a, cx| {
                            a.recents.clear();
                            a.save_recents();
                            cx.notify();
                        });
                    },
                )),
            }
        }
    }

    /// Right-click menu for a tab.
    pub(super) fn tab_menu(
        app: WeakEntity<Self>,
        tab_id: u64,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, window, cx| {
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
            // Windows this tab could move to.
            let (tab_count, windows) = app
                .upgrade()
                .map(|a| {
                    let a = a.read(cx);
                    (a.tabs.len(), a.other_windows(cx))
                })
                .unwrap_or_default();
            let mut menu = menu.item(
                menu_item(t!("menu-reconnect"), Some(Sc::Reconnect))
                    .on_click(act(|a, ix, w, cx| a.reconnect_focused(ix, w, cx))),
            );
            if kind == TabKind::Terminal {
                menu = menu
                    .item(
                        menu_item(t!("menu-split-right"), Some(Sc::SplitRight)).on_click(act(
                            |a, ix, w, cx| {
                                a.activate_tab(ix, w, cx);
                                a.split_focused(false, w, cx);
                            },
                        )),
                    )
                    .item(
                        menu_item(t!("menu-split-down"), Some(Sc::SplitDown)).on_click(act(
                            |a, ix, w, cx| {
                                a.activate_tab(ix, w, cx);
                                a.split_focused(true, w, cx);
                            },
                        )),
                    );
            }
            if can_break {
                menu = menu.item(
                    menu_item(t!("menu-send-break"), Some(Sc::SendBreak)).on_click(act(
                        |a, ix, _, cx| {
                            if let Some(PaneBody::Terminal(v)) =
                                a.tabs[ix].focused_pane().map(|p| &p.body)
                            {
                                v.update(cx, |v, cx| v.send_break(cx));
                            }
                        },
                    )),
                );
            }
            if linked {
                menu = menu.item(
                    PopupMenuItem::new(t!("menu-follow-terminal"))
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
                    menu_item(t!("menu-duplicate"), Some(Sc::DuplicateTab)).on_click(act(
                        |a, ix, w, cx| {
                            if let Some(p) = a.tabs[ix].focused_pane() {
                                let (profile, quick, kind) =
                                    (p.profile.clone(), p.quick.clone(), a.tabs[ix].kind);
                                a.open_tab_quick(profile, quick, kind, w, cx);
                            }
                        },
                    )),
                )
                .item(
                    PopupMenuItem::new(t!("menu-open-files")).on_click(act(|a, ix, w, cx| {
                        if let Some(p) = a.tabs[ix].focused_pane() {
                            let (profile, quick) = (p.profile.clone(), p.quick.clone());
                            a.open_tab_quick(profile, quick, TabKind::Files, w, cx);
                        }
                    })),
                )
                .item(
                    PopupMenuItem::new(t!("menu-open-terminal")).on_click(act(|a, ix, w, cx| {
                        if let Some(p) = a.tabs[ix].focused_pane() {
                            let (profile, quick) = (p.profile.clone(), p.quick.clone());
                            a.open_tab_quick(profile, quick, TabKind::Terminal, w, cx);
                        }
                    })),
                )
                .separator()
                .item(
                    PopupMenuItem::new(t!("menu-move-new-window"))
                        .disabled(tab_count < 2)
                        .on_click({
                            let app = app.clone();
                            move |_, window, cx| {
                                let _ = app.update(cx, |a, cx| {
                                    a.move_to_new_window(tab_id, None, window, cx)
                                });
                            }
                        }),
                )
                .when(!windows.is_empty(), |menu| {
                    let app = app.clone();
                    let windows = windows.clone();
                    menu.submenu(t!("menu-move-to-window"), window, cx, move |sub, _, _| {
                        windows.iter().fold(sub, |sub, (key, name)| {
                            let (app, key) = (app.clone(), *key);
                            sub.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, window, cx| {
                                    let _ = app.update(cx, |a, cx| {
                                        a.move_to_window(tab_id, key, window, cx)
                                    });
                                },
                            ))
                        })
                    })
                })
                .separator()
                .item(
                    menu_item(t!("menu-close"), Some(Sc::ClosePane))
                        .on_click(act(|a, ix, _, cx| a.close_tab(ix, cx))),
                )
                .item(
                    PopupMenuItem::new(t!("menu-close-others")).on_click(act(|a, ix, _, cx| {
                        let keep = a.tabs.remove(ix);
                        let closed = std::mem::take(&mut a.tabs);
                        for t in &closed {
                            a.remember_closed(t, cx);
                        }
                        a.tabs = vec![keep];
                        a.active = Some(0);
                        a.unlink_dead_follows();
                        a.persist_tabs(cx);
                        cx.notify();
                    })),
                )
                .item(
                    PopupMenuItem::new(t!("menu-close-all")).on_click(act(|a, _, _, cx| {
                        let closed = std::mem::take(&mut a.tabs);
                        for t in &closed {
                            a.remember_closed(t, cx);
                        }
                        a.active = None;
                        a.persist_tabs(cx);
                        cx.notify();
                    })),
                )
        }
    }

    /// The Tools menu in the sidebar header.
    pub(super) fn tools_button(app: WeakEntity<Self>) -> impl IntoElement {
        let item = move |label: SharedString, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let app = app.clone();
            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                let _ = app.update(cx, |a, cx| f(a, window, cx));
            })
        };
        Button::new("tools")
            .ghost()
            .small()
            .icon(gpui_kit::assets::IconName::Wrench)
            .tooltip(t!("sidebar-tools"))
            .accessibility_label(t!("sidebar-tools"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                menu.item(item(t!("tools-generate-key"), |a, w, cx| {
                    crate::tools::keygen::open(a.backend.clone(), cx.entity().downgrade(), w, cx)
                }))
                .item(item(t!("tools-inspect-key"), |a, w, cx| {
                    crate::tools::keyconv::open(a.backend.clone(), w, cx)
                }))
                .item(item(t!("tools-known-hosts"), |a, w, cx| {
                    crate::tools::knownhosts::open(a.backend.clone(), w, cx)
                }))
                .separator()
                .item(item(t!("tools-saved-passwords"), |a, w, cx| {
                    a.open_settings(Some(crate::settingsform::Section::Passwords), w, cx)
                }))
                .item(item(t!("tools-settings-sync"), |a, w, cx| {
                    a.open_settings(Some(crate::settingsform::Section::Sync), w, cx)
                }))
            })
    }
}
