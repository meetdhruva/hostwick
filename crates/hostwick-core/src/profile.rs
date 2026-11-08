use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::DEFAULT_PORT;

/// Stable identity of a saved host profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(pub Uuid);

impl ProfileId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProfileId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// A host and port with an optional login, used for jump hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

/// How to authenticate. Tried in order; the first success wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Keys held by Pageant / the OpenSSH agent / `SSH_AUTH_SOCK`.
    Agent,
    /// An OpenSSH-format private key on disk. The passphrase, if any, is
    /// looked up in the vault under `passphrase_credential`, then prompted.
    KeyFile {
        path: PathBuf,
        #[serde(default)]
        passphrase_credential: Option<String>,
    },
    /// Password from the vault (`credential`), falling back to a prompt.
    Password {
        #[serde(default)]
        credential: Option<String>,
    },
    /// Server-driven prompts (OTP, MFA, PAM).
    KeyboardInteractive,
    /// Kerberos single sign-on (`gssapi-with-mic`): the current Windows
    /// domain login, or a `kinit` ticket on macOS/Linux.
    Gssapi {
        /// Forward the Kerberos ticket to the server (OpenSSH
        /// `GSSAPIDelegateCredentials`).
        #[serde(default)]
        delegate: bool,
    },
}

/// A port forward attached to a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForwardSpec {
    /// `-L bind:port:host:hostport` — listen locally, connect from the server.
    Local {
        bind_addr: String,
        bind_port: u16,
        host: String,
        host_port: u16,
    },
    /// `-R bind:port:host:hostport` — server listens, we connect locally.
    Remote {
        bind_addr: String,
        bind_port: u16,
        host: String,
        host_port: u16,
    },
    /// `-D bind:port` — local SOCKS5 proxy through the server.
    Dynamic { bind_addr: String, bind_port: u16 },
}

impl ForwardSpec {
    /// Parses OpenSSH-style specs: `-L [bind:]port:host:hostport`,
    /// `-R [bind:]port:host:hostport`, `-D [bind:]port`.
    pub fn parse(flag: char, spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split(':').collect();
        let port = |s: &str| {
            s.parse::<u16>()
                .map_err(|_| format!("bad port `{s}` in `{spec}`"))
        };
        match flag {
            'L' | 'R' => {
                let (bind_addr, bind_port, host, host_port) = match parts.as_slice() {
                    [p, h, hp] => ("127.0.0.1".to_owned(), port(p)?, (*h).to_owned(), port(hp)?),
                    [b, p, h, hp] => ((*b).to_owned(), port(p)?, (*h).to_owned(), port(hp)?),
                    _ => return Err(format!("expected [bind:]port:host:hostport, got `{spec}`")),
                };
                Ok(if flag == 'L' {
                    Self::Local {
                        bind_addr,
                        bind_port,
                        host,
                        host_port,
                    }
                } else {
                    Self::Remote {
                        bind_addr,
                        bind_port,
                        host,
                        host_port,
                    }
                })
            }
            'D' => match parts.as_slice() {
                [p] => Ok(Self::Dynamic {
                    bind_addr: "127.0.0.1".to_owned(),
                    bind_port: port(p)?,
                }),
                [b, p] => Ok(Self::Dynamic {
                    bind_addr: (*b).to_owned(),
                    bind_port: port(p)?,
                }),
                _ => Err(format!("expected [bind:]port, got `{spec}`")),
            },
            other => Err(format!("unknown forward flag -{other}")),
        }
    }
}

impl fmt::Display for ForwardSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local {
                bind_addr,
                bind_port,
                host,
                host_port,
            } => {
                write!(f, "L {bind_addr}:{bind_port} -> {host}:{host_port}")
            }
            Self::Remote {
                bind_addr,
                bind_port,
                host,
                host_port,
            } => {
                write!(f, "R {bind_addr}:{bind_port} -> {host}:{host_port}")
            }
            Self::Dynamic {
                bind_addr,
                bind_port,
            } => write!(f, "D {bind_addr}:{bind_port} (SOCKS5)"),
        }
    }
}

/// Where a profile came from, so re-imports can update instead of duplicate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileSource {
    #[default]
    Manual,
    SshConfig {
        alias: String,
    },
    Putty {
        session: String,
    },
    WinScp {
        session: String,
    },
    SolarPutty {
        session: String,
    },
}

/// A saved connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProfile {
    pub id: ProfileId,
    /// Display name, unique within its folder.
    pub name: String,
    /// Slash-separated folder path in the session tree, e.g. `prod/db`.
    #[serde(default)]
    pub folder: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default = "default_auth")]
    pub auth: Vec<AuthMethod>,
    /// Jump hosts, outermost first (OpenSSH `ProxyJump a,b`).
    #[serde(default)]
    pub jump: Vec<Endpoint>,
    #[serde(default)]
    pub forwards: Vec<ForwardSpec>,
    /// Keepalive interval in seconds; 0 disables.
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u32,
    /// Opt in to `ssh-rsa` (SHA-1) and other legacy algorithms for old gear.
    #[serde(default)]
    pub allow_legacy_algorithms: bool,
    #[serde(default)]
    pub agent_forwarding: bool,
    #[serde(default = "default_term")]
    pub term: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub source: ProfileSource,
    /// Log terminal output of sessions to this host.
    #[serde(default)]
    pub session_log: Option<crate::SessionLog>,
    /// SSH (default), telnet or a serial port.
    #[serde(default)]
    pub protocol: crate::Protocol,
    /// SOCKS/HTTP proxy for reaching the first hop.
    #[serde(default)]
    pub proxy: Option<crate::ProxySpec>,
    /// Forward X11 so remote GUI programs show on this computer.
    #[serde(default)]
    pub x11_forwarding: bool,
    /// Local X display for X11 forwarding; None uses `$DISPLAY`, or
    /// `localhost:0` on Windows (VcXsrv/Xming).
    #[serde(default)]
    pub x11_display: Option<String>,
    #[serde(default)]
    pub file_protocol: crate::FileProtocol,
    #[serde(default)]
    pub sudo: crate::SudoOptions,
    /// Commands sent after the shell starts; see `parse_login_script`.
    #[serde(default)]
    pub login_script: String,
    /// Make the shell report its current folder (OSC 7), so the file
    /// browser can follow the terminal.
    #[serde(default)]
    pub shell_integration: bool,
    #[serde(default)]
    pub snippets: Vec<crate::Snippet>,
    /// Remote folders starred in the file browser (absolute paths); they
    /// travel with the host through settings sync.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<String>,
    /// Last change, Unix milliseconds; set by the store, used to merge
    /// settings sync.
    #[serde(default)]
    pub updated_ms: u64,
}

fn default_auth() -> Vec<AuthMethod> {
    vec![
        AuthMethod::Agent,
        AuthMethod::Password { credential: None },
        AuthMethod::KeyboardInteractive,
    ]
}

fn default_keepalive() -> u32 {
    30
}

fn default_term() -> String {
    "xterm-256color".to_owned()
}

impl HostProfile {
    pub fn new(name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            id: ProfileId::new(),
            name: name.into(),
            folder: String::new(),
            host: host.into(),
            port: DEFAULT_PORT,
            user: None,
            auth: default_auth(),
            jump: Vec::new(),
            forwards: Vec::new(),
            keepalive_secs: default_keepalive(),
            allow_legacy_algorithms: false,
            agent_forwarding: false,
            term: default_term(),
            tags: Vec::new(),
            notes: String::new(),
            source: ProfileSource::Manual,
            session_log: None,
            protocol: crate::Protocol::Ssh,
            proxy: None,
            x11_forwarding: false,
            x11_display: None,
            file_protocol: crate::FileProtocol::Auto,
            sudo: crate::SudoOptions::default(),
            login_script: String::new(),
            shell_integration: false,
            snippets: Vec::new(),
            bookmarks: Vec::new(),
            updated_ms: 0,
        }
    }

    /// Login name, falling back to the local OS user like OpenSSH does.
    pub fn user_or_local(&self) -> String {
        self.user.clone().unwrap_or_else(local_username)
    }

    /// `folder/name`, used for display and search.
    pub fn path(&self) -> String {
        if self.folder.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.folder.trim_end_matches('/'), self.name)
        }
    }
}

pub fn local_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_owned())
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("profile not found: {0}")]
    NotFound(String),
    #[error("storage backend: {0}")]
    Backend(String),
    #[error("stored data is corrupt: {0}")]
    Corrupt(String),
}

/// Repository port for saved profiles. `hostwick-store` provides the SQLite
/// adapter; tests can provide an in-memory one.
pub trait ProfileStore: Send + Sync {
    fn list(&self) -> Result<Vec<HostProfile>, StoreError>;
    fn get(&self, id: ProfileId) -> Result<HostProfile, StoreError>;
    fn find_by_name(&self, name: &str) -> Result<Option<HostProfile>, StoreError>;
    fn upsert(&self, profile: &HostProfile) -> Result<(), StoreError>;
    fn delete(&self, id: ProfileId) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_are_optional_and_round_trip() {
        let mut p = HostProfile::new("web", "10.0.0.1");
        // Profiles saved before bookmarks existed still load.
        let old = serde_json::to_string(&p).unwrap_or_default();
        assert!(
            !old.contains("bookmarks"),
            "empty bookmarks are not written"
        );
        let back: HostProfile = serde_json::from_str(&old).unwrap_or_else(|e| panic!("{e}"));
        assert!(back.bookmarks.is_empty());
        p.bookmarks = vec!["/var/www".into(), "/etc/nginx".into()];
        let json = serde_json::to_string(&p).unwrap_or_default();
        let back: HostProfile = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.bookmarks, p.bookmarks);
    }

    #[test]
    fn forward_specs_parse_like_openssh() {
        assert_eq!(
            ForwardSpec::parse('L', "8080:localhost:80"),
            Ok(ForwardSpec::Local {
                bind_addr: "127.0.0.1".into(),
                bind_port: 8080,
                host: "localhost".into(),
                host_port: 80
            })
        );
        assert_eq!(
            ForwardSpec::parse('R', "0.0.0.0:9000:10.0.0.5:22"),
            Ok(ForwardSpec::Remote {
                bind_addr: "0.0.0.0".into(),
                bind_port: 9000,
                host: "10.0.0.5".into(),
                host_port: 22
            })
        );
        assert_eq!(
            ForwardSpec::parse('D', "1080"),
            Ok(ForwardSpec::Dynamic {
                bind_addr: "127.0.0.1".into(),
                bind_port: 1080
            })
        );
        assert!(ForwardSpec::parse('L', "nope").is_err());
        assert!(ForwardSpec::parse('X', "1").is_err());
    }

    #[test]
    fn profile_json_is_forward_compatible() {
        // Older/minimal JSON must still load with sensible defaults.
        let p: HostProfile = serde_json::from_str(
            r#"{"id":"6f1c2c1e-6f1c-4c1e-8f1c-6f1c2c1e6f1c","name":"web","host":"10.0.0.2"}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.port, 22);
        assert_eq!(p.keepalive_secs, 30);
        assert_eq!(p.auth.first(), Some(&AuthMethod::Agent));
        assert_eq!(p.path(), "web");
    }
}
