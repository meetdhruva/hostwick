//! Bridge between the GPUI (UI thread) world and the Tokio runtime that runs
//! every network operation.
//!
//! UI code never awaits network I/O directly: it spawns work on the runtime
//! and awaits the `JoinHandle` from a GPUI task, so the UI thread only ever
//! wakes up when there is a result to show.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use hostwick_core::{AuthMethod, HostProfile, SecretString, Target};
use hostwick_sftp::{RemoteFs, SftpFs, SftpOptions};
use hostwick_ssh::{
    ConnectContext, Connection, ConnectionPool, HostKeyDecision, HostKeyInfo, Interaction,
    KnownHosts, NoCredentials, PtyOptions, ShellSession,
};
use hostwick_store::{AppPaths, SqliteStore};
use hostwick_transfer::{EngineConfig, TransferEngine};

/// A question the SSH layer needs answered by the person.
pub enum Prompt {
    HostKey {
        info: HostKeyInfo,
        reply: oneshot::Sender<HostKeyDecision>,
    },
    Secret {
        title: String,
        label: String,
        reply: oneshot::Sender<Option<String>>,
    },
    KeyboardInteractive {
        title: String,
        instructions: String,
        prompts: Vec<(String, bool)>,
        reply: oneshot::Sender<Option<Vec<String>>>,
    },
    Banner(String),
}

/// [`Interaction`] implemented by forwarding to the UI and awaiting a reply.
/// A dismissed dialog (dropped reply) means "no" — never "yes".
struct UiInteraction {
    tx: UnboundedSender<Prompt>,
}

#[async_trait]
impl Interaction for UiInteraction {
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

    async fn password(&self, user: &str, host: &str) -> Option<SecretString> {
        let (reply, rx) = oneshot::channel();
        let title = format!("Password for {user}@{host}");
        self.tx
            .unbounded_send(Prompt::Secret {
                title,
                label: "Password".into(),
                reply,
            })
            .ok()?;
        rx.await.ok().flatten().map(SecretString::from)
    }

    async fn key_passphrase(&self, path: &Path) -> Option<SecretString> {
        let (reply, rx) = oneshot::channel();
        let title = format!("Passphrase for {}", path.display());
        self.tx
            .unbounded_send(Prompt::Secret {
                title,
                label: "Passphrase".into(),
                reply,
            })
            .ok()?;
        rx.await.ok().flatten().map(SecretString::from)
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

    fn banner(&self, text: &str) {
        let _ = self.tx.unbounded_send(Prompt::Banner(text.to_owned()));
    }
}

pub struct Backend {
    pub rt: tokio::runtime::Runtime,
    pub pool: ConnectionPool,
    pub store: SqliteStore,
    pub paths: AppPaths,
    pub engine: TransferEngine,
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
            paths,
            engine,
            forwards: std::sync::Mutex::default(),
            interaction: Arc::new(UiInteraction { tx: prompts }),
        })
    }

    pub fn context(&self) -> ConnectContext {
        let mut ctx = ConnectContext::new(self.interaction.clone(), Arc::new(NoCredentials));
        ctx.known_hosts = Some(KnownHosts::new(&self.paths.known_hosts));
        ctx.connect_timeout = Duration::from_secs(20);
        ctx
    }

    pub fn handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    /// Shared, pooled connection for this profile (one login per host).
    async fn connection(&self, profile: &HostProfile) -> Result<Connection> {
        let ctx = self.context();
        let backoff =
            hostwick_core::Backoff::new(Duration::from_millis(500), Duration::from_secs(5))
                .with_max_attempts(2);
        Ok(self
            .pool
            .get_or_connect(profile, &ctx, Some(backoff))
            .await?)
    }

    pub async fn open_shell(
        self: Arc<Self>,
        profile: HostProfile,
        cols: u32,
        rows: u32,
    ) -> Result<ShellSession> {
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
                Err(e) => tracing::warn!(error = %e, "could not start saved forward"),
            }
        }
        Ok(shell)
    }

    pub async fn open_files(self: Arc<Self>, profile: HostProfile) -> Result<Arc<dyn RemoteFs>> {
        let conn = self.connection(&profile).await?;
        let fs = SftpFs::open(&conn, &SftpOptions::default())
            .await
            .context("starting SFTP")?;
        Ok(Arc::new(fs))
    }
}

/// Ad-hoc profile from quick-connect text (`user@host:port`).
pub fn quick_profile(text: &str) -> Result<HostProfile> {
    let t = Target::parse(text)?;
    let mut p = HostProfile::new(t.to_string(), t.host.clone());
    p.port = t.port_or_default();
    p.user = t.user;
    let mut auth = vec![AuthMethod::Agent];
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        for k in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            let path = Path::new(&home).join(".ssh").join(k);
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
