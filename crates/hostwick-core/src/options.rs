//! Per-host options beyond the SSH basics: transport (SSH, telnet, serial),
//! outbound proxy, X11, file-access mode, login scripts and snippets.

use serde::{Deserialize, Serialize};

/// How the terminal reaches the host. For `Serial`, `HostProfile::host` is the
/// device (`COM3`, `/dev/ttyUSB0`) and `port` is unused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Ssh,
    Telnet,
    Serial(SerialSettings),
}

impl Protocol {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Telnet => "Telnet",
            Self::Serial(_) => "Serial",
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            Self::Telnet => 23,
            _ => 22,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowControl {
    None,
    /// XON/XOFF.
    Software,
    /// RTS/CTS.
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SerialSettings {
    pub baud: u32,
    /// 5–8.
    pub data_bits: u8,
    pub parity: Parity,
    /// 1 or 2.
    pub stop_bits: u8,
    pub flow: FlowControl,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            baud: 9600,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow: FlowControl::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyKind {
    Socks5,
    Socks4,
    /// HTTP `CONNECT`.
    Http,
}

/// Proxy used to reach the first hop (the outermost jump host, or the host
/// itself when there are no jump hosts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxySpec {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    /// Vault credential id holding the proxy password.
    #[serde(default)]
    pub password_credential: Option<String>,
}

impl ProxyKind {
    /// Port used when a proxy URL has none.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Socks5 | Self::Socks4 => 1080,
            Self::Http => 8080,
        }
    }

    fn scheme(self) -> &'static str {
        match self {
            Self::Socks5 => "socks5",
            Self::Socks4 => "socks4",
            Self::Http => "http",
        }
    }
}

impl ProxySpec {
    /// Parses `scheme://[user@]host[:port]` with scheme `socks5` (or
    /// `socks5h`), `socks4` (or `socks4a`) or `http`. IPv6 hosts go in
    /// brackets. Passwords are not accepted in the URL (they would end up in
    /// shell history); they come from the vault or a prompt.
    pub fn parse_url(url: &str) -> Result<Self, String> {
        let url = url.trim();
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| format!("expected scheme://host:port, got `{url}`"))?;
        let kind = match scheme.to_ascii_lowercase().as_str() {
            "socks5" | "socks5h" | "socks" => ProxyKind::Socks5,
            "socks4" | "socks4a" => ProxyKind::Socks4,
            "http" => ProxyKind::Http,
            other => {
                return Err(format!(
                    "unsupported proxy type `{other}` (use socks5, socks4 or http)"
                ));
            }
        };
        let rest = rest.trim_end_matches('/');
        let (user, hostport) = match rest.rsplit_once('@') {
            Some((u, h)) => {
                if u.contains(':') {
                    return Err(
                        "put the proxy password in the vault (or type it when asked), not in the URL"
                            .into(),
                    );
                }
                (Some(u.to_owned()).filter(|u| !u.is_empty()), h)
            }
            None => (None, rest),
        };
        let (host, port) = if let Some(after) = hostport.strip_prefix('[') {
            let (h, tail) = after
                .split_once(']')
                .ok_or_else(|| format!("unclosed `[` in `{url}`"))?;
            let port = match tail.strip_prefix(':') {
                Some(p) => Some(p),
                None if tail.is_empty() => None,
                None => return Err(format!("bad proxy address `{hostport}`")),
            };
            (h, port)
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (hostport, None),
            }
        };
        if host.is_empty() || host.contains('/') {
            return Err(format!("missing or bad proxy host in `{url}`"));
        }
        let port = match port {
            Some(p) => p
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| format!("bad proxy port `{p}`"))?,
            None => kind.default_port(),
        };
        Ok(Self {
            kind,
            host: host.to_owned(),
            port,
            user,
            password_credential: None,
        })
    }

    /// The inverse of [`ProxySpec::parse_url`] (without any password).
    pub fn to_url(&self) -> String {
        let user = self
            .user
            .as_deref()
            .map(|u| format!("{u}@"))
            .unwrap_or_default();
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{}://{user}{host}:{}", self.kind.scheme(), self.port)
    }
}

/// Which file protocol the file browser and transfers use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileProtocol {
    /// SFTP, falling back to SCP when the server has no SFTP subsystem.
    #[default]
    Auto,
    Sftp,
    Scp,
}

/// Run the file server as another user via `sudo` (e.g. root).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SudoOptions {
    /// Open the file browser elevated by default.
    pub enabled: bool,
    /// Target user; empty means root.
    pub user: String,
    /// Path of `sftp-server`; empty means auto-detect.
    pub sftp_server: String,
    /// Vault credential id with the sudo password (usually the login
    /// password). None: use the login password if saved, else prompt.
    pub password_credential: Option<String>,
}

/// A saved piece of text to send to a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    pub text: String,
    /// Press Enter after sending.
    #[serde(default = "yes")]
    pub run: bool,
}

fn yes() -> bool {
    true
}

/// Colour terminal text matching a pattern (SecureCRT-style keyword
/// highlighting). `color` is `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightRule {
    pub pattern: String,
    /// Treat `pattern` as a regular expression; otherwise a plain word,
    /// matched case-insensitively.
    #[serde(default)]
    pub regex: bool,
    pub color: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl HighlightRule {
    /// Sensible defaults: errors red, warnings amber, success green.
    pub fn defaults() -> Vec<Self> {
        let r = |p: &str, c: &str| Self {
            pattern: p.into(),
            regex: true,
            color: c.into(),
            enabled: true,
        };
        vec![
            r(
                r"(?i)\b(error|failed|failure|fatal|denied|critical)\b",
                "#ff5f5f",
            ),
            r(r"(?i)\b(warn|warning|deprecated)\b", "#ffaf00"),
            // `\b` cannot follow `)`, so "active (running)" sits outside it.
            r(
                r"(?i)\b(ok|success|succeeded|passed)\b|\bactive \(running\)",
                "#5fd75f",
            ),
        ]
    }
}

/// One step of a login script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptStep {
    /// Type this text and press Enter.
    Send(String),
    /// Wait until the output contains this text (case-sensitive), up to
    /// `timeout_ms`.
    WaitFor {
        text: String,
        timeout_ms: u64,
    },
    Sleep(u64),
}

/// Parse a login script. One command per line; directives:
/// `@wait <text>` (10 s timeout), `@wait <ms> <text>`, `@sleep <ms>`.
/// Blank lines and lines starting with `#` are ignored; `\#` sends a `#`.
pub fn parse_login_script(text: &str) -> Result<Vec<ScriptStep>, String> {
    let mut steps = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let err = |m: &str| format!("line {}: {m}", i + 1);
        if let Some(rest) = trimmed.strip_prefix("@sleep") {
            let ms = rest
                .trim()
                .parse::<u64>()
                .map_err(|_| err("@sleep needs milliseconds"))?;
            steps.push(ScriptStep::Sleep(ms.min(600_000)));
        } else if let Some(rest) = trimmed.strip_prefix("@wait") {
            let rest = rest.trim();
            let (timeout_ms, text) = match rest.split_once(' ') {
                Some((n, t)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
                    (n.parse::<u64>().map_err(|_| err("bad timeout"))?, t.trim())
                }
                _ => (10_000, rest),
            };
            if text.is_empty() {
                return Err(err("@wait needs text to wait for"));
            }
            steps.push(ScriptStep::WaitFor {
                text: text.to_owned(),
                timeout_ms: timeout_ms.min(600_000),
            });
        } else if trimmed.starts_with('@') {
            return Err(err("unknown directive (use @wait or @sleep)"));
        } else {
            let text = line
                .strip_prefix("\\#")
                .map(|r| format!("#{r}"))
                .unwrap_or_else(|| line.to_owned());
            steps.push(ScriptStep::Send(text));
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_script_parses() {
        let s = "# comment\nsudo -i\n@wait 5000 password for\n@sleep 200\n\\#literal\n@wait $ \n";
        let steps = parse_login_script(s).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            steps,
            vec![
                ScriptStep::Send("sudo -i".into()),
                ScriptStep::WaitFor {
                    text: "password for".into(),
                    timeout_ms: 5000
                },
                ScriptStep::Sleep(200),
                ScriptStep::Send("#literal".into()),
                ScriptStep::WaitFor {
                    text: "$".into(),
                    timeout_ms: 10_000
                },
            ]
        );
        assert!(parse_login_script("@sleep x").is_err());
        assert!(parse_login_script("@bogus").is_err());
        assert!(parse_login_script("@wait").is_err());
    }

    #[test]
    fn options_round_trip_and_defaults() {
        let p = Protocol::Serial(SerialSettings::default());
        let json = serde_json::to_string(&p).unwrap_or_default();
        assert_eq!(serde_json::from_str::<Protocol>(&json).ok(), Some(p));
        assert_eq!(
            serde_json::from_str::<Protocol>(r#"{"kind":"telnet"}"#).ok(),
            Some(Protocol::Telnet)
        );
        assert_eq!(Protocol::Telnet.default_port(), 23);
        assert_eq!(HighlightRule::defaults().len(), 3);
    }

    #[test]
    fn proxy_urls_parse() {
        let p =
            ProxySpec::parse_url("socks5://alice@10.0.0.1:1081").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.kind, ProxyKind::Socks5);
        assert_eq!(
            (p.host.as_str(), p.port, p.user.as_deref()),
            ("10.0.0.1", 1081, Some("alice"))
        );
        assert_eq!(p.to_url(), "socks5://alice@10.0.0.1:1081");

        let p = ProxySpec::parse_url("http://proxy.corp").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((p.kind, p.port, p.user), (ProxyKind::Http, 8080, None));

        let p = ProxySpec::parse_url("socks4a://[::1]:9050/").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            (p.kind, p.host.as_str(), p.port),
            (ProxyKind::Socks4, "::1", 9050)
        );
        assert_eq!(p.to_url(), "socks4://[::1]:9050");

        assert!(ProxySpec::parse_url("socks5h://h").is_ok_and(|p| p.port == 1080));
        assert!(ProxySpec::parse_url("ftp://h:1").is_err());
        assert!(ProxySpec::parse_url("h:1080").is_err());
        assert!(ProxySpec::parse_url("http://u:pw@h:1").is_err_and(|e| e.contains("vault")));
        assert!(ProxySpec::parse_url("http://h:0").is_err());
        assert!(ProxySpec::parse_url("http://:80").is_err());
    }
}
