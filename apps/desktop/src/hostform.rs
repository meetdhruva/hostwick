//! "New host" / "Edit host" dialog covering every profile setting.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::*;
use hostwick_core::{AuthMethod, Endpoint, ForwardSpec, HostProfile, LogMode, SessionLog, Target};

/// Text form of forwards, one per space: `L8080:localhost:80 D1080`.
pub fn forwards_to_text(f: &[ForwardSpec]) -> String {
    let bind = |b: &str, p: u16| {
        if b == "127.0.0.1" {
            p.to_string()
        } else {
            format!("{b}:{p}")
        }
    };
    f.iter()
        .map(|s| match s {
            ForwardSpec::Local {
                bind_addr,
                bind_port,
                host,
                host_port,
            } => {
                format!("L{}:{host}:{host_port}", bind(bind_addr, *bind_port))
            }
            ForwardSpec::Remote {
                bind_addr,
                bind_port,
                host,
                host_port,
            } => {
                format!("R{}:{host}:{host_port}", bind(bind_addr, *bind_port))
            }
            ForwardSpec::Dynamic {
                bind_addr,
                bind_port,
            } => format!("D{}", bind(bind_addr, *bind_port)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Accepts `L8080:host:80`, `-L 8080:host:80` (ssh style) and lists separated
/// by spaces, commas or new lines.
pub fn parse_forwards(text: &str) -> Result<Vec<ForwardSpec>, String> {
    let mut out = Vec::new();
    let mut tokens = text
        .split([' ', ',', '\n', '\t'])
        .map(str::trim)
        .filter(|t| !t.is_empty());
    while let Some(tok) = tokens.next() {
        let t = tok.trim_start_matches('-');
        let mut chars = t.chars();
        let flag = chars.next().map(|c| c.to_ascii_uppercase()).unwrap_or(' ');
        let mut spec = chars.as_str().trim_start_matches('=').to_owned();
        if spec.is_empty() {
            // `-L 8080:host:80`: the spec is the next token.
            spec = tokens
                .next()
                .ok_or_else(|| format!("`{tok}` needs a port specification"))?
                .to_owned();
        }
        out.push(ForwardSpec::parse(flag, &spec)?);
    }
    Ok(out)
}

pub fn jumps_to_text(j: &[Endpoint]) -> String {
    j.iter()
        .map(|e| {
            let user = e.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
            if e.port == 22 {
                format!("{user}{}", e.host)
            } else {
                format!("{user}{}:{}", e.host, e.port)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn parse_jumps(text: &str) -> Result<Vec<Endpoint>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let target = Target::parse(t).map_err(|e| format!("jump host `{t}`: {e}"))?;
            Ok(Endpoint {
                port: target.port_or_default(),
                host: target.host,
                user: target.user,
            })
        })
        .collect()
}

fn key_paths(p: &HostProfile) -> String {
    p.auth
        .iter()
        .filter_map(|a| match a {
            AuthMethod::KeyFile { path, .. } => Some(path.display().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

struct Fields {
    name: Entity<InputState>,
    folder: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    keys: Entity<InputState>,
    jump: Entity<InputState>,
    forwards: Entity<InputState>,
    keepalive: Entity<InputState>,
    term: Entity<InputState>,
    log: Entity<InputState>,
    notes: Entity<InputState>,
}

/// Open the editor. `existing = None` creates a new host. `on_save` gets the
/// validated profile (same id when editing).
pub fn open(
    existing: Option<HostProfile>,
    window: &mut Window,
    cx: &mut App,
    on_save: impl Fn(HostProfile, &mut Window, &mut App) + 'static,
) {
    let editing = existing.is_some();
    let base = existing.unwrap_or_else(|| HostProfile::new("", ""));
    let mk = |value: String, ph: &str, window: &mut Window, cx: &mut App| {
        let ph = ph.to_owned();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ph)
                .default_value(value)
        })
    };
    let f = Rc::new(Fields {
        name: mk(base.name.clone(), "prod-web-1", window, cx),
        folder: mk(base.folder.clone(), "prod/web (optional)", window, cx),
        host: mk(
            base.host.clone(),
            "10.0.0.5 or host.example.com",
            window,
            cx,
        ),
        port: mk(base.port.to_string(), "22", window, cx),
        user: mk(
            base.user.clone().unwrap_or_default(),
            "deploy (empty = your local user name)",
            window,
            cx,
        ),
        keys: mk(
            key_paths(&base),
            "~/.ssh/id_ed25519, C:\\keys\\work.ppk (OpenSSH or PuTTY .ppk)",
            window,
            cx,
        ),
        jump: mk(
            jumps_to_text(&base.jump),
            "user@bastion:22, inner-host (optional)",
            window,
            cx,
        ),
        forwards: mk(
            forwards_to_text(&base.forwards),
            "L8080:localhost:80 R9000:localhost:3000 D1080",
            window,
            cx,
        ),
        keepalive: mk(
            base.keepalive_secs.to_string(),
            "seconds, 0 = off",
            window,
            cx,
        ),
        term: mk(base.term.clone(), "xterm-256color", window, cx),
        log: mk(
            base.session_log
                .as_ref()
                .map(|l| l.path.clone())
                .unwrap_or_default(),
            "empty = off, or e.g. &S-&Y&M&D-&T.log",
            window,
            cx,
        ),
        notes: mk(base.notes.clone(), "", window, cx),
    });
    let legacy = Rc::new(Cell::new(base.allow_legacy_algorithms));
    let agent_fwd = Rc::new(Cell::new(base.agent_forwarding));
    let raw_log = Rc::new(Cell::new(
        base.session_log
            .as_ref()
            .is_some_and(|l| l.mode == LogMode::Raw),
    ));
    let base = Rc::new(base);
    let on_save = Rc::new(on_save);
    let first = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            f.name.focus_handle(cx).focus(window, cx);
        }
        let muted = cx.theme().muted_foreground;
        let row = |label: &str, input: &Entity<InputState>| {
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(div().w(px(110.0)).text_size(px(13.0)).child(label.to_owned()))
                .child(div().flex_1().child(Input::new(input).small()))
        };
        let check = |id: &'static str, label: &'static str, cell: &Rc<Cell<bool>>| {
            let c = cell.clone();
            Checkbox::new(id).label(label).checked(cell.get()).on_click(move |v, window, _| {
                c.set(*v);
                window.refresh();
            })
        };
        let key_input = f.keys.clone();
        let browse = Button::new("browse-key").xsmall().label("Browse…").on_click(move |_, window, cx| {
            let picked = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some("Private key (OpenSSH or .ppk)".into()),
            });
            let input = key_input.clone();
            window
                .spawn(cx, async move |cx| {
                    let Ok(Ok(Some(paths))) = picked.await else { return };
                    let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
                    let _ = cx.update(|window, cx| input.update(cx, |s, cx| s.set_value(text, window, cx)));
                })
                .detach();
        });

        let body = div()
            .id("host-form")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .max_h(px(560.0))
            .overflow_y_scroll()
            .child(row("Name", &f.name))
            .child(row("Folder", &f.folder))
            .child(row("Host", &f.host))
            .child(row("Port", &f.port))
            .child(row("User", &f.user))
            .child(row("Key files", &f.keys).child(browse))
            .child(row("Jump hosts", &f.jump))
            .child(row("Port forwards", &f.forwards))
            .child(row("Keepalive (s)", &f.keepalive))
            .child(row("Terminal type", &f.term))
            .child(row("Session log", &f.log))
            .child(
                div()
                    .pl(px(118.0))
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child("Tokens: &S session, &H host, &P port, &U user, &Y&M&D date, &T time. Relative paths go to the log folder."),
            )
            .child(row("Notes", &f.notes))
            .child(
                div()
                    .pl(px(118.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(check("raw-log", "Log raw output (with escape codes)", &raw_log))
                    .child(check("legacy", "Allow legacy algorithms (ssh-rsa/SHA-1) for old devices", &legacy)),
            );

        let (f2, base2, save2) = (f.clone(), base.clone(), on_save.clone());
        let (legacy2, agent2, raw2) = (legacy.clone(), agent_fwd.clone(), raw_log.clone());
        let submit = Rc::new(move |window: &mut Window, cx: &mut App| {
            let v = |i: &Entity<InputState>| i.read(cx).value().trim().to_owned();
            let fail = |msg: String, window: &mut Window, cx: &mut App| {
                window.push_notification(Notification::warning(msg), cx);
            };
            let (name, host) = (v(&f2.name), v(&f2.host));
            if name.is_empty() || host.is_empty() {
                return fail("Name and host are required".into(), window, cx);
            }
            let Ok(port) = v(&f2.port).parse::<u16>() else {
                return fail("Port must be a number from 1 to 65535".into(), window, cx);
            };
            let Ok(keepalive) = v(&f2.keepalive).parse::<u32>() else {
                return fail("Keepalive must be a number of seconds".into(), window, cx);
            };
            let jump = match parse_jumps(&v(&f2.jump)) {
                Ok(j) => j,
                Err(e) => return fail(e, window, cx),
            };
            let forwards = match parse_forwards(&v(&f2.forwards)) {
                Ok(f) => f,
                Err(e) => return fail(format!("Port forwards: {e}"), window, cx),
            };
            let mut p = (*base2).clone();
            p.name = name;
            p.folder = v(&f2.folder).trim_matches('/').to_owned();
            p.host = host;
            p.port = if port == 0 { 22 } else { port };
            p.user = Some(v(&f2.user)).filter(|u| !u.is_empty());
            p.jump = jump;
            p.forwards = forwards;
            p.keepalive_secs = keepalive;
            p.term = Some(v(&f2.term)).filter(|t| !t.is_empty()).unwrap_or_else(|| "xterm-256color".into());
            p.notes = v(&f2.notes);
            p.allow_legacy_algorithms = legacy2.get();
            p.agent_forwarding = agent2.get();
            let log = v(&f2.log);
            p.session_log = (!log.is_empty()).then(|| SessionLog {
                path: log,
                mode: if raw2.get() { LogMode::Raw } else { LogMode::Printable },
                append: true,
            });
            // Auth order: agent, key files (keeping saved passphrase ids),
            // password (keeping its vault id), keyboard-interactive.
            let old_pass = base2.auth.iter().find_map(|a| match a {
                AuthMethod::Password { credential } => credential.clone(),
                _ => None,
            });
            let mut auth = vec![AuthMethod::Agent];
            for k in v(&f2.keys).split(',').map(str::trim).filter(|k| !k.is_empty()) {
                let path: std::path::PathBuf = k.into();
                let passphrase_credential = base2.auth.iter().find_map(|a| match a {
                    AuthMethod::KeyFile { path: old, passphrase_credential } if *old == path => {
                        passphrase_credential.clone()
                    }
                    _ => None,
                });
                auth.push(AuthMethod::KeyFile { path, passphrase_credential });
            }
            auth.push(AuthMethod::Password { credential: old_pass });
            auth.push(AuthMethod::KeyboardInteractive);
            p.auth = auth;
            save2(p, window, cx);
            window.close_dialog(cx);
        });
        let (s1, s2) = (submit.clone(), submit.clone());
        dialog
            .title(if editing { "Edit host" } else { "New host" })
            .w(px(640.0))
            .child(body)
            .on_ok(move |_, window, cx| {
                s1(window, cx);
                false
            })
            .footer(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(Button::new("cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("save").primary().label("Save").on_click(move |_, window, cx| s2(window, cx))),
            )
    });
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob exports its own `test` attribute.
    use super::{forwards_to_text, jumps_to_text, parse_forwards, parse_jumps};

    #[test]
    fn forwards_and_jumps_round_trip() {
        let text = "L8080:localhost:80 R0.0.0.0:9000:localhost:3000 D1080";
        let f = parse_forwards(text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.len(), 3);
        assert_eq!(forwards_to_text(&f), text);
        assert_eq!(
            parse_forwards("-L 8080:a:80, -D 1080").map(|v| v.len()),
            Ok(2)
        );
        assert!(parse_forwards("X1").is_err());

        let j = parse_jumps("ops@bastion:2222, inner").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(jumps_to_text(&j), "ops@bastion:2222, inner");
        assert!(parse_jumps("user@").is_err());
    }
}
