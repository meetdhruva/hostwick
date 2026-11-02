use hostwick_core::{ForwardSpec, HostProfile, ProfileSource};

use crate::raw::{RawSession, Value};
use crate::{ImportError, ImportResult, auth_with_key, percent_decode, split_folder};

const SESSIONS_KEY: &str = r"HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\";

/// Parse a regedit export of PuTTY's sessions
/// (`reg export HKCU\Software\SimonTatham\PuTTY\Sessions putty.reg`).
pub fn parse_putty_reg(text: &str) -> Result<ImportResult, ImportError> {
    if !text.contains("SimonTatham\\PuTTY\\Sessions") {
        return Err(ImportError::Format("PuTTY .reg"));
    }
    let mut sessions = Vec::new();
    let mut current: Option<RawSession> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some(s) = current.take() {
                sessions.push(s);
            }
            current = section
                .strip_prefix(SESSIONS_KEY)
                .filter(|n| !n.is_empty() && !n.contains('\\'))
                .map(|n| RawSession {
                    name: percent_decode(n),
                    ..Default::default()
                });
            continue;
        }
        let Some(cur) = current.as_mut() else {
            continue;
        };
        if let Some((k, v)) = parse_reg_line(line) {
            cur.insert(&k, v);
        }
    }
    if let Some(s) = current.take() {
        sessions.push(s);
    }
    Ok(convert(sessions))
}

/// `"Name"="string"` or `"Name"=dword:0000abcd`.
fn parse_reg_line(line: &str) -> Option<(String, Value)> {
    let rest = line.strip_prefix('"')?;
    let (name, rest) = rest.split_once("\"=")?;
    if let Some(hexval) = rest.strip_prefix("dword:") {
        return Some((
            name.to_owned(),
            Value::Dword(u32::from_str_radix(hexval.trim(), 16).ok()?),
        ));
    }
    let s = rest.strip_prefix('"')?.strip_suffix('"')?;
    // .reg string escaping: \\ and \"
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    Some((name.to_owned(), Value::Str(out)))
}

/// Read sessions straight from the registry (Windows only).
pub fn putty_from_registry() -> Result<ImportResult, ImportError> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let root = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\SimonTatham\PuTTY\Sessions")?;
        let mut sessions = Vec::new();
        for name in root.enum_keys().flatten() {
            let Ok(key) = root.open_subkey(&name) else {
                continue;
            };
            let mut raw = RawSession {
                name: percent_decode(&name),
                ..Default::default()
            };
            for field in [
                "HostName",
                "UserName",
                "Protocol",
                "PublicKeyFile",
                "PortForwardings",
                "ProxyHost",
                "ProxyUsername",
                "TerminalType",
            ] {
                if let Ok(v) = key.get_value::<String, _>(field) {
                    raw.insert(field, Value::Str(v));
                }
            }
            for field in [
                "PortNumber",
                "ProxyMethod",
                "ProxyPort",
                "PingIntervalSecs",
                "AgentFwd",
            ] {
                if let Ok(v) = key.get_value::<u32, _>(field) {
                    raw.insert(field, Value::Dword(v));
                }
            }
            sessions.push(raw);
        }
        Ok(convert(sessions))
    }
    #[cfg(not(windows))]
    {
        Err(ImportError::Unsupported(
            "reading the PuTTY registry; export it to a .reg file instead",
        ))
    }
}

fn convert(sessions: Vec<RawSession>) -> ImportResult {
    let mut out = ImportResult::default();
    for s in sessions {
        if s.name == "Default Settings" {
            continue;
        }
        let proto = s.str("Protocol").unwrap_or_else(|| "ssh".into());
        if proto != "ssh" {
            out.warnings.push(format!(
                "{}: skipped ({proto} sessions are not supported)",
                s.name
            ));
            continue;
        }
        let Some(host_field) = s.str("HostName") else {
            out.warnings
                .push(format!("{}: skipped (no host name)", s.name));
            continue;
        };
        // PuTTY allows `user@host` in HostName.
        let (user_in_host, host) = match host_field.rsplit_once('@') {
            Some((u, h)) => (Some(u.to_owned()), h.to_owned()),
            None => (None, host_field),
        };
        let (folder, name) = split_folder(&s.name);
        let mut p = HostProfile::new(name, host);
        p.folder = folder;
        p.port = s
            .num("PortNumber")
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n != 0)
            .unwrap_or(22);
        p.user = s.str("UserName").or(user_in_host);
        p.auth = auth_with_key(s.str("PublicKeyFile"));
        p.agent_forwarding = s.num("AgentFwd") == Some(1);
        if let Some(ping) = s.num("PingIntervalSecs").filter(|n| *n > 0) {
            p.keepalive_secs = ping;
        }
        p.source = ProfileSource::Putty {
            session: s.name.clone(),
        };
        if let Some(fw) = s.str("PortForwardings") {
            for item in fw.split(',').filter(|i| !i.is_empty()) {
                match parse_putty_forward(item) {
                    Some(spec) => p.forwards.push(spec),
                    None => out
                        .warnings
                        .push(format!("{}: could not parse forward `{item}`", s.name)),
                }
            }
        }
        if let Some(term) = s.str("TerminalType") {
            p.term = term;
        }
        match (s.num("ProxyMethod").unwrap_or(0), s.str("ProxyHost")) {
            (0, _) => {}
            // 6 = "SSH to proxy and use port forwarding" (PuTTY 0.77+): a jump host.
            (6, Some(host)) => p.jump.push(hostwick_core::Endpoint {
                host,
                port: s.num("ProxyPort").and_then(|n| u16::try_from(n).ok()).filter(|n| *n != 0).unwrap_or(22),
                user: s.str("ProxyUsername"),
            }),
            (method, host) => out.warnings.push(format!(
                "{}: imported, but its PuTTY {} proxy ({}) is not supported — connect directly or set a jump host",
                s.name,
                match method {
                    1 => "SOCKS4",
                    2 => "SOCKS5",
                    3 => "HTTP",
                    4 => "Telnet",
                    5 => "local-command",
                    _ => "unknown",
                },
                host.unwrap_or_default()
            )),
        }
        out.profiles.push(p);
    }
    out
}

/// PuTTY forward syntax: `[4|6]L[bind:]port=host:port`, `R...`, `D[bind:]port`.
fn parse_putty_forward(item: &str) -> Option<ForwardSpec> {
    let item = item.trim().trim_start_matches(['4', '6']);
    let kind = item.chars().next()?;
    let rest = &item[1..];
    let split_listen = |l: &str| -> Option<(String, u16)> {
        match l.rsplit_once(':') {
            Some((b, p)) => Some((b.to_owned(), p.parse().ok()?)),
            None => Some(("127.0.0.1".to_owned(), l.parse().ok()?)),
        }
    };
    match kind {
        'D' => {
            let (bind_addr, bind_port) = split_listen(rest)?;
            Some(ForwardSpec::Dynamic {
                bind_addr,
                bind_port,
            })
        }
        'L' | 'R' => {
            let (listen, dest) = rest.split_once('=')?;
            let (bind_addr, bind_port) = split_listen(listen)?;
            let (host, port) = dest.rsplit_once(':')?;
            let (host, host_port) = (host.to_owned(), port.parse().ok()?);
            Some(if kind == 'L' {
                ForwardSpec::Local {
                    bind_addr,
                    bind_port,
                    host,
                    host_port,
                }
            } else {
                ForwardSpec::Remote {
                    bind_addr,
                    bind_port,
                    host,
                    host_port,
                }
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hostwick_core::AuthMethod;
    use pretty_assertions::assert_eq;

    const REG: &str = r#"Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions]

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\Default%20Settings]
"HostName"=""

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\prod%2Fweb%2001]
"HostName"="deploy@10.1.2.3"
"PortNumber"=dword:00000016
"UserName"=""
"Protocol"="ssh"
"PublicKeyFile"="C:\\Users\\me\\keys\\prod.ppk"
"PortForwardings"="L8080=localhost:80,4R127.0.0.1:2222=localhost:22,D1080,Lgarbage"
"PingIntervalSecs"=dword:0000003c
"ProxyMethod"=dword:00000000

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\router]
"HostName"="192.168.1.1"
"PortNumber"=dword:00000017
"Protocol"="telnet"

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\db]
"HostName"="db.internal"
"PortNumber"=dword:00000896
"UserName"="postgres"
"Protocol"="ssh"
"ProxyMethod"=dword:00000002
"ProxyHost"="proxy.corp"

[HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\behind-bastion]
"HostName"="10.9.9.9"
"Protocol"="ssh"
"TerminalType"="xterm"
"ProxyMethod"=dword:00000006
"ProxyHost"="bastion.corp"
"ProxyPort"=dword:00000937
"ProxyUsername"="jumper"
"#;

    #[test]
    fn parses_reg_export() {
        let r = parse_putty_reg(REG).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.profiles.len(), 3);
        let jb = &r.profiles[2];
        assert_eq!(jb.term, "xterm");
        assert_eq!(
            jb.jump,
            vec![hostwick_core::Endpoint {
                host: "bastion.corp".into(),
                port: 2359,
                user: Some("jumper".into())
            }]
        );
        let web = &r.profiles[0];
        assert_eq!((web.folder.as_str(), web.name.as_str()), ("prod", "web 01"));
        assert_eq!(
            (web.host.as_str(), web.port, web.user.as_deref()),
            ("10.1.2.3", 22, Some("deploy"))
        );
        assert_eq!(web.keepalive_secs, 60);
        assert_eq!(
            web.auth[1],
            AuthMethod::KeyFile {
                path: r"C:\Users\me\keys\prod.ppk".into(),
                passphrase_credential: None
            }
        );
        assert_eq!(web.forwards.len(), 3);
        assert_eq!(
            web.forwards[0].to_string(),
            "L 127.0.0.1:8080 -> localhost:80"
        );
        assert_eq!(
            web.forwards[1].to_string(),
            "R 127.0.0.1:2222 -> localhost:22"
        );
        assert_eq!(web.forwards[2].to_string(), "D 127.0.0.1:1080 (SOCKS5)");
        let db = &r.profiles[1];
        assert_eq!((db.port, db.user.as_deref()), (2198, Some("postgres")));
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
        assert!(r.warnings.iter().any(|w| w.contains("telnet")));
        assert!(r.warnings.iter().any(|w| w.contains("garbage")));
        assert!(r.warnings.iter().any(|w| w.contains("proxy.corp")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse_putty_reg("[HKEY_CURRENT_USER\\Software\\Other]").is_err());
    }
}
