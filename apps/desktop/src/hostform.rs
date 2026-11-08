//! "New host" / "Edit host" dialog: every profile setting, in sections
//! (General, Authentication, Connection, Terminal, Files, Advanced), with
//! validation before saving.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::accesskit::Role;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use hostwick_core::{
    AuthMethod, Endpoint, FileProtocol, FlowControl, ForwardSpec, HostProfile, LogMode, Parity,
    Protocol, ProxyKind, ProxySpec, SerialSettings, SessionLog, SudoOptions, Target,
};
use hostwick_term::PortInfo;

use crate::backend::Backend;
use crate::editors::SnippetEditor;
use crate::vault::{CredKind, existing_id};

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
                .ok_or_else(|| t!("form-forward-needs-spec", token = tok).to_string())?
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
            let target = Target::parse(t).map_err(|e| {
                t!("form-jump-invalid", host = t, error = e.to_string()).to_string()
            })?;
            Ok(Endpoint {
                port: target.port_or_default(),
                host: target.host,
                user: target.user,
            })
        })
        .collect()
}

/// A TCP port from a form field (`what` names it in the message).
pub fn parse_port(text: &str, what: &str) -> Result<u16, String> {
    text.trim()
        .parse::<u16>()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| t!("form-port-invalid", field = what).to_string())
}

/// Serial line settings from the form's fields.
pub fn parse_serial(
    baud: &str,
    data_bits: u8,
    parity: Parity,
    stop_bits: u8,
    flow: FlowControl,
) -> Result<SerialSettings, String> {
    let baud = baud
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|b| (50..=4_000_000).contains(b))
        .ok_or_else(|| t!("form-baud-invalid").to_string())?;
    if !(5..=8).contains(&data_bits) || !(1..=2).contains(&stop_bits) {
        return Err(t!("form-bits-invalid").into());
    }
    Ok(SerialSettings {
        baud,
        data_bits,
        parity,
        stop_bits,
        flow,
    })
}

/// Login-script problems for the inline message.
pub fn script_problem(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    hostwick_core::parse_login_script(text).err()
}

const BAUDS: [u32; 10] = [
    1200, 2400, 4800, 9600, 19_200, 38_400, 57_600, 115_200, 230_400, 921_600,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Auth,
    Connection,
    Terminal,
    Files,
    Advanced,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::General,
        Section::Auth,
        Section::Connection,
        Section::Terminal,
        Section::Files,
        Section::Advanced,
    ];

    fn label(self) -> SharedString {
        match self {
            Section::General => t!("host-section-general"),
            Section::Auth => t!("host-section-auth"),
            Section::Connection => t!("host-section-connection"),
            Section::Terminal => t!("host-section-terminal"),
            Section::Files => t!("host-section-files"),
            Section::Advanced => t!("host-section-advanced"),
        }
    }
}

/// The environment picker's choices (a custom colour has its own field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvChoice {
    None,
    Dev,
    Staging,
    Prod,
    Custom,
}

impl EnvChoice {
    fn of(e: &hostwick_core::Environment) -> Self {
        use hostwick_core::Environment as E;
        match e {
            E::None => Self::None,
            E::Dev => Self::Dev,
            E::Staging => Self::Staging,
            E::Prod => Self::Prod,
            E::Custom(_) => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtoChoice {
    Ssh,
    Telnet,
    Serial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyChoice {
    None,
    Socks5,
    Socks4,
    Http,
}

type OnSave = Rc<dyn Fn(HostProfile, &mut Window, &mut App)>;

pub struct HostForm {
    backend: Arc<Backend>,
    base: HostProfile,
    editing: bool,
    section: Section,
    // General
    name: Entity<InputState>,
    folder: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    notes: Entity<TextareaState>,
    env: EnvChoice,
    env_color: Entity<InputState>,
    proto: ProtoChoice,
    // Serial
    baud: Entity<InputState>,
    data_bits: u8,
    parity: Parity,
    stop_bits: u8,
    flow: FlowControl,
    ports: Option<Vec<PortInfo>>,
    // Authentication
    use_agent: bool,
    use_password: bool,
    use_kbd: bool,
    use_kerberos: bool,
    kerberos_delegate: bool,
    keys: Entity<InputState>,
    forget: Vec<CredKind>,
    // Connection
    jump: Entity<InputState>,
    forwards: Entity<InputState>,
    keepalive: Entity<InputState>,
    proxy: ProxyChoice,
    proxy_host: Entity<InputState>,
    proxy_port: Entity<InputState>,
    proxy_user: Entity<InputState>,
    agent_fwd: bool,
    x11: bool,
    x11_display: Entity<InputState>,
    // Terminal
    term: Entity<InputState>,
    script: Entity<TextareaState>,
    shell_integration: bool,
    snippets: Entity<SnippetEditor>,
    log: Entity<InputState>,
    raw_log: bool,
    // Files
    file_protocol: FileProtocol,
    sudo: bool,
    sudo_user: Entity<InputState>,
    sftp_server: Entity<InputState>,
    /// "Test connection" for the proxy: running, and the last result
    /// (ok, text).
    proxy_testing: bool,
    proxy_test: Option<(bool, String)>,
    // Advanced
    legacy: bool,
    error: Option<(Section, String)>,
    on_save: OnSave,
    _subs: Vec<Subscription>,
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

impl HostForm {
    fn new(
        base: HostProfile,
        editing: bool,
        backend: Arc<Backend>,
        on_save: OnSave,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mk = |value: String, ph: SharedString, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(ph)
                    .default_value(value)
            })
        };
        let serial = match &base.protocol {
            Protocol::Serial(s) => s.clone(),
            _ => SerialSettings::default(),
        };
        let proxy = base.proxy.clone();
        let has = |f: fn(&AuthMethod) -> bool| base.auth.iter().any(f);
        let kerberos_delegate = base
            .auth
            .iter()
            .any(|a| matches!(a, AuthMethod::Gssapi { delegate: true }));
        let mut form = Self {
            name: mk(base.name.clone(), "prod-web-1".into(), window, cx),
            folder: mk(
                base.folder.clone(),
                t!("host-folder-placeholder"),
                window,
                cx,
            ),
            host: mk(base.host.clone(), t!("host-host-placeholder"), window, cx),
            port: mk(base.port.to_string(), "22".into(), window, cx),
            user: mk(
                base.user.clone().unwrap_or_default(),
                t!("host-user-placeholder"),
                window,
                cx,
            ),
            notes: {
                let n = base.notes.clone();
                cx.new(|cx| TextareaState::new(window, cx).rows(3).default_value(n))
            },
            env: EnvChoice::of(&base.environment),
            env_color: mk(
                match &base.environment {
                    hostwick_core::Environment::Custom(c) => c.clone(),
                    _ => String::new(),
                },
                "#8a5cf6".into(),
                window,
                cx,
            ),
            proto: match base.protocol {
                Protocol::Ssh => ProtoChoice::Ssh,
                Protocol::Telnet => ProtoChoice::Telnet,
                Protocol::Serial(_) => ProtoChoice::Serial,
            },
            baud: mk(serial.baud.to_string(), "9600".into(), window, cx),
            data_bits: serial.data_bits,
            parity: serial.parity,
            stop_bits: serial.stop_bits,
            flow: serial.flow,
            ports: None,
            use_agent: has(|a| matches!(a, AuthMethod::Agent)),
            use_password: has(|a| matches!(a, AuthMethod::Password { .. })),
            use_kbd: has(|a| matches!(a, AuthMethod::KeyboardInteractive)),
            use_kerberos: has(|a| matches!(a, AuthMethod::Gssapi { .. })),
            kerberos_delegate,
            keys: mk(
                key_paths(&base),
                "~/.ssh/id_ed25519, C:\\keys\\work.ppk".into(),
                window,
                cx,
            ),
            forget: Vec::new(),
            jump: mk(
                jumps_to_text(&base.jump),
                "user@bastion:22, inner-host".into(),
                window,
                cx,
            ),
            forwards: mk(
                forwards_to_text(&base.forwards),
                "L8080:localhost:80 R9000:localhost:3000 D1080".into(),
                window,
                cx,
            ),
            keepalive: mk(
                base.keepalive_secs.to_string(),
                t!("host-keepalive-placeholder"),
                window,
                cx,
            ),
            proxy: match proxy.as_ref().map(|p| p.kind) {
                None => ProxyChoice::None,
                Some(ProxyKind::Socks5) => ProxyChoice::Socks5,
                Some(ProxyKind::Socks4) => ProxyChoice::Socks4,
                Some(ProxyKind::Http) => ProxyChoice::Http,
            },
            proxy_host: mk(
                proxy.as_ref().map(|p| p.host.clone()).unwrap_or_default(),
                "proxy.example.com".into(),
                window,
                cx,
            ),
            proxy_port: mk(
                proxy
                    .as_ref()
                    .map(|p| p.port.to_string())
                    .unwrap_or_default(),
                "1080".into(),
                window,
                cx,
            ),
            proxy_user: mk(
                proxy
                    .as_ref()
                    .and_then(|p| p.user.clone())
                    .unwrap_or_default(),
                t!("host-optional-placeholder"),
                window,
                cx,
            ),
            agent_fwd: base.agent_forwarding,
            x11: base.x11_forwarding,
            x11_display: mk(
                base.x11_display.clone().unwrap_or_default(),
                if cfg!(windows) {
                    t!("host-x11-placeholder-windows")
                } else {
                    t!("host-x11-placeholder")
                },
                window,
                cx,
            ),
            term: mk(base.term.clone(), "xterm-256color".into(), window, cx),
            script: {
                let s = base.login_script.clone();
                cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(5)
                        .placeholder(format!(
                            "# {}\n# sudo -i\n# @wait #\n# cd /srv/app",
                            t!("host-script-example")
                        ))
                        .default_value(s)
                })
            },
            shell_integration: base.shell_integration,
            snippets: cx.new(|cx| {
                SnippetEditor::new(&base.snippets, t!("host-snippets-empty"), window, cx)
            }),
            log: mk(
                base.session_log
                    .as_ref()
                    .map(|l| l.path.clone())
                    .unwrap_or_default(),
                t!("host-log-placeholder"),
                window,
                cx,
            ),
            raw_log: base
                .session_log
                .as_ref()
                .is_some_and(|l| l.mode == LogMode::Raw),
            file_protocol: base.file_protocol,
            sudo: base.sudo.enabled,
            sudo_user: mk(base.sudo.user.clone(), "root".into(), window, cx),
            sftp_server: mk(
                base.sudo.sftp_server.clone(),
                t!("host-sftp-server-placeholder"),
                window,
                cx,
            ),
            legacy: base.allow_legacy_algorithms,
            proxy_testing: false,
            proxy_test: None,
            error: None,
            on_save,
            section: Section::General,
            editing,
            backend,
            base,
            _subs: Vec::new(),
        };
        // Re-render on edits that drive inline messages.
        let script = form.script.clone();
        form._subs
            .push(cx.subscribe(&script, |f, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    if matches!(f.error, Some((Section::Terminal, _))) {
                        f.error = None;
                    }
                    cx.notify();
                }
            }));
        let x = form.x11_display.clone();
        form._subs
            .push(cx.subscribe(&x, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }));
        if form.proto == ProtoChoice::Serial {
            form.refresh_ports(window, cx);
        }
        form
    }

    fn refresh_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ports = None;
        let job = self.backend.rt.spawn_blocking(hostwick_term::list_ports);
        cx.spawn_in(window, async move |this, cx| {
            let ports = job.await.unwrap_or_default();
            let _ = this.update(cx, |f, cx| {
                f.ports = Some(ports);
                cx.notify();
            });
        })
        .detach();
    }

    fn set_proto(&mut self, p: ProtoChoice, window: &mut Window, cx: &mut Context<Self>) {
        if p == self.proto {
            return;
        }
        // Port default follows the protocol unless the person changed it.
        let port = self.port.read(cx).value().trim().to_owned();
        let default_for = |p: ProtoChoice| if p == ProtoChoice::Telnet { "23" } else { "22" };
        if port.is_empty() || port == default_for(self.proto) {
            let new = default_for(p);
            self.port.update(cx, |s, cx| s.set_value(new, window, cx));
        }
        if p == ProtoChoice::Serial && self.ports.is_none() {
            self.refresh_ports(window, cx);
        }
        let ph: SharedString = match p {
            ProtoChoice::Serial if cfg!(windows) => "COM3".into(),
            ProtoChoice::Serial => "/dev/ttyUSB0".into(),
            _ => t!("host-host-placeholder"),
        };
        self.host
            .update(cx, |s, cx| s.set_placeholder(ph, window, cx));
        self.proto = p;
        cx.notify();
    }

    fn val(&self, i: &Entity<InputState>, cx: &App) -> String {
        i.read(cx).value().trim().to_owned()
    }

    /// Build the profile, or the section and message of the first problem.
    fn build(&self, cx: &App) -> Result<HostProfile, (Section, String)> {
        use Section::*;
        let v = |i: &Entity<InputState>| self.val(i, cx);
        let (name, host) = (v(&self.name), v(&self.host));
        let serial = self.proto == ProtoChoice::Serial;
        if host.is_empty() {
            return Err((
                General,
                if serial {
                    t!("host-error-no-device").into()
                } else {
                    t!("host-error-no-host").into()
                },
            ));
        }
        let mut p = self.base.clone();
        p.name = if name.is_empty() { host.clone() } else { name };
        p.folder = v(&self.folder).trim_matches('/').to_owned();
        p.host = host;
        p.notes = self.notes.read(cx).value().trim_end().to_owned();
        p.environment = match self.env {
            EnvChoice::None => hostwick_core::Environment::None,
            EnvChoice::Dev => hostwick_core::Environment::Dev,
            EnvChoice::Staging => hostwick_core::Environment::Staging,
            EnvChoice::Prod => hostwick_core::Environment::Prod,
            EnvChoice::Custom => {
                let c = v(&self.env_color);
                if hostwick_core::parse_hex_color(&c).is_none() {
                    return Err((General, t!("tx-env-error-color").into()));
                }
                hostwick_core::Environment::Custom(c)
            }
        };
        p.protocol = match self.proto {
            ProtoChoice::Ssh => Protocol::Ssh,
            ProtoChoice::Telnet => Protocol::Telnet,
            ProtoChoice::Serial => Protocol::Serial(
                parse_serial(
                    &v(&self.baud),
                    self.data_bits,
                    self.parity,
                    self.stop_bits,
                    self.flow,
                )
                .map_err(|e| (General, e))?,
            ),
        };
        if !serial {
            p.port = parse_port(&v(&self.port), &t!("host-port")).map_err(|e| (General, e))?;
        }
        p.user = Some(v(&self.user)).filter(|u| !u.is_empty());

        // Authentication: Kerberos, agent, keys, password, keyboard-interactive.
        let mut auth = Vec::new();
        if self.use_kerberos {
            auth.push(AuthMethod::Gssapi {
                delegate: self.kerberos_delegate,
            });
        }
        if self.use_agent {
            auth.push(AuthMethod::Agent);
        }
        for k in v(&self.keys)
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            let path: std::path::PathBuf = k.into();
            let kind = CredKind::KeyPassphrase(path.clone());
            let passphrase_credential = if self.forget.contains(&kind) {
                None
            } else {
                existing_id(&self.base, &kind)
            };
            auth.push(AuthMethod::KeyFile {
                path,
                passphrase_credential,
            });
        }
        if self.use_password {
            let credential = if self.forget.contains(&CredKind::Password) {
                None
            } else {
                existing_id(&self.base, &CredKind::Password)
            };
            auth.push(AuthMethod::Password { credential });
        }
        if self.use_kbd {
            auth.push(AuthMethod::KeyboardInteractive);
        }
        if auth.is_empty() && self.proto == ProtoChoice::Ssh {
            return Err((Auth, t!("host-error-no-auth").into()));
        }
        if !auth.is_empty() {
            p.auth = auth;
        }

        p.jump = parse_jumps(&v(&self.jump)).map_err(|e| (Connection, e))?;
        p.forwards = parse_forwards(&v(&self.forwards))
            .map_err(|e| (Connection, t!("host-error-forwards", error = e).into()))?;
        p.keepalive_secs = v(&self.keepalive)
            .parse::<u32>()
            .ok()
            .filter(|k| *k <= 3600)
            .ok_or_else(|| (Connection, t!("host-error-keepalive").to_string()))?;
        p.proxy = self.proxy_spec(cx).map_err(|e| (Connection, e))?;
        p.agent_forwarding = self.agent_fwd;
        p.x11_forwarding = self.x11;
        let display = v(&self.x11_display);
        if !display.is_empty() {
            hostwick_ssh::X11Display::parse(&display).map_err(|e| {
                (
                    Connection,
                    t!("host-error-x11", error = e.to_string()).into(),
                )
            })?;
        }
        p.x11_display = Some(display).filter(|d| !d.is_empty());

        let term = v(&self.term);
        p.term = if term.is_empty() {
            "xterm-256color".into()
        } else {
            term
        };
        let script = self.script.read(cx).value().to_string();
        if let Some(e) = script_problem(&script) {
            return Err((Terminal, t!("host-error-script", error = e).into()));
        }
        p.login_script = script.trim_end().to_owned();
        p.shell_integration = self.shell_integration;
        p.snippets = self.snippets.read(cx).values(cx);
        let log = v(&self.log);
        p.session_log = (!log.is_empty()).then_some(SessionLog {
            path: log,
            mode: if self.raw_log {
                LogMode::Raw
            } else {
                LogMode::Printable
            },
            append: true,
        });

        p.file_protocol = self.file_protocol;
        p.sudo = SudoOptions {
            enabled: self.sudo,
            user: v(&self.sudo_user),
            sftp_server: v(&self.sftp_server),
            password_credential: if self.forget.contains(&CredKind::Sudo) {
                None
            } else {
                self.base.sudo.password_credential.clone()
            },
        };
        p.allow_legacy_algorithms = self.legacy;
        Ok(p)
    }

    /// The proxy as the form describes it (None = no proxy).
    fn proxy_spec(&self, cx: &App) -> Result<Option<ProxySpec>, String> {
        let v = |i: &Entity<InputState>| self.val(i, cx);
        let kind = match self.proxy {
            ProxyChoice::None => return Ok(None),
            ProxyChoice::Socks4 => ProxyKind::Socks4,
            ProxyChoice::Http => ProxyKind::Http,
            ProxyChoice::Socks5 => ProxyKind::Socks5,
        };
        let host = v(&self.proxy_host);
        if host.is_empty() {
            return Err(t!("host-error-proxy-host").into());
        }
        let port_text = v(&self.proxy_port);
        let port = if port_text.is_empty() {
            kind.default_port()
        } else {
            parse_port(&port_text, &t!("host-proxy-port"))?
        };
        let user = Some(v(&self.proxy_user)).filter(|u| !u.is_empty());
        let keep = self.base.proxy.as_ref().filter(|old| {
            old.host == host && old.user == user && !self.forget.contains(&CredKind::Proxy)
        });
        Ok(Some(ProxySpec {
            kind,
            host,
            port,
            password_credential: keep.and_then(|o| o.password_credential.clone()),
            user,
        }))
    }

    /// "Test connection": through the proxy to the first hop (the first
    /// jump host, else the host) on its port, off the UI thread.
    fn test_proxy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.proxy_testing {
            return;
        }
        let spec = match self.proxy_spec(cx) {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                self.proxy_test = Some((false, e));
                cx.notify();
                return;
            }
        };
        let jumps = parse_jumps(&self.val(&self.jump, cx));
        let target = match jumps.as_deref() {
            Ok([first, ..]) if self.proto == ProtoChoice::Ssh => {
                Ok((first.host.clone(), first.port))
            }
            Err(e) => Err(e.clone()),
            _ => {
                let host = self.val(&self.host, cx);
                if host.is_empty() {
                    Err(t!("host-error-no-host").to_string())
                } else {
                    parse_port(&self.val(&self.port, cx), &t!("host-port")).map(|p| (host, p))
                }
            }
        };
        let (host, port) = match target {
            Ok(t) => t,
            Err(e) => {
                self.proxy_test = Some((false, e));
                cx.notify();
                return;
            }
        };
        self.proxy_testing = true;
        self.proxy_test = None;
        let job = self
            .backend
            .rt
            .spawn(self.backend.clone().test_proxy(spec, host, port));
        cx.spawn_in(window, async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |f, cx| {
                f.proxy_testing = false;
                f.proxy_test = Some(match res {
                    Ok(Ok(text)) => (true, text),
                    Ok(Err(e)) => (false, e),
                    Err(e) => (false, t!("error-internal", error = e.to_string()).into()),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.build(cx) {
            Ok(p) => {
                // Forgotten secrets also leave the vault (when it is open).
                let forgotten: Vec<String> = self
                    .forget
                    .iter()
                    .filter_map(|k| existing_id(&self.base, k))
                    .filter(|id| !crate::vault::profile_ids(&p).iter().any(|(_, x)| x == id))
                    .collect();
                if !forgotten.is_empty() && self.backend.vault.is_unlocked() {
                    let vault = self.backend.vault.clone();
                    let owner = p.id;
                    self.backend.rt.spawn_blocking(move || {
                        for id in forgotten {
                            // Only this host's own secrets: key passphrases
                            // and shared ids may serve other hosts too.
                            if crate::vault::owned_by(&id, owner)
                                && let Err(e) = vault.remove(&id)
                            {
                                tracing::warn!("could not remove a saved password: {e}");
                            }
                        }
                    });
                }
                (self.on_save)(p, window, cx);
                window.defer(cx, |window, cx| window.close_dialog(cx));
            }
            Err((section, msg)) => {
                self.section = section;
                self.error = Some((section, msg));
                cx.notify();
            }
        }
    }

    // ---- rendering ----------------------------------------------------------

    fn row(label: impl Into<SharedString>, el: impl IntoElement) -> Div {
        let label: SharedString = label.into();
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(130.0))
                    .flex_none()
                    .text_size(px(13.0))
                    .child(label.clone()),
            )
            // Named after the label, so a screen reader says what a field,
            // or a row of choices, is for.
            .child(
                div()
                    .id(ElementId::Name(label.clone()))
                    .role(Role::Group)
                    .aria_label(label)
                    .flex_1()
                    .min_w_0()
                    .child(el),
            )
    }

    fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
        div()
            .pl(px(138.0))
            .text_size(px(11.0))
            .whitespace_normal()
            .text_color(cx.theme().muted_foreground)
            .child(crate::a11y::text(text))
    }

    fn check(
        id: &'static str,
        label: SharedString,
        value: bool,
        cx: &mut Context<Self>,
        set: fn(&mut Self, bool),
    ) -> Div {
        div().pl(px(138.0)).child(
            crate::a11y::checkbox(id, label, false, cx)
                .checked(value)
                .on_click(cx.listener(move |f, v: &bool, _, cx| {
                    set(f, *v);
                    cx.notify();
                })),
        )
    }

    fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        current: T,
        options: &[(T, SharedString)],
        cx: &mut Context<Self>,
        set: fn(&mut Self, T, &mut Window, &mut Context<Self>),
    ) -> Div {
        let mut row = div().flex().gap(px(4.0)).flex_wrap();
        for (i, (value, label)) in options.iter().cloned().enumerate() {
            row = row.child(
                Button::new((id, i))
                    .xsmall()
                    .label(label)
                    .toggled(current == value)
                    .when(current == value, |b| b.primary())
                    .on_click(cx.listener(move |f, _, w, cx| {
                        set(f, value, w, cx);
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// "Saved in your vault · Forget" for a credential the host refers to.
    fn saved_secret(
        &self,
        kind: CredKind,
        what: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let id = existing_id(&self.base, &kind)?;
        let theme = cx.theme().clone();
        let forgotten = self.forget.contains(&kind);
        let unlocked = self.backend.vault.is_unlocked();
        let text = if forgotten {
            t!("host-saved-forget-pending", what = what)
        } else if !unlocked {
            t!("host-saved-locked", what = what)
        } else if self.backend.vault.has(&id) {
            t!("host-saved", what = what)
        } else {
            t!("host-saved-missing", what = what)
        };
        let k = kind.clone();
        Some(
            div()
                .pl(px(138.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(12.0))
                .text_color(theme.muted_foreground)
                .child(text)
                .child(
                    Button::new(SharedString::from(format!("forget-{kind:?}")))
                        .xsmall()
                        .ghost()
                        .label(if forgotten {
                            t!("host-saved-keep")
                        } else {
                            t!("host-saved-forget")
                        })
                        .on_click(cx.listener(move |f, _, _, cx| {
                            if let Some(i) = f.forget.iter().position(|x| *x == k) {
                                f.forget.remove(i);
                            } else {
                                f.forget.push(k.clone());
                            }
                            cx.notify();
                        })),
                ),
        )
    }

    fn browse_keys(&self) -> Button {
        let key_input = self.keys.clone();
        Button::new("browse-key")
            .small()
            .label(t!("button-browse"))
            .on_click(move |_, window, cx| {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some(t!("host-pick-key")),
                });
                let input = key_input.clone();
                window
                    .spawn(cx, async move |cx| {
                        let Ok(Ok(Some(paths))) = picked.await else {
                            return;
                        };
                        let _ = cx.update(|window, cx| {
                            let current = input.read(cx).value().trim().to_owned();
                            let mut list: Vec<String> = current
                                .split(',')
                                .map(|s| s.trim().to_owned())
                                .filter(|s| !s.is_empty())
                                .collect();
                            for p in paths {
                                let s = p.display().to_string();
                                if !list.contains(&s) {
                                    list.push(s);
                                }
                            }
                            input.update(cx, |s, cx| s.set_value(list.join(", "), window, cx));
                        });
                    })
                    .detach();
            })
    }

    fn render_general(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let serial = self.proto == ProtoChoice::Serial;
        let body = body
            .child(Self::row(
                t!("host-name"),
                Input::new(&self.name).small().aria_label(t!("host-name")),
            ))
            .child(Self::row(
                t!("host-folder"),
                Input::new(&self.folder)
                    .small()
                    .aria_label(t!("host-folder")),
            ))
            .child(Self::row(
                t!("host-protocol"),
                Self::choice(
                    "proto",
                    self.proto,
                    &[
                        (ProtoChoice::Ssh, t!("protocol-ssh")),
                        (ProtoChoice::Telnet, t!("protocol-telnet")),
                        (ProtoChoice::Serial, t!("protocol-serial")),
                    ],
                    cx,
                    |f, v, w, cx| f.set_proto(v, w, cx),
                ),
            ));
        let body = if serial {
            self.render_serial(body, cx)
        } else {
            body.child(Self::row(
                t!("host-host"),
                Input::new(&self.host).small().aria_label(t!("host-host")),
            ))
            .child(Self::row(
                t!("host-port"),
                div()
                    .w(px(120.0))
                    .child(Input::new(&self.port).small().aria_label(t!("host-port"))),
            ))
            .child(Self::row(
                t!("host-user"),
                Input::new(&self.user).small().aria_label(t!("host-user")),
            ))
            .when(self.proto == ProtoChoice::Telnet, |d| {
                d.child(Self::hint(t!("host-telnet-hint"), cx))
            })
        };
        body.child(Self::row(
            t!("tx-env-label"),
            Self::choice(
                "env",
                self.env,
                &[
                    (EnvChoice::None, t!("tx-env-none")),
                    (EnvChoice::Dev, t!("tx-env-dev")),
                    (EnvChoice::Staging, t!("tx-env-staging")),
                    (EnvChoice::Prod, t!("tx-env-prod")),
                    (EnvChoice::Custom, t!("tx-env-custom")),
                ],
                cx,
                |f, v, _, _| f.env = v,
            ),
        ))
        .when(self.env == EnvChoice::Custom, |d| {
            d.child(Self::row(
                t!("tx-env-color"),
                div().w(px(160.0)).child(
                    Input::new(&self.env_color)
                        .small()
                        .aria_label(t!("tx-env-color")),
                ),
            ))
        })
        .child(Self::hint(t!("tx-env-hint"), cx))
        .child(Self::row(
            t!("host-notes"),
            Textarea::new(&self.notes)
                .small()
                .aria_label(t!("host-notes")),
        ))
    }

    fn render_serial(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let ports = self.ports.clone();
        let host = self.host.clone();
        let picker = Button::new("ports")
            .small()
            .label(match &ports {
                None => t!("host-ports-looking"),
                Some(p) if p.is_empty() => t!("host-ports-none"),
                Some(p) => t!("host-ports-count", count = p.len()),
            })
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let mut menu = menu;
                match &ports {
                    Some(list) if !list.is_empty() => {
                        for p in list {
                            let (name, host) = (p.name.clone(), host.clone());
                            let label = if p.description.is_empty() {
                                p.name.clone()
                            } else {
                                format!("{} — {}", p.name, p.description)
                            };
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    let name = name.clone();
                                    host.update(cx, |s, cx| s.set_value(name, window, cx));
                                },
                            ));
                        }
                    }
                    _ => {
                        menu = menu
                            .item(PopupMenuItem::new(t!("host-ports-none-menu")).disabled(true));
                    }
                }
                menu
            });
        let baud_input = self.baud.clone();
        let bauds = Button::new("bauds")
            .small()
            .icon(IconName::ChevronDown)
            .accessibility_label(t!("host-baud-common"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let mut menu = menu;
                for b in BAUDS {
                    let input = baud_input.clone();
                    menu = menu.item(PopupMenuItem::new(b.to_string()).on_click(
                        move |_, window, cx| {
                            input.update(cx, |s, cx| s.set_value(b.to_string(), window, cx))
                        },
                    ));
                }
                menu
            });
        body.child(Self::row(
            t!("host-device"),
            div()
                .flex()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&self.host).small().aria_label(t!("host-device"))),
                )
                .child(picker)
                .child(
                    Button::new("refresh-ports")
                        .small()
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip(t!("host-ports-refresh"))
                        .accessibility_label(t!("host-ports-refresh"))
                        .on_click(cx.listener(|f, _, w, cx| f.refresh_ports(w, cx))),
                ),
        ))
        .child(Self::hint(
            if cfg!(windows) {
                t!("host-device-hint-windows")
            } else {
                t!("host-device-hint")
            },
            cx,
        ))
        .child(Self::row(
            t!("host-baud"),
            div()
                .flex()
                .gap(px(4.0))
                .child(
                    div()
                        .w(px(120.0))
                        .child(Input::new(&self.baud).small().aria_label(t!("host-baud"))),
                )
                .child(bauds),
        ))
        .child(Self::row(
            t!("host-data-bits"),
            Self::choice(
                "bits",
                self.data_bits,
                &[
                    (5, "5".into()),
                    (6, "6".into()),
                    (7, "7".into()),
                    (8, "8".into()),
                ],
                cx,
                |f, v, _, _| f.data_bits = v,
            ),
        ))
        .child(Self::row(
            t!("host-parity"),
            Self::choice(
                "parity",
                self.parity,
                &[
                    (Parity::None, t!("host-parity-none")),
                    (Parity::Odd, t!("host-parity-odd")),
                    (Parity::Even, t!("host-parity-even")),
                ],
                cx,
                |f, v, _, _| f.parity = v,
            ),
        ))
        .child(Self::row(
            t!("host-stop-bits"),
            Self::choice(
                "stop",
                self.stop_bits,
                &[(1, "1".into()), (2, "2".into())],
                cx,
                |f, v, _, _| f.stop_bits = v,
            ),
        ))
        .child(Self::row(
            t!("host-flow"),
            Self::choice(
                "flow",
                self.flow,
                &[
                    (FlowControl::None, t!("host-flow-none")),
                    (FlowControl::Software, "XON/XOFF".into()),
                    (FlowControl::Hardware, "RTS/CTS".into()),
                ],
                cx,
                |f, v, _, _| f.flow = v,
            ),
        ))
        .child(
            div()
                .pl(px(138.0))
                .text_size(px(11.0))
                .text_color(theme.muted_foreground)
                .child(match self.ports.as_ref().map(|p| p.len()) {
                    Some(0) if cfg!(target_os = "linux") => t!("host-serial-none-linux"),
                    _ => t!("host-serial-hint"),
                }),
        )
    }

    fn render_auth(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        if self.proto != ProtoChoice::Ssh {
            return body.child(Self::hint(t!("host-auth-not-ssh"), cx).pl(px(0.0)));
        }
        let pw = self.saved_secret(CredKind::Password, t!("host-what-password"), cx);
        let keys_text = self.val(&self.keys, cx);
        let key_saved: Vec<Div> = keys_text
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .filter_map(|k| {
                let kind = CredKind::KeyPassphrase(k.into());
                existing_id(&self.base, &kind)?;
                self.saved_secret(kind, t!("host-what-key-passphrase"), cx)
            })
            .collect();
        body.child(Self::row(
            t!("host-key-files"),
            div()
                .flex()
                .gap(px(6.0))
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.keys)
                            .small()
                            .aria_label(t!("host-key-files")),
                    ),
                )
                .child(self.browse_keys()),
        ))
        .child(Self::hint(t!("host-keys-hint"), cx))
        .children(key_saved)
        .child(Self::check(
            "auth-agent",
            t!("host-auth-agent"),
            self.use_agent,
            cx,
            |f, v| f.use_agent = v,
        ))
        .child(Self::check(
            "auth-pw",
            t!("host-auth-password"),
            self.use_password,
            cx,
            |f, v| f.use_password = v,
        ))
        .children(pw)
        .when(
            self.use_password && existing_id(&self.base, &CredKind::Password).is_none(),
            |d| d.child(Self::hint(t!("host-auth-remember-hint"), cx)),
        )
        .child(Self::check(
            "auth-kbd",
            t!("host-auth-kbd"),
            self.use_kbd,
            cx,
            |f, v| f.use_kbd = v,
        ))
        .child(Self::check(
            "auth-krb",
            t!("host-auth-kerberos"),
            self.use_kerberos,
            cx,
            |f, v| f.use_kerberos = v,
        ))
        .when(self.use_kerberos, |d| {
            d.child(div().pl(px(24.0)).child(Self::check(
                "auth-krb-fwd",
                t!("host-auth-kerberos-forward"),
                self.kerberos_delegate,
                cx,
                |f, v| f.kerberos_delegate = v,
            )))
            .child(Self::hint(
                if cfg!(windows) {
                    t!("host-auth-kerberos-hint-windows")
                } else {
                    t!("host-auth-kerberos-hint")
                },
                cx,
            ))
        })
        .child(Self::hint(t!("host-auth-order"), cx))
    }

    fn render_connection(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let ssh = self.proto == ProtoChoice::Ssh;
        let serial = self.proto == ProtoChoice::Serial;
        if serial {
            return body.child(Self::hint(t!("host-connection-serial"), cx).pl(px(0.0)));
        }
        let display = self.val(&self.x11_display, cx);
        let display_err = (!display.is_empty())
            .then(|| hostwick_ssh::X11Display::parse(&display).err())
            .flatten();
        let proxy_saved = self.saved_secret(CredKind::Proxy, t!("host-what-proxy-password"), cx);
        let body = body
            .child(Self::row(
                t!("host-proxy"),
                Self::choice(
                    "proxy",
                    self.proxy,
                    &[
                        (ProxyChoice::None, t!("host-proxy-none")),
                        (ProxyChoice::Socks5, "SOCKS5".into()),
                        (ProxyChoice::Socks4, "SOCKS4".into()),
                        (ProxyChoice::Http, "HTTP".into()),
                    ],
                    cx,
                    |f, v, _, _| f.proxy = v,
                ),
            ))
            .when(self.proxy != ProxyChoice::None, |d| {
                d.child(Self::row(
                    t!("host-proxy-host-port"),
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            div().flex_1().child(
                                Input::new(&self.proxy_host)
                                    .small()
                                    .aria_label(t!("host-proxy")),
                            ),
                        )
                        .child(
                            div().w(px(90.0)).child(
                                Input::new(&self.proxy_port)
                                    .small()
                                    .aria_label(t!("host-proxy-port")),
                            ),
                        ),
                ))
                .when(self.proxy != ProxyChoice::Socks4, |d| {
                    d.child(Self::row(
                        t!("host-proxy-user"),
                        Input::new(&self.proxy_user)
                            .small()
                            .aria_label(t!("host-proxy-user")),
                    ))
                    .children(proxy_saved)
                    .child(Self::hint(t!("host-proxy-password-hint"), cx))
                })
                .child(
                    div()
                        .pl(px(138.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("proxy-test")
                                .small()
                                .label(if self.proxy_testing {
                                    t!("host-proxy-testing")
                                } else {
                                    t!("host-proxy-test")
                                })
                                .disabled(self.proxy_testing)
                                .tooltip(t!("host-proxy-test-tooltip"))
                                .on_click(cx.listener(|f, _, w, cx| f.test_proxy(w, cx))),
                        )
                        .when_some(self.proxy_test.clone(), |d, (ok, text)| {
                            d.child(
                                div()
                                    .id("proxy-test-result")
                                    .role(Role::Status)
                                    .aria_label(text.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.0))
                                    .whitespace_normal()
                                    .text_color(if ok { theme.success } else { theme.danger })
                                    .child(text),
                            )
                        }),
                )
            });
        if !ssh {
            return body;
        }
        body.child(Self::row(
            t!("host-jump-hosts"),
            Input::new(&self.jump)
                .small()
                .aria_label(t!("host-jump-hosts")),
        ))
        .child(Self::hint(t!("host-jump-hint"), cx))
        .child(Self::row(
            t!("host-forwards"),
            Input::new(&self.forwards)
                .small()
                .aria_label(t!("host-forwards")),
        ))
        .child(Self::hint(t!("host-forwards-hint"), cx))
        .child(Self::row(
            t!("host-keepalive"),
            div().w(px(120.0)).child(
                Input::new(&self.keepalive)
                    .small()
                    .aria_label(t!("host-keepalive")),
            ),
        ))
        .child(Self::check(
            "agent-fwd",
            t!("host-agent-forwarding"),
            self.agent_fwd,
            cx,
            |f, v| f.agent_fwd = v,
        ))
        .child(Self::hint(t!("host-agent-forwarding-hint"), cx))
        .child(Self::check("x11", t!("host-x11"), self.x11, cx, |f, v| {
            f.x11 = v
        }))
        .when(self.x11, |d| {
            d.child(Self::row(
                t!("host-x11-display"),
                Input::new(&self.x11_display)
                    .small()
                    .aria_label(t!("host-x11-display")),
            ))
            .when_some(display_err, |d, e| {
                d.child(
                    div()
                        .pl(px(138.0))
                        .text_size(px(11.0))
                        .text_color(theme.danger)
                        .child(e),
                )
            })
            .child(Self::hint(
                if cfg!(windows) {
                    t!("host-x11-hint-windows")
                } else if cfg!(target_os = "macos") {
                    t!("host-x11-hint-macos")
                } else {
                    t!("host-x11-hint")
                },
                cx,
            ))
        })
    }

    fn render_terminal(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let script = self.script.read(cx).value().to_string();
        let problem = script_problem(&script);
        body.child(Self::row(
            t!("host-terminal-type"),
            div().w(px(200.0)).child(
                Input::new(&self.term)
                    .small()
                    .aria_label(t!("host-terminal-type")),
            ),
        ))
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(130.0))
                        .flex_none()
                        .text_size(px(13.0))
                        .child(t!("host-login-script")),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Textarea::new(&self.script)
                            .small()
                            .aria_label(t!("host-login-script")),
                    ),
                ),
        )
        .child(match problem {
            Some(e) => div()
                .pl(px(138.0))
                .text_size(px(11.0))
                .text_color(theme.danger)
                .child(e),
            None => Self::hint(t!("host-login-script-hint"), cx),
        })
        .when(self.proto == ProtoChoice::Ssh, |d| {
            d.child(Self::check(
                "shell-int",
                t!("host-shell-integration"),
                self.shell_integration,
                cx,
                |f, v| f.shell_integration = v,
            ))
            .child(Self::hint(t!("host-shell-integration-hint"), cx))
        })
        .child(
            div()
                .pt(px(4.0))
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t!("host-snippets-title")),
        )
        .child(self.snippets.clone())
        .child(Self::row(
            t!("host-session-log"),
            Input::new(&self.log)
                .small()
                .aria_label(t!("host-session-log")),
        ))
        .child(Self::hint(t!("host-session-log-hint"), cx))
        .child(Self::check(
            "raw-log",
            t!("host-raw-log"),
            self.raw_log,
            cx,
            |f, v| f.raw_log = v,
        ))
    }

    fn render_files(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        if self.proto != ProtoChoice::Ssh {
            return body.child(Self::hint(t!("host-files-not-ssh"), cx).pl(px(0.0)));
        }
        let sudo_saved = self.saved_secret(CredKind::Sudo, t!("host-what-sudo-password"), cx);
        body.child(Self::row(
            t!("host-file-protocol"),
            Self::choice(
                "fproto",
                self.file_protocol,
                &[
                    (FileProtocol::Auto, t!("host-file-protocol-auto")),
                    (FileProtocol::Sftp, "SFTP".into()),
                    (FileProtocol::Scp, "SCP".into()),
                ],
                cx,
                |f, v, _, _| f.file_protocol = v,
            ),
        ))
        .child(Self::hint(t!("host-file-protocol-hint"), cx))
        .child(Self::check(
            "sudo",
            t!("host-sudo"),
            self.sudo,
            cx,
            |f, v| f.sudo = v,
        ))
        .child(Self::row(
            t!("host-sudo-user"),
            div().w(px(200.0)).child(
                Input::new(&self.sudo_user)
                    .small()
                    .aria_label(t!("host-sudo-user")),
            ),
        ))
        .child(Self::row(
            "sftp-server",
            Input::new(&self.sftp_server)
                .small()
                .aria_label("sftp-server"),
        ))
        .children(sudo_saved)
        .child(Self::hint(t!("host-sudo-hint"), cx))
    }

    fn render_advanced(&mut self, body: Div, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        body.child(Self::check(
            "legacy",
            t!("host-legacy"),
            self.legacy,
            cx,
            |f, v| f.legacy = v,
        ))
        .child(Self::hint(t!("host-legacy-hint"), cx))
        .when(self.editing, |d| {
            d.child(
                div()
                    .pt(px(8.0))
                    .text_size(px(11.0))
                    .text_color(theme.muted_foreground)
                    .child(t!("host-id", id = self.base.id.0.to_string())),
            )
        })
    }
}

impl Render for HostForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let current = self.section;
        let err_section = self.error.as_ref().map(|(s, _)| *s);
        let title = if self.editing {
            t!("host-title-edit")
        } else {
            t!("host-title-new")
        };
        // Sections: buttons, so Tab reaches them and Enter/Space opens one.
        let nav = div()
            .id("host-sections")
            .role(Role::Toolbar)
            .aria_label(t!("a11y-sections"))
            .w(px(150.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .pr(px(10.0))
            .border_r_1()
            .border_color(theme.border)
            .children(Section::ALL.iter().enumerate().map(|(i, s)| {
                let s = *s;
                let problem = err_section == Some(s);
                crate::a11y::section_button(("hsec", i), s.label(), s == current, cx)
                    .when(problem, |b| {
                        b.accessibility_label(t!("a11y-section-problem", section = s.label()))
                            .child(div().size(px(6.0)).rounded_full().bg(theme.danger))
                    })
                    .on_click(cx.listener(move |f, _, _, cx| {
                        f.section = s;
                        cx.notify();
                    }))
            }));
        let body = div().flex().flex_col().gap(px(8.0));
        let content = match self.section {
            Section::General => self.render_general(body, cx),
            Section::Auth => self.render_auth(body, cx),
            Section::Connection => self.render_connection(body, cx),
            Section::Terminal => self.render_terminal(body, cx),
            Section::Files => self.render_files(body, cx),
            Section::Advanced => self.render_advanced(body, cx),
        };
        div()
            .id("host-form")
            .role(Role::Group)
            .aria_label(title)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div().flex().h(px(480.0)).child(nav).child(
                    div()
                        .id("host-content")
                        .flex_1()
                        .min_w_0()
                        .pl(px(14.0))
                        .pr(px(6.0))
                        .overflow_y_scroll()
                        .child(content),
                ),
            )
            .when_some(self.error.clone(), |d, (_, e)| {
                d.child(
                    div()
                        .id("host-error")
                        .role(Role::Alert)
                        .aria_label(e.clone())
                        .aria_value(e.clone())
                        .text_size(px(13.0))
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }
}

/// Open the editor. `existing = None` creates a new host. `on_save` gets the
/// validated profile (same id when editing).
pub fn open(
    existing: Option<HostProfile>,
    backend: Arc<Backend>,
    window: &mut Window,
    cx: &mut App,
    on_save: impl Fn(HostProfile, &mut Window, &mut App) + 'static,
) {
    let editing = existing.is_some();
    let base = existing.unwrap_or_else(new_host_base);
    open_with(base, editing, backend, window, cx, on_save);
}

/// A new host, starting from `base` (quick connect's "Save as host").
pub fn open_new(
    base: HostProfile,
    backend: Arc<Backend>,
    window: &mut Window,
    cx: &mut App,
    on_save: impl Fn(HostProfile, &mut Window, &mut App) + 'static,
) {
    open_with(base, false, backend, window, cx, on_save);
}

/// An empty new host, with the key files chosen on the welcome screen.
fn new_host_base() -> HostProfile {
    let mut p = HostProfile::new("", "");
    let keys = crate::backend::default_keys();
    if !keys.is_empty() {
        p.auth = crate::hostops::replace_keys(&p.auth, &keys);
    }
    p
}

fn open_with(
    base: HostProfile,
    editing: bool,
    backend: Arc<Backend>,
    window: &mut Window,
    cx: &mut App,
    on_save: impl Fn(HostProfile, &mut Window, &mut App) + 'static,
) {
    let on_save: OnSave = Rc::new(on_save);
    let form = cx.new(|cx| HostForm::new(base, editing, backend, on_save, window, cx));
    let first = std::rc::Rc::new(std::cell::Cell::new(true));
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let input = form.read(cx).name.clone();
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        let (f1, f2) = (form.clone(), form.clone());
        dialog
            .title(crate::a11y::text(if editing {
                t!("host-title-edit")
            } else {
                t!("host-title-new")
            }))
            .w(px(860.0))
            .margin_top(px(40.0))
            .child(form.clone())
            // Enter (outside multi-line fields) saves; `save` closes the
            // dialog itself when the host is valid.
            .on_ok(move |_, window, cx| {
                f2.update(cx, |f, cx| f.save(window, cx));
                false
            })
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
                        Button::new("save")
                            .primary()
                            .label(t!("button-save"))
                            .on_click(move |_, window, cx| {
                                f1.update(cx, |f, cx| f.save(window, cx))
                            }),
                    ),
            )
    });
}

/// Add a key file to a saved host's authentication (key generator's "Use
/// in host"). False if it was already there.
pub fn add_key_to_profile(p: &mut HostProfile, path: std::path::PathBuf) -> bool {
    if p.auth
        .iter()
        .any(|a| matches!(a, AuthMethod::KeyFile { path: k, .. } if *k == path))
    {
        return false;
    }
    let at = p
        .auth
        .iter()
        .position(|a| {
            matches!(
                a,
                AuthMethod::Password { .. } | AuthMethod::KeyboardInteractive
            )
        })
        .unwrap_or(p.auth.len());
    p.auth.insert(
        at,
        AuthMethod::KeyFile {
            path,
            passphrase_credential: None,
        },
    );
    true
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob exports its own `test` attribute.
    use super::{
        add_key_to_profile, forwards_to_text, jumps_to_text, parse_forwards, parse_jumps,
        parse_port, parse_serial, script_problem,
    };
    use hostwick_core::{AuthMethod, FlowControl, HostProfile, Parity};

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

    #[test]
    fn field_validation() {
        assert_eq!(parse_port(" 2222 ", "Port"), Ok(2222));
        assert!(parse_port("0", "Port").is_err());
        assert!(parse_port("70000", "Port").is_err());
        assert!(parse_port("ssh", "Port").is_err());
        let s = parse_serial("115200", 8, Parity::None, 1, FlowControl::None)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.baud, 115_200);
        assert!(parse_serial("fast", 8, Parity::None, 1, FlowControl::None).is_err());
        assert!(parse_serial("9600", 9, Parity::None, 1, FlowControl::None).is_err());
        assert_eq!(script_problem(""), None);
        assert_eq!(script_problem("sudo -i\n@wait #\n@sleep 100"), None);
        assert!(script_problem("ls\n@bogus").is_some_and(|e| e.contains("line 2")));
    }

    #[test]
    fn use_key_in_host() {
        let mut p = HostProfile::new("web", "10.0.0.1");
        assert!(add_key_to_profile(&mut p, "/k/id_ed25519".into()));
        assert!(!add_key_to_profile(&mut p, "/k/id_ed25519".into()));
        // Before password and keyboard-interactive.
        assert!(matches!(p.auth[1], AuthMethod::KeyFile { .. }));
    }
}
