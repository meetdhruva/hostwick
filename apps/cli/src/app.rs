use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hostwick_core::{
    AuthMethod, Endpoint, ForwardSpec, HostProfile, ProfileStore, ProxySpec, SecretString, Target,
};
use hostwick_ssh::{ConnectContext, Connection, Credentials, KnownHosts};
use hostwick_store::{AppPaths, SqliteStore};
use hostwick_vault::Vault;

use crate::ConnOpts;
use crate::prompt::{Policy, TerminalInteraction};

pub struct App {
    pub paths: AppPaths,
    pub store: SqliteStore,
    pub policy: Policy,
}

/// Secrets copied out of the unlocked vault for one command.
struct VaultCredentials(HashMap<String, SecretString>);

impl Credentials for VaultCredentials {
    fn get(&self, id: &str) -> Option<SecretString> {
        self.0.get(id).cloned()
    }
}

impl App {
    pub fn open(policy: Policy) -> Result<Self> {
        let paths = AppPaths::detect();
        paths
            .ensure()
            .with_context(|| format!("creating {}", paths.data_dir.display()))?;
        let store = SqliteStore::open(&paths.db).context("opening profile database")?;
        Ok(Self {
            paths,
            store,
            policy,
        })
    }

    /// A saved profile by name, or an ad-hoc one built from `user@host:port`.
    /// Command-line options override profile values.
    pub fn resolve(&self, target: &str, o: &ConnOpts) -> Result<HostProfile> {
        let t = Target::parse(target)?;
        let mut p = match self.store.find_by_name(&t.host)? {
            Some(p) => p,
            None => {
                let mut p = HostProfile::new(t.to_string(), t.host.clone());
                p.auth = default_auth();
                p
            }
        };
        if let Some(u) = o.user.clone().or(t.user) {
            p.user = Some(u);
        }
        if let Some(port) = o.port.or(t.port) {
            p.port = port;
        }
        if !o.identity.is_empty() {
            let keys = o.identity.iter().map(|k| AuthMethod::KeyFile {
                path: k.clone(),
                passphrase_credential: None,
            });
            // Explicit keys go first, like `ssh -i`.
            p.auth = keys
                .chain(
                    p.auth
                        .into_iter()
                        .filter(|a| !matches!(a, AuthMethod::KeyFile { .. })),
                )
                .collect();
        }
        if let Some(j) = &o.jump {
            p.jump = parse_jump(j)?;
        }
        p.allow_legacy_algorithms |= o.legacy;
        if let Some(url) = &o.proxy {
            p.proxy = apply_proxy(p.proxy.take(), url)?;
        }
        if o.gssapi {
            enable_gssapi(&mut p.auth);
        }
        Ok(p)
    }

    pub fn context(&self, profile: &HostProfile) -> Result<ConnectContext> {
        let credentials = self.credentials_for(profile)?;
        let mut ctx = ConnectContext::new(
            Arc::new(TerminalInteraction::new(self.policy)),
            Arc::new(credentials),
        );
        ctx.known_hosts = Some(KnownHosts::new(&self.paths.known_hosts));
        ctx.connect_timeout = Duration::from_secs(20);
        Ok(ctx)
    }

    pub async fn connect(&self, profile: &HostProfile) -> Result<Connection> {
        let ctx = self.context(profile)?;
        Connection::connect(profile, &ctx)
            .await
            .with_context(|| format!("connecting to {}", profile.path()))
    }

    /// Unlock the vault only if this profile references stored secrets.
    fn credentials_for(&self, p: &HostProfile) -> Result<VaultCredentials> {
        let ids: Vec<&str> = p
            .auth
            .iter()
            .filter_map(|a| match a {
                AuthMethod::Password { credential } => credential.as_deref(),
                AuthMethod::KeyFile {
                    passphrase_credential,
                    ..
                } => passphrase_credential.as_deref(),
                _ => None,
            })
            .chain(
                p.proxy
                    .as_ref()
                    .and_then(|x| x.password_credential.as_deref()),
            )
            .chain(
                p.sudo
                    .password_credential
                    .as_deref()
                    .filter(|_| p.sudo.enabled),
            )
            .collect();
        if ids.is_empty() || !self.paths.vault.exists() {
            return Ok(VaultCredentials(HashMap::new()));
        }
        let vault = self.unlock_vault()?;
        Ok(VaultCredentials(
            ids.iter()
                .filter_map(|id| Some(((*id).to_owned(), vault.get(id)?)))
                .collect(),
        ))
    }

    pub fn unlock_vault(&self) -> Result<Vault> {
        if self.policy.batch {
            bail!("the vault is locked and --batch forbids prompting for the master password");
        }
        let pw = crate::tty::read_secret("Vault master password: ")?;
        Ok(Vault::unlock(&self.paths.vault, &pw)?)
    }
}

/// OpenSSH-like defaults for an unsaved host: agent, the usual key files,
/// then password and keyboard-interactive.
pub fn default_auth() -> Vec<AuthMethod> {
    let mut v = vec![AuthMethod::Agent];
    for k in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        if let Some(home) = dirs_home() {
            let path = home.join(".ssh").join(k);
            if path.exists() {
                v.push(AuthMethod::KeyFile {
                    path,
                    passphrase_credential: None,
                });
            }
        }
    }
    v.push(AuthMethod::Password { credential: None });
    v.push(AuthMethod::KeyboardInteractive);
    v
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(Into::into)
}

/// `--proxy URL` over a profile's proxy. Keeps the saved vault entry for
/// the password when the URL names the same proxy and user.
fn apply_proxy(saved: Option<ProxySpec>, url: &str) -> Result<Option<ProxySpec>> {
    if url.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let mut spec = ProxySpec::parse_url(url).map_err(anyhow::Error::msg)?;
    if let Some(old) = saved
        && old.host == spec.host
        && old.port == spec.port
        && old.user == spec.user
    {
        spec.password_credential = old.password_credential;
    }
    Ok(Some(spec))
}

/// `-K`: Kerberos first, with ticket forwarding (OpenSSH tries
/// gssapi-with-mic first too).
fn enable_gssapi(auth: &mut Vec<AuthMethod>) {
    match auth
        .iter_mut()
        .find(|a| matches!(a, AuthMethod::Gssapi { .. }))
    {
        Some(AuthMethod::Gssapi { delegate }) => *delegate = true,
        _ => auth.insert(0, AuthMethod::Gssapi { delegate: true }),
    }
}

pub fn parse_jump(s: &str) -> Result<Vec<Endpoint>> {
    s.split(',')
        .filter(|h| !h.trim().is_empty())
        .map(|h| {
            let t = Target::parse(h)?;
            Ok(Endpoint {
                port: t.port_or_default(),
                host: t.host,
                user: t.user,
            })
        })
        .collect()
}

pub fn parse_forwards(
    local: &[String],
    remote: &[String],
    dynamic: &[String],
) -> Result<Vec<ForwardSpec>> {
    let mut out = Vec::new();
    for (flag, list) in [('L', local), ('R', remote), ('D', dynamic)] {
        for s in list {
            out.push(ForwardSpec::parse(flag, s).map_err(anyhow::Error::msg)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jump_chains() -> Result<()> {
        let j = parse_jump("ops@bastion:2222,inner")?;
        assert_eq!(j.len(), 2);
        assert_eq!(
            (j[0].host.as_str(), j[0].port, j[0].user.as_deref()),
            ("bastion", 2222, Some("ops"))
        );
        assert_eq!((j[1].host.as_str(), j[1].port), ("inner", 22));
        Ok(())
    }

    #[test]
    fn proxy_flag_and_gssapi() -> Result<()> {
        let saved = ProxySpec {
            password_credential: Some("vault-id".into()),
            ..ProxySpec::parse_url("socks5://me@p:1080").map_err(anyhow::Error::msg)?
        };
        let same = apply_proxy(Some(saved.clone()), "socks5://me@p:1080")?;
        assert_eq!(
            same.and_then(|s| s.password_credential).as_deref(),
            Some("vault-id")
        );
        let other = apply_proxy(Some(saved.clone()), "http://p:3128")?;
        assert!(other.is_some_and(|s| s.password_credential.is_none()));
        assert!(apply_proxy(Some(saved), "none")?.is_none());
        assert!(apply_proxy(None, "gopher://x").is_err());

        let mut auth = vec![AuthMethod::Agent];
        enable_gssapi(&mut auth);
        assert_eq!(auth[0], AuthMethod::Gssapi { delegate: true });
        let mut auth = vec![AuthMethod::Agent, AuthMethod::Gssapi { delegate: false }];
        enable_gssapi(&mut auth);
        assert_eq!(auth.len(), 2);
        assert_eq!(auth[1], AuthMethod::Gssapi { delegate: true });
        Ok(())
    }
}
