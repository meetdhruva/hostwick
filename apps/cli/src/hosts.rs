use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use hostwick_core::{AuthMethod, ExposeSecret, HostProfile, ProfileStore};
use hostwick_import::{ImportResult, decode_text};
use hostwick_vault::Vault;

use crate::ConnOpts;
use crate::app::{App, parse_forwards};

// Parsed once per process; variant size is irrelevant here.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum HostsCmd {
    /// List saved hosts (optionally filtered by a substring).
    List { filter: Option<String> },
    /// Show one host in detail.
    Show { name: String },
    /// Save a host: `hosts add NAME [user@]host[:port] [options]`.
    Add {
        name: String,
        target: String,
        #[command(flatten)]
        conn: ConnOpts,
        /// Folder in the session tree, e.g. `prod/db`.
        #[arg(long, default_value = "")]
        folder: String,
        /// Use the vault credential with this id as the password.
        #[arg(long)]
        password_credential: Option<String>,
        /// Vault credential id with the `--proxy` password.
        #[arg(long)]
        proxy_credential: Option<String>,
        /// Forward the SSH agent (`-A`).
        #[arg(short = 'A')]
        agent: bool,
        /// Forward X11 (`-X`).
        #[arg(short = 'X')]
        x11: bool,
        #[arg(short = 'L')]
        local: Vec<String>,
        #[arg(short = 'R')]
        remote: Vec<String>,
        #[arg(short = 'D')]
        dynamic: Vec<String>,
    },
    /// Delete a saved host.
    Rm { name: String },
}

#[derive(Subcommand)]
pub enum ImportCmd {
    /// OpenSSH config (default: ~/.ssh/config).
    SshConfig {
        path: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    /// PuTTY sessions from a `.reg` export (or the registry on Windows when omitted).
    Putty {
        file: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    /// WinSCP sessions from WinSCP.ini (or the registry on Windows when omitted).
    Winscp {
        file: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Solar-PuTTY sessions exported without encryption (JSON).
    Solarputty {
        file: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum VaultCmd {
    /// Create the vault.
    Init,
    /// Store a secret (prompted) under an id.
    Set { id: String },
    /// Delete a secret.
    Rm { id: String },
    /// List secret ids (never values).
    List,
    /// Change the master password.
    Passwd,
}

fn find(app: &App, name: &str) -> Result<HostProfile> {
    app.store
        .find_by_name(name)?
        .with_context(|| format!("no saved host named `{name}`"))
}

pub fn run_hosts(app: &App, cmd: HostsCmd) -> Result<i32> {
    match cmd {
        HostsCmd::List { filter } => {
            let needle = filter.map(|f| f.to_lowercase());
            for p in app.store.list()? {
                let line = format!("{}  {}@{}:{}", p.path(), p.user_or_local(), p.host, p.port);
                if needle
                    .as_ref()
                    .is_none_or(|n| line.to_lowercase().contains(n))
                {
                    println!("{line}");
                }
            }
        }
        HostsCmd::Show { name } => {
            let p = find(app, &name)?;
            println!("name:      {}", p.path());
            println!("address:   {}@{}:{}", p.user_or_local(), p.host, p.port);
            for (i, j) in p.jump.iter().enumerate() {
                println!("jump {}:    {}:{}", i + 1, j.host, j.port);
            }
            for a in &p.auth {
                println!("auth:      {a:?}");
            }
            for f in &p.forwards {
                println!("forward:   {f}");
            }
            if let Some(proxy) = &p.proxy {
                println!("proxy:     {}", proxy.to_url());
            }
            if p.x11_forwarding || p.agent_forwarding {
                println!(
                    "forwarding: X11 {}{}  agent {}",
                    if p.x11_forwarding { "on" } else { "off" },
                    p.x11_display
                        .as_deref()
                        .map(|d| format!(" ({d})"))
                        .unwrap_or_default(),
                    if p.agent_forwarding { "on" } else { "off" }
                );
            }
            println!(
                "keepalive: {}s   legacy algorithms: {}",
                p.keepalive_secs, p.allow_legacy_algorithms
            );
        }
        HostsCmd::Add {
            name,
            target,
            conn,
            folder,
            password_credential,
            proxy_credential,
            agent,
            x11,
            local,
            remote,
            dynamic,
        } => {
            if app.store.find_by_name(&name)?.is_some() {
                bail!("a host named `{name}` already exists");
            }
            let mut p = app.resolve(&target, &conn)?;
            p.id = hostwick_core::ProfileId::new();
            p.name = name;
            p.folder = folder;
            p.forwards = parse_forwards(&local, &remote, &dynamic)?;
            p.agent_forwarding |= agent;
            p.x11_forwarding |= x11;
            if let Some(cred) = proxy_credential {
                match p.proxy.as_mut() {
                    Some(proxy) => proxy.password_credential = Some(cred),
                    None => bail!("--proxy-credential needs --proxy"),
                }
            }
            if let Some(cred) = password_credential {
                for a in &mut p.auth {
                    if let AuthMethod::Password { credential } = a {
                        *credential = Some(cred.clone());
                    }
                }
            }
            app.store.upsert(&p)?;
            println!("saved {}", p.path());
        }
        HostsCmd::Rm { name } => {
            let p = find(app, &name)?;
            app.store.delete(p.id)?;
            println!("deleted {}", p.path());
        }
    }
    Ok(0)
}

fn read(path: &PathBuf) -> Result<String> {
    Ok(decode_text(
        &std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
    ))
}

pub fn run_import(app: &App, cmd: ImportCmd) -> Result<i32> {
    let (result, dry_run) = match cmd {
        ImportCmd::SshConfig { path, dry_run } => {
            let path = match path {
                Some(p) => p,
                None => std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| PathBuf::from(h).join(".ssh").join("config"))
                    .context("no home directory")?,
            };
            let text = hostwick_import::load_ssh_config(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            (hostwick_import::parse_ssh_config(&text), dry_run)
        }
        ImportCmd::Putty {
            file: Some(f),
            dry_run,
        } => (hostwick_import::parse_putty_reg(&read(&f)?)?, dry_run),
        ImportCmd::Putty {
            file: None,
            dry_run,
        } => (hostwick_import::putty_from_registry()?, dry_run),
        ImportCmd::Winscp {
            file: Some(f),
            dry_run,
        } => (hostwick_import::parse_winscp_ini(&read(&f)?)?, dry_run),
        ImportCmd::Winscp {
            file: None,
            dry_run,
        } => (hostwick_import::winscp_from_registry()?, dry_run),
        ImportCmd::Solarputty { file, dry_run } => (
            hostwick_import::parse_solarputty_json(&read(&file)?)?,
            dry_run,
        ),
    };
    apply_import(app, result, dry_run)
}

/// Re-importing updates profiles from the same source instead of
/// duplicating them; hand-made edits to other fields are kept.
fn apply_import(app: &App, r: ImportResult, dry_run: bool) -> Result<i32> {
    let existing = app.store.list()?;
    let (mut added, mut updated) = (0, 0);
    for mut p in r.profiles {
        let prior = existing
            .iter()
            .find(|e| e.source == p.source && p.source != Default::default());
        let verb = if let Some(prior) = prior {
            p.id = prior.id;
            p.notes.clone_from(&prior.notes);
            p.tags.clone_from(&prior.tags);
            updated += 1;
            "update"
        } else {
            added += 1;
            "add   "
        };
        println!(
            "{verb} {}  ({}@{}:{})",
            p.path(),
            p.user_or_local(),
            p.host,
            p.port
        );
        if !dry_run {
            app.store.upsert(&p)?;
        }
    }
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
    println!(
        "{}{added} added, {updated} updated, {} warning(s)",
        if dry_run { "(dry run) " } else { "" },
        r.warnings.len()
    );
    Ok(0)
}

pub fn run_vault(app: &App, cmd: VaultCmd) -> Result<i32> {
    let path = &app.paths.vault;
    match cmd {
        VaultCmd::Init => {
            let pw = crate::tty::read_secret("New master password: ")?;
            let again = crate::tty::read_secret("Repeat: ")?;
            if pw.expose_secret() != again.expose_secret() {
                bail!("passwords did not match");
            }
            if pw.expose_secret().len() < 8 {
                bail!("use at least 8 characters");
            }
            Vault::create(path, &pw)?;
            println!("vault created at {}", path.display());
        }
        VaultCmd::Set { id } => {
            let mut v = app.unlock_vault()?;
            let secret = crate::tty::read_secret(&format!("Secret for `{id}`: "))?;
            v.set(id.clone(), secret)?;
            println!("stored `{id}`");
        }
        VaultCmd::Rm { id } => {
            let mut v = app.unlock_vault()?;
            println!(
                "{}",
                if v.remove(&id)? {
                    "removed"
                } else {
                    "no such id"
                }
            );
        }
        VaultCmd::List => {
            let v = app.unlock_vault()?;
            for id in v.ids() {
                println!("{id}");
            }
        }
        VaultCmd::Passwd => {
            let mut v = app.unlock_vault()?;
            let pw = crate::tty::read_secret("New master password: ")?;
            v.change_master(&pw)?;
            println!("master password changed");
        }
    }
    Ok(0)
}
