use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hostwick_core::{AuthMethod, Backoff, ForwardSpec, HostProfile};
use russh::client::{self, Handle, Msg};
use russh::keys::Algorithm;
use russh::{Channel, ChannelStream, Disconnect, Preferred, kex};

use crate::SshError;
use crate::auth::{AuthCtx, authenticate};
use crate::exec::ExecOutput;
use crate::forward::{self, ForwardHandle};
use crate::handler::{ClientHandler, RemoteForwardMap, VerifyOutcome};
use crate::interaction::{Credentials, Interaction};
use crate::known_hosts::KnownHosts;
use crate::proxy::{self, ProxyCredentials};
use crate::shell::{PtyOptions, SessionForwarding, ShellSession};
use crate::x11::X11Forwarding;

/// Shared services every connection needs.
#[derive(Clone)]
pub struct ConnectContext {
    pub interaction: Arc<dyn Interaction>,
    pub credentials: Arc<dyn Credentials>,
    /// `None` disables persistence (every key is "unknown" and must be
    /// accepted interactively). Never disables verification.
    pub known_hosts: Option<KnownHosts>,
    /// Covers TCP connect + key exchange, not the (interactive) auth phase.
    pub connect_timeout: Duration,
}

impl ConnectContext {
    pub fn new(interaction: Arc<dyn Interaction>, credentials: Arc<dyn Credentials>) -> Self {
        Self {
            interaction,
            credentials,
            known_hosts: KnownHosts::openssh_default(),
            connect_timeout: Duration::from_secs(15),
        }
    }
}

/// An authenticated SSH connection. Cheap to clone; all clones share the
/// same TCP link and multiplex channels over it.
#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

struct Inner {
    handle: Handle<ClientHandler>,
    label: String,
    forwarding: SessionForwarding,
    pub(crate) remote_forwards: RemoteForwardMap,
    /// Keeps the jump-host connection alive for as long as we are.
    _via: Option<Connection>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("label", &self.inner.label)
            .finish()
    }
}

impl Connection {
    /// Connect through any jump hosts, verify host keys, and authenticate.
    pub async fn connect(profile: &HostProfile, ctx: &ConnectContext) -> Result<Self, SshError> {
        let user = profile.user_or_local();
        let mut via: Option<Connection> = None;
        let hop_methods = jump_auth(&profile.auth);
        for hop in &profile.jump {
            let hop_user = hop.user.clone().unwrap_or_else(|| user.clone());
            let conn = Self::connect_hop(
                &hop.host,
                hop.port,
                &hop_user,
                profile,
                &hop_methods,
                ctx,
                via.take(),
                false,
            )
            .await?;
            via = Some(conn);
        }
        Self::connect_hop(
            &profile.host,
            profile.port,
            &user,
            profile,
            &profile.auth,
            ctx,
            via,
            true,
        )
        .await
    }

    /// [`Connection::connect`] with exponential backoff on transient errors
    /// (network down, timeouts). Auth and host-key failures fail fast.
    pub async fn connect_with_retry(
        profile: &HostProfile,
        ctx: &ConnectContext,
        mut backoff: Backoff,
    ) -> Result<Self, SshError> {
        loop {
            match Self::connect(profile, ctx).await {
                Ok(c) => return Ok(c),
                Err(e) if e.is_transient() => match backoff.next_delay() {
                    Some(d) => {
                        tracing::info!(error = %e, delay = ?d, attempt = backoff.attempt(), "retrying connect");
                        tokio::time::sleep(d).await;
                    }
                    None => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn connect_hop(
        host: &str,
        port: u16,
        user: &str,
        profile: &HostProfile,
        methods: &[AuthMethod],
        ctx: &ConnectContext,
        via: Option<Connection>,
        // The destination (not a jump host): only it gets X11/agent forwarding.
        is_target: bool,
    ) -> Result<Self, SshError> {
        let label = format!("{user}@{host}:{port}");
        let config = Arc::new(client_config(profile));
        let outcome = Arc::new(Mutex::new(VerifyOutcome::default()));
        let remote_forwards = RemoteForwardMap::default();
        let forwarding = SessionForwarding {
            agent: is_target && profile.agent_forwarding,
            x11: (is_target && profile.x11_forwarding).then(|| {
                X11Forwarding::new(profile.x11_display.as_deref(), ctx.interaction.clone())
            }),
        };
        let handler = ClientHandler {
            host: host.to_owned(),
            port,
            known_hosts: ctx.known_hosts.clone(),
            interaction: ctx.interaction.clone(),
            outcome: outcome.clone(),
            remote_forwards: remote_forwards.clone(),
            x11: forwarding.x11.clone(),
            agent_forwarding: forwarding.agent,
            agent_warned: Default::default(),
        };

        // The proxy (first hop only) runs before the timed handshake: it may
        // prompt for the proxy password, and has per-step timeouts of its own.
        let proxied = match (&via, &profile.proxy) {
            (None, Some(spec)) => Some(connect_via_proxy(spec, ctx, host, port).await?),
            _ => None,
        };

        let handshake = async {
            match &via {
                None => {
                    let tcp = match proxied {
                        Some(tcp) => tcp,
                        None => tokio::net::TcpStream::connect((host, port)).await.map_err(
                            |reason| SshError::Connect {
                                host: host.to_owned(),
                                port,
                                reason,
                            },
                        )?,
                    };
                    let _ = tcp.set_nodelay(true);
                    client::connect_stream(config, tcp, handler)
                        .await
                        .map_err(SshError::from)
                }
                Some(jump) => {
                    let ch = jump.open_direct_tcpip(host, port).await?;
                    client::connect_stream(config, ch.into_stream(), handler)
                        .await
                        .map_err(SshError::from)
                }
            }
        };
        let result = tokio::time::timeout(ctx.connect_timeout, handshake)
            .await
            .map_err(|_| SshError::Timeout(ctx.connect_timeout, label.clone()))?;
        let mut handle = match result {
            Ok(h) => h,
            Err(e) => {
                let rejected = outcome.lock().map(|o| o.rejected).unwrap_or(false);
                return Err(if rejected {
                    SshError::HostKeyRejected(label)
                } else {
                    e
                });
            }
        };

        let auth_ctx = AuthCtx {
            user,
            host,
            interaction: ctx.interaction.as_ref(),
            credentials: ctx.credentials.as_ref(),
        };
        authenticate(&mut handle, methods, &auth_ctx).await?;
        tracing::info!(%label, "connected");

        Ok(Self {
            inner: Arc::new(Inner {
                handle,
                label,
                forwarding,
                remote_forwards,
                _via: via,
            }),
        })
    }

    pub fn label(&self) -> &str {
        &self.inner.label
    }

    pub fn is_closed(&self) -> bool {
        self.inner.handle.is_closed()
    }

    pub(crate) fn handle(&self) -> &Handle<ClientHandler> {
        &self.inner.handle
    }

    pub(crate) fn remote_forwards(&self) -> &RemoteForwardMap {
        &self.inner.remote_forwards
    }

    /// Interactive login shell with a PTY.
    pub async fn open_shell(&self, pty: PtyOptions) -> Result<ShellSession, SshError> {
        ShellSession::open(self.handle(), Some(pty), None, &self.inner.forwarding).await
    }

    /// Run a command; stdout/stderr/exit stream back as [`crate::ShellEvent`]s.
    /// Pass a PTY for interactive commands (`ssh -t`). Agent and X11
    /// forwarding apply as for the shell (so `ssh -X host xterm` works).
    pub async fn open_exec(
        &self,
        command: &str,
        pty: Option<PtyOptions>,
    ) -> Result<ShellSession, SshError> {
        ShellSession::open(self.handle(), pty, Some(command), &self.inner.forwarding).await
    }

    /// Run a command to completion and collect its output. Housekeeping
    /// commands never forward the agent or X11.
    pub async fn exec(&self, command: &str) -> Result<ExecOutput, SshError> {
        let session = ShellSession::open(
            self.handle(),
            None,
            Some(command),
            &SessionForwarding::default(),
        )
        .await?;
        ExecOutput::collect(session).await
    }

    /// Round-trip time to the server: one SSH keepalive request, answered
    /// by the server's transport layer (no channel, no process). Used by
    /// the terminal status bar.
    pub async fn ping(&self) -> Result<std::time::Duration, SshError> {
        let start = std::time::Instant::now();
        self.handle().send_ping().await?;
        Ok(start.elapsed())
    }

    /// Raw stream for a subsystem (e.g. `sftp`).
    pub async fn open_subsystem(&self, name: &str) -> Result<ChannelStream<Msg>, SshError> {
        let ch = self.handle().channel_open_session().await?;
        ch.request_subsystem(true, name).await?;
        Ok(ch.into_stream())
    }

    /// `direct-tcpip` channel: the server connects to `host:port` for us.
    pub async fn open_direct_tcpip(&self, host: &str, port: u16) -> Result<Channel<Msg>, SshError> {
        self.handle()
            .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
            .await
            .map_err(|e| SshError::ChannelRefused(format!("{host}:{port}: {e}")))
    }

    /// Start a port forward. It runs until the returned handle is dropped.
    pub async fn start_forward(&self, spec: &ForwardSpec) -> Result<ForwardHandle, SshError> {
        forward::start(self.clone(), spec.clone()).await
    }

    pub async fn disconnect(&self) {
        let _ = self
            .inner
            .handle
            .disconnect(Disconnect::ByApplication, "bye", "en")
            .await;
    }
}

/// The target's auth methods as used for a jump host: the same order, but
/// never the target's saved password (a jump host is another server and
/// must not learn it; its own password is asked for, labelled with its
/// name) and never Kerberos ticket delegation. Agent, key files (with their
/// passphrases, which unlock the local file) and keyboard-interactive stay.
pub(crate) fn jump_auth(methods: &[AuthMethod]) -> Vec<AuthMethod> {
    methods
        .iter()
        .map(|m| match m {
            AuthMethod::Password { .. } => AuthMethod::Password { credential: None },
            AuthMethod::Gssapi { .. } => AuthMethod::Gssapi { delegate: false },
            other => other.clone(),
        })
        .collect()
}

/// A TCP connection to `host:port` through `spec`, with the same
/// credentials (vault or prompt) and timeouts SSH connections use. For other
/// protocols (telnet) that honour a host's proxy setting.
pub async fn connect_via_proxy(
    spec: &hostwick_core::ProxySpec,
    ctx: &ConnectContext,
    host: &str,
    port: u16,
) -> Result<tokio::net::TcpStream, SshError> {
    let (creds, saved) = proxy_credentials(spec, ctx).await;
    tracing::info!(proxy = %spec.to_url(), %host, port, "connecting through proxy");
    let res = proxy::connect(spec, &creds, host, port, ctx.connect_timeout).await;
    if let (Some(user), Some(_)) = (&creds.user, &creds.password) {
        // Only a verdict on the password itself: a refused or unreachable
        // proxy says nothing about it.
        let verdict = match &res {
            Ok(_) => Some(true),
            Err(SshError::Proxy { reason, .. })
                if reason.contains("rejected the") || reason.contains("requires a password") =>
            {
                Some(false)
            }
            Err(_) => None,
        };
        if let Some(accepted) = verdict {
            let proxy_host = format!("proxy {}", spec.host);
            ctx.interaction.secret_outcome(
                crate::SecretUse::Password {
                    user,
                    host: &proxy_host,
                },
                saved,
                accepted,
            );
        }
    }
    res
}

/// The proxy user and password: the vault entry, else a prompt (only when a
/// proxy user is set; SOCKS4 has no password). Also whether the password
/// came from the vault.
async fn proxy_credentials(
    spec: &hostwick_core::ProxySpec,
    ctx: &ConnectContext,
) -> (ProxyCredentials, bool) {
    let Some(user) = spec.user.clone().filter(|u| !u.is_empty()) else {
        return (ProxyCredentials::default(), false);
    };
    let (password, saved) = if spec.kind == hostwick_core::ProxyKind::Socks4 {
        (None, false)
    } else {
        match spec
            .password_credential
            .as_deref()
            .and_then(|c| ctx.credentials.get(c))
        {
            Some(p) => (Some(p), true),
            None => {
                let proxy_host = format!("proxy {}", spec.host);
                (ctx.interaction.password(&user, &proxy_host).await, false)
            }
        }
    };
    (
        ProxyCredentials {
            user: Some(user),
            password,
        },
        saved,
    )
}

/// russh client config from a profile: keepalive, and algorithm policy.
pub(crate) fn client_config(profile: &HostProfile) -> client::Config {
    let mut preferred = Preferred::default();
    if profile.allow_legacy_algorithms {
        let mut kex_list = preferred.kex.to_vec();
        kex_list.extend([kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1]);
        preferred.kex = Cow::Owned(kex_list);
    } else {
        // Secure by default: no SHA-1 `ssh-rsa` host-key signatures.
        preferred.key = Cow::Owned(
            preferred
                .key
                .iter()
                .filter(|a| !matches!(a, Algorithm::Rsa { hash: None }))
                .cloned()
                .collect(),
        );
    }
    client::Config {
        preferred,
        nodelay: true,
        inactivity_timeout: None,
        keepalive_interval: (profile.keepalive_secs > 0)
            .then(|| Duration::from_secs(u64::from(profile.keepalive_secs))),
        keepalive_max: 3,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A jump host never gets the target's saved password or a delegated
    /// Kerberos ticket.
    #[test]
    fn jump_hops_drop_target_secrets() {
        let key = AuthMethod::KeyFile {
            path: "/k".into(),
            passphrase_credential: Some("key:/k".into()),
        };
        let target = vec![
            AuthMethod::Agent,
            key.clone(),
            AuthMethod::Gssapi { delegate: true },
            AuthMethod::Password {
                credential: Some("host:1:password".into()),
            },
            AuthMethod::KeyboardInteractive,
        ];
        assert_eq!(
            jump_auth(&target),
            vec![
                AuthMethod::Agent,
                key,
                AuthMethod::Gssapi { delegate: false },
                AuthMethod::Password { credential: None },
                AuthMethod::KeyboardInteractive,
            ]
        );
    }

    #[test]
    fn legacy_algorithms_are_opt_in() {
        let mut p = HostProfile::new("x", "h");
        let strict = client_config(&p);
        assert!(
            !strict
                .preferred
                .key
                .iter()
                .any(|a| matches!(a, Algorithm::Rsa { hash: None }))
        );
        assert!(!strict.preferred.kex.contains(&kex::DH_G1_SHA1));
        assert_eq!(strict.keepalive_interval, Some(Duration::from_secs(30)));

        p.allow_legacy_algorithms = true;
        p.keepalive_secs = 0;
        let legacy = client_config(&p);
        assert!(legacy.preferred.kex.contains(&kex::DH_G1_SHA1));
        assert_eq!(legacy.keepalive_interval, None);
    }
}
