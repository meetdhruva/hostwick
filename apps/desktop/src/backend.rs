//! Bridge between the GPUI (UI thread) world and the Tokio runtime that runs
//! every network operation.
//!
//! UI code never awaits network I/O directly: it spawns work on the runtime
//! and awaits the `JoinHandle` from a GPUI task, so the UI thread only ever
//! wakes up when there is a result to show.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use hostwick_core::{
    AuthMethod, HostProfile, ProfileId, ProfileStore, Protocol, SecretString, Target,
};
use hostwick_sftp::{RemoteFs, SudoError};
use hostwick_ssh::{
    ConnectContext, Connection, ConnectionPool, HostKeyDecision, HostKeyInfo, Interaction,
    KnownHosts, PtyOptions, SecretUse, SshError,
};
use hostwick_store::{AppPaths, SqliteStore};
use hostwick_term::{TerminalSession, telnet};
use hostwick_transfer::{EngineConfig, TransferEngine};

use crate::vault::{
    CredKind, VaultCredentials, VaultManager, assign_id, credential_id, existing_id,
};

/// "Remember" checkbox offered on a secret prompt.
#[derive(Debug, Clone)]
pub struct RememberOffer {
    pub label: String,
    /// Ticked to start with (replacing a saved secret that was rejected).
    pub checked: bool,
}

/// What the person typed into a secret prompt.
pub struct SecretAnswer {
    pub value: String,
    pub remember: bool,
}

/// A question the SSH layer needs answered by the person.
pub enum Prompt {
    HostKey {
        info: HostKeyInfo,
        reply: oneshot::Sender<HostKeyDecision>,
    },
    Secret {
        title: String,
        label: String,
        /// Shown above the field (e.g. "The saved password was rejected").
        note: Option<String>,
        remember: Option<RememberOffer>,
        reply: oneshot::Sender<Option<SecretAnswer>>,
    },
    KeyboardInteractive {
        title: String,
        instructions: String,
        prompts: Vec<(String, bool)>,
        reply: oneshot::Sender<Option<Vec<String>>>,
    },
    Banner(String),
    /// Something worth a passing notification (X11 not available, touch the
    /// security key, a key that could not be used).
    Notice(String),
    /// Unlock the vault (or create it when `create`); reply true once open.
    Vault {
        create: bool,
        reason: String,
        reply: oneshot::Sender<bool>,
    },
    /// Saved hosts changed in the background (a password was remembered).
    ProfilesChanged,
}

/// [`Interaction`] implemented by forwarding to the UI and awaiting a reply.
/// A dismissed dialog (dropped reply) means "no" — never "yes".
struct UiInteraction {
    tx: UnboundedSender<Prompt>,
}

impl UiInteraction {
    async fn secret(
        &self,
        title: String,
        label: &str,
        note: Option<String>,
        remember: Option<RememberOffer>,
    ) -> Option<SecretAnswer> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .unbounded_send(Prompt::Secret {
                title,
                label: label.into(),
                note,
                remember,
                reply,
            })
            .ok()?;
        rx.await.ok().flatten()
    }

    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision {
        let (reply, rx) = oneshot::channel();
        if self
            .tx
            .unbounded_send(Prompt::HostKey {
                info: info.clone(),
                reply,
            })
            .is_err()
        {
            return HostKeyDecision::Reject;
        }
        rx.await.unwrap_or(HostKeyDecision::Reject)
    }

    async fn keyboard_interactive(
        &self,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Vec<String>> {
        let (reply, rx) = oneshot::channel();
        let title = if name.is_empty() {
            "Server authentication".to_owned()
        } else {
            name.to_owned()
        };
        self.tx
            .unbounded_send(Prompt::KeyboardInteractive {
                title,
                instructions: instructions.to_owned(),
                prompts: prompts.to_vec(),
                reply,
            })
            .ok()?;
        rx.await.ok().flatten()
    }

    fn notice(&self, text: &str) {
        let _ = self.tx.unbounded_send(Prompt::Notice(text.to_owned()));
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| path.to_owned()),
        Err(_) => path.to_owned(),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[derive(Default)]
struct SessionState {
    /// Typed secrets the person asked to remember, not yet confirmed.
    typed: Vec<(CredKind, SecretString)>,
    /// Typed, ticked "remember" and accepted by the server: to be saved.
    accepted: Vec<(CredKind, SecretString)>,
    /// Saved secrets that were rejected during this connect.
    rejected_saved: Vec<CredKind>,
}

/// The interaction for one connect of one host: knows which host it is, so
/// password prompts can offer "Remember", and remembers what to save once
/// the server accepted it.
struct SessionInteraction {
    ui: Arc<UiInteraction>,
    vault: Arc<VaultManager>,
    /// Saved host (prompts offer to remember); None for quick connect.
    profile: Option<HostProfile>,
    state: Mutex<SessionState>,
}

impl SessionInteraction {
    /// Which of the host's secrets a prompt for `user@host` is.
    fn classify(&self, secret: SecretUse<'_>) -> Option<CredKind> {
        let p = self.profile.as_ref()?;
        match secret {
            SecretUse::Password { host, .. } => {
                if let (Some(proxy), Some(name)) = (&p.proxy, host.strip_prefix("proxy "))
                    && name == proxy.host
                {
                    return Some(CredKind::Proxy);
                }
                // Jump hosts share the auth list but not the password.
                (host == p.host).then_some(CredKind::Password)
            }
            SecretUse::KeyPassphrase(path) => p.auth.iter().find_map(|a| match a {
                AuthMethod::KeyFile { path: k, .. } if k == path || expand_tilde(k) == path => {
                    Some(CredKind::KeyPassphrase(k.clone()))
                }
                _ => None,
            }),
        }
    }

    fn offer(
        &self,
        kind: Option<&CredKind>,
        what: &str,
    ) -> (Option<String>, Option<RememberOffer>) {
        let Some(kind) = kind else {
            return (None, None);
        };
        let rejected = self
            .state
            .lock()
            .map(|s| s.rejected_saved.contains(kind))
            .unwrap_or(false);
        if rejected {
            (
                Some(format!(
                    "The saved {what} was not accepted. Type the current one."
                )),
                Some(RememberOffer {
                    label: format!("Update the saved {what}"),
                    checked: true,
                }),
            )
        } else {
            (
                None,
                Some(RememberOffer {
                    label: format!("Remember {what}"),
                    checked: false,
                }),
            )
        }
    }

    async fn ask(
        &self,
        kind: Option<CredKind>,
        title: String,
        label: &str,
        what: &str,
    ) -> Option<SecretString> {
        let (note, offer) = self.offer(kind.as_ref(), what);
        let ans = self.ui.secret(title, label, note, offer).await?;
        let secret = SecretString::from(ans.value);
        if ans.remember
            && let Some(kind) = kind
            && let Ok(mut s) = self.state.lock()
        {
            s.typed.retain(|(k, _)| *k != kind);
            s.typed.push((kind, secret.clone()));
        }
        Some(secret)
    }

    fn take_accepted(&self) -> Vec<(CredKind, SecretString)> {
        self.state
            .lock()
            .map(|mut s| std::mem::take(&mut s.accepted))
            .unwrap_or_default()
    }
}

#[async_trait]
impl Interaction for SessionInteraction {
    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision {
        self.ui.host_key(info).await
    }

    async fn password(&self, user: &str, host: &str) -> Option<SecretString> {
        let kind = self.classify(SecretUse::Password { user, host });
        let what = if kind == Some(CredKind::Proxy) {
            "proxy password"
        } else {
            "password"
        };
        self.ask(
            kind,
            format!("Password for {user}@{host}"),
            "Password",
            what,
        )
        .await
    }

    async fn key_passphrase(&self, path: &Path) -> Option<SecretString> {
        let kind = self.classify(SecretUse::KeyPassphrase(path));
        self.ask(
            kind,
            format!("Passphrase for {}", path.display()),
            "Passphrase",
            "passphrase",
        )
        .await
    }

    async fn keyboard_interactive(
        &self,
        name: &str,
        instructions: &str,
        prompts: &[(String, bool)],
    ) -> Option<Vec<String>> {
        self.ui
            .keyboard_interactive(name, instructions, prompts)
            .await
    }

    fn banner(&self, text: &str) {
        let _ = self.ui.tx.unbounded_send(Prompt::Banner(text.to_owned()));
    }

    fn notice(&self, text: &str) {
        self.ui.notice(text);
    }

    fn secret_outcome(&self, secret: SecretUse<'_>, saved: bool, accepted: bool) {
        let Some(kind) = self.classify(secret) else {
            return;
        };
        let Ok(mut s) = self.state.lock() else {
            return;
        };
        if saved && !accepted {
            if let Some(id) = self.profile.as_ref().and_then(|p| existing_id(p, &kind)) {
                self.vault.mark_stale(&id);
            }
            s.rejected_saved.push(kind);
        } else if !saved && accepted {
            if let Some(i) = s.typed.iter().position(|(k, _)| *k == kind) {
                let item = s.typed.remove(i);
                s.accepted.retain(|(k, _)| *k != kind);
                s.accepted.push(item);
            }
        } else if !saved {
            // Rejected typed secret: never save it.
            s.typed.retain(|(k, _)| *k != kind);
        }
    }
}

/// Errors after which retrying cannot help (wrong credentials, a rejected
/// or changed host key): auto-reconnect stops at these.
pub fn is_fatal(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        matches!(
            c.downcast_ref::<SshError>(),
            Some(
                SshError::AuthFailed { .. }
                    | SshError::HostKeyRejected(_)
                    | SshError::KnownHosts(_)
                    | SshError::Key { .. }
            )
        )
    })
}

pub struct Backend {
    pub rt: tokio::runtime::Runtime,
    pub pool: ConnectionPool,
    pub store: SqliteStore,
    pub paths: AppPaths,
    pub engine: TransferEngine,
    pub vault: Arc<VaultManager>,
    /// One vault dialog at a time (restored tabs connect together).
    vault_gate: tokio::sync::Mutex<()>,
    /// Saved forwards started with a terminal; dropped (stopped) on exit.
    forwards: std::sync::Mutex<Vec<hostwick_ssh::ForwardHandle>>,
    interaction: Arc<UiInteraction>,
}

impl Backend {
    pub fn new(prompts: UnboundedSender<Prompt>) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("hostwick-io")
            .enable_all()
            .build()
            .context("starting network runtime")?;
        let paths = AppPaths::detect();
        paths
            .ensure()
            .with_context(|| format!("creating {}", paths.data_dir.display()))?;
        let store = SqliteStore::open(&paths.db).context("opening profile database")?;
        let engine = {
            let _guard = rt.enter();
            TransferEngine::new(EngineConfig::default())
        };
        Ok(Self {
            rt,
            pool: ConnectionPool::new(),
            store,
            vault: Arc::new(VaultManager::new(paths.vault.clone())),
            paths,
            engine,
            vault_gate: tokio::sync::Mutex::new(()),
            forwards: std::sync::Mutex::default(),
            interaction: Arc::new(UiInteraction { tx: prompts }),
        })
    }

    /// Context for connecting to `profile`: vault-backed credentials and
    /// prompts that can offer to remember what is typed.
    fn session(&self, profile: &HostProfile) -> (ConnectContext, Arc<SessionInteraction>) {
        let saved = self.store.get(profile.id).is_ok();
        let session = Arc::new(SessionInteraction {
            ui: self.interaction.clone(),
            vault: self.vault.clone(),
            profile: saved.then(|| profile.clone()),
            state: Mutex::default(),
        });
        let mut ctx = ConnectContext::new(
            session.clone(),
            Arc::new(VaultCredentials(self.vault.clone())),
        );
        ctx.known_hosts = Some(KnownHosts::new(&self.paths.known_hosts));
        ctx.connect_timeout = Duration::from_secs(20);
        (ctx, session)
    }

    pub fn handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    pub fn send(&self, p: Prompt) {
        let _ = self.interaction.tx.unbounded_send(p);
    }

    pub fn notice(&self, text: impl Into<String>) {
        self.send(Prompt::Notice(text.into()));
    }

    /// Open the vault through the UI. True once unlocked; false if the
    /// person declined.
    pub async fn ensure_vault(&self, reason: String) -> bool {
        if self.vault.is_unlocked() {
            return true;
        }
        let _gate = self.vault_gate.lock().await;
        if self.vault.is_unlocked() {
            return true;
        }
        let (reply, rx) = oneshot::channel();
        self.send(Prompt::Vault {
            create: !self.vault.exists(),
            reason,
            reply,
        });
        rx.await.unwrap_or(false)
    }

    /// Before connecting: offer to unlock the vault when the host has saved
    /// secrets in it (once per session if the person says "not now").
    async fn prepare_vault(&self, profile: &HostProfile) {
        if self.vault.is_unlocked() || self.vault.declined() || !self.vault.exists() {
            return;
        }
        if crate::vault::profile_ids(profile).is_empty() {
            return;
        }
        if !self
            .ensure_vault(format!(
                "{} has saved passwords. Unlock them with your master password.",
                profile.name
            ))
            .await
        {
            self.vault.set_declined(true);
        }
    }

    /// Save secrets the person asked to remember (and the server accepted)
    /// into the vault, and point the saved host at them.
    async fn remember(&self, profile: ProfileId, items: Vec<(CredKind, SecretString)>) {
        if items.is_empty() {
            return;
        }
        let reason = if self.vault.exists() {
            "Unlock your saved passwords to store this one.".to_owned()
        } else {
            "Create a vault for saved passwords. One master password protects them; it is never stored.".to_owned()
        };
        if !self.ensure_vault(reason).await {
            self.notice("Not saved: the password vault is locked.");
            return;
        }
        let Ok(mut p) = self.store.get(profile) else {
            return;
        };
        let mut saved = 0;
        for (kind, secret) in items {
            let id = existing_id(&p, &kind).unwrap_or_else(|| credential_id(p.id, &kind));
            match self.vault.set(&id, secret) {
                Ok(()) => {
                    if assign_id(&mut p, &kind, Some(id)) {
                        saved += 1;
                    }
                }
                Err(e) => self.notice(format!("Could not save the password: {e}")),
            }
        }
        if saved > 0 {
            match self.store.upsert(&p) {
                Ok(()) => {
                    tracing::info!(host = %p.name, saved, "remembered credentials");
                    self.send(Prompt::ProfilesChanged);
                }
                Err(e) => self.notice(format!("Could not update {}: {e}", p.name)),
            }
        }
    }

    /// [`remember`](Self::remember) in the background, so the session opens
    /// while the vault dialog is up.
    fn remember_later(self: &Arc<Self>, profile: ProfileId, items: Vec<(CredKind, SecretString)>) {
        if items.is_empty() {
            return;
        }
        let this = self.clone();
        self.rt
            .spawn(async move { this.remember(profile, items).await });
    }

    /// Shared, pooled connection for this profile (one login per host).
    pub(crate) async fn connection(self: &Arc<Self>, profile: &HostProfile) -> Result<Connection> {
        self.prepare_vault(profile).await;
        let (ctx, session) = self.session(profile);
        let backoff =
            hostwick_core::Backoff::new(Duration::from_millis(500), Duration::from_secs(5))
                .with_max_attempts(2);
        let conn = self
            .pool
            .get_or_connect(profile, &ctx, Some(backoff))
            .await?;
        self.remember(profile.id, session.take_accepted()).await;
        Ok(conn)
    }

    /// Open the host's terminal over its protocol: an SSH shell (plus its
    /// saved forwards), a telnet connection (through the host's proxy, if
    /// any) or a local serial port.
    pub async fn open_shell(
        self: Arc<Self>,
        profile: HostProfile,
        cols: u32,
        rows: u32,
    ) -> Result<TerminalSession> {
        match &profile.protocol {
            Protocol::Ssh => {}
            Protocol::Telnet => {
                let opts = telnet::TelnetOptions::from_profile(&profile, cols, rows);
                let session = match &profile.proxy {
                    Some(spec) => {
                        self.prepare_vault(&profile).await;
                        let (ctx, session) = self.session(&profile);
                        let tcp =
                            hostwick_ssh::connect_via_proxy(spec, &ctx, &opts.host, opts.port)
                                .await?;
                        self.remember_later(profile.id, session.take_accepted());
                        telnet::tune_socket(&tcp, opts.keepalive);
                        telnet::start(tcp, &opts)
                    }
                    None => telnet::connect(&opts).await?,
                };
                return Ok(session);
            }
            Protocol::Serial(settings) => {
                return Ok(hostwick_term::serial::open(&profile.host, settings).await?);
            }
        }
        let conn = self.connection(&profile).await?;
        let pty = PtyOptions {
            term: profile.term.clone(),
            cols,
            rows,
        };
        let shell = conn.open_shell(pty).await?;
        for spec in &profile.forwards {
            match conn.start_forward(spec).await {
                Ok(h) => {
                    if let Ok(mut f) = self.forwards.lock() {
                        f.push(h);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "could not start saved forward");
                    self.notice(format!("{}: {e}", profile.name));
                }
            }
        }
        Ok(shell.into())
    }

    /// The file browser's remote side: the profile's file protocol, elevated
    /// through sudo when the profile says so. If sudo fails the browser
    /// still opens, unelevated, with the reason in `notice`.
    pub async fn open_files(self: Arc<Self>, profile: HostProfile) -> Result<FilesSession> {
        if profile.protocol != Protocol::Ssh {
            anyhow::bail!(
                "Files need SSH: {} is a {} connection, which cannot transfer files.",
                profile.name,
                profile.protocol.label().to_lowercase()
            );
        }
        if profile.sudo.enabled {
            match self.clone().open_remote(profile.clone(), true).await {
                Ok(s) => return Ok(s),
                Err(e) => {
                    let mut s = self.open_remote(profile, false).await?;
                    s.notice = Some(format!("Could not open as root: {e:#}"));
                    return Ok(s);
                }
            }
        }
        self.open_remote(profile, false).await
    }

    /// Open the remote file system, elevated (`sudo`) or not. The sudo
    /// password comes from the vault (the host's sudo credential, else its
    /// login password credential) or is asked for through the UI, and
    /// asked again after a wrong one (offering to remember it).
    pub async fn open_remote(
        self: Arc<Self>,
        profile: HostProfile,
        elevate: bool,
    ) -> Result<FilesSession> {
        let conn = self.connection(&profile).await?;
        let sudo = elevate.then_some(&profile.sudo);
        let saved_id = existing_id(&profile, &CredKind::Sudo)
            .or_else(|| existing_id(&profile, &CredKind::Password));
        let mut saved = saved_id.as_deref().and_then(|id| self.vault.get(id));
        let can_remember = self.store.get(profile.id).is_ok();
        let who = format!("{}@{}", profile.user_or_local(), profile.host);
        // What was typed last, if the person ticked "remember".
        let typed: Arc<Mutex<Option<SecretString>>> = Arc::default();
        let used_saved = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let this = self.clone();
        let fs = {
            let (typed, used_saved, saved_id) =
                (typed.clone(), used_saved.clone(), saved_id.clone());
            hostwick_sftp::open_remote_fs_with(&conn, profile.file_protocol, sudo, move |why| {
                let from_vault = saved.take();
                let (this, who, typed, used_saved, saved_id) = (
                    this.clone(),
                    who.clone(),
                    typed.clone(),
                    used_saved.clone(),
                    saved_id.clone(),
                );
                async move {
                    if let Some(p) = from_vault {
                        used_saved.store(true, std::sync::atomic::Ordering::Relaxed);
                        return Some(p);
                    }
                    let wrong = matches!(why, Some(SudoError::WrongPassword));
                    let saved_rejected =
                        wrong && used_saved.swap(false, std::sync::atomic::Ordering::Relaxed);
                    if saved_rejected && let Some(id) = &saved_id {
                        this.vault.mark_stale(id);
                    }
                    let title = if wrong && !saved_rejected {
                        format!("Wrong password. sudo password for {who}")
                    } else {
                        format!("sudo password for {who}")
                    };
                    let note = saved_rejected
                        .then(|| "sudo did not accept the saved password.".to_owned());
                    let offer = can_remember.then(|| RememberOffer {
                        label: if saved_rejected {
                            "Update the saved sudo password".into()
                        } else {
                            "Remember sudo password".into()
                        },
                        checked: saved_rejected,
                    });
                    let ans = this
                        .interaction
                        .secret(title, "Password", note, offer)
                        .await?;
                    let secret = SecretString::from(ans.value);
                    if let Ok(mut t) = typed.lock() {
                        *t = ans.remember.then(|| secret.clone());
                    }
                    Some(secret)
                }
            })
            .await
            .context(if elevate {
                "opening files as root"
            } else {
                "opening the file browser"
            })?
        };
        let to_save = typed.lock().ok().and_then(|mut t| t.take());
        if let Some(secret) = to_save {
            self.remember(profile.id, vec![(CredKind::Sudo, secret)])
                .await;
        }
        Ok(FilesSession {
            elevated: elevate,
            fs,
            notice: None,
        })
    }

    /// Ask the person for a secret through the app's prompt dialogs.
    pub async fn ask_secret(&self, title: String, label: String) -> Option<SecretString> {
        self.interaction
            .secret(title, &label, None, None)
            .await
            .map(|a| SecretString::from(a.value))
    }
}

/// What [`Backend::open_files`] opened.
pub struct FilesSession {
    pub fs: Arc<dyn RemoteFs>,
    /// Running through sudo.
    pub elevated: bool,
    /// Something to tell the person (e.g. why sudo was not used).
    pub notice: Option<String>,
}

/// Ad-hoc profile from quick-connect text (`user@host:port`).
pub fn quick_profile(text: &str) -> Result<HostProfile> {
    let t = Target::parse(text)?;
    let mut p = HostProfile::new(t.to_string(), t.host.clone());
    p.port = t.port_or_default();
    p.user = t.user;
    let mut auth = vec![AuthMethod::Agent];
    if let Some(home) = home_dir() {
        for k in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            let path = home.join(".ssh").join(k);
            if path.exists() {
                auth.push(AuthMethod::KeyFile {
                    path,
                    passphrase_credential: None,
                });
            }
        }
    }
    auth.push(AuthMethod::Password { credential: None });
    auth.push(AuthMethod::KeyboardInteractive);
    p.auth = auth;
    Ok(p)
}
