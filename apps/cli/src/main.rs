//! `hostwick` — command-line SSH, port forwarding and file transfer.
//!
//! Familiar to OpenSSH and PuTTY users: `hostwick ssh` ≈ `ssh`/`plink`,
//! `hostwick cp` ≈ `scp`/`pscp`, and saved profiles work everywhere a host does.

mod app;
mod cp;
mod hosts;
mod prompt;
mod shell;
mod sync;
mod tty;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "hostwick",
    version,
    about = "Fast, reliable SSH, SFTP and tunnels",
    propagate_version = true
)]
struct Cli {
    /// More logging (-v info, -vv debug). Also `HOSTWICK_LOG=debug`.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Never prompt. Unknown host keys and missing passwords fail.
    #[arg(long, global = true)]
    batch: bool,
    /// Trust and remember host keys seen for the first time (never changed ones).
    #[arg(long, global = true)]
    accept_new: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a shell or run a command (like `ssh`).
    Ssh(SshArgs),
    /// Copy files to/from a host (like `scp`): `cp [-r] SRC DST`, remote side as `HOST:PATH`.
    Cp(cp::CpArgs),
    /// Synchronize directories (like WinSCP): `sync [--delete] [--watch] SRC DST`.
    Sync(sync::SyncArgs),
    /// List a remote directory: `ls HOST:PATH`.
    Ls { target: String },
    /// Manage saved hosts.
    #[command(subcommand)]
    Hosts(hosts::HostsCmd),
    /// Import sessions from other tools.
    #[command(subcommand)]
    Import(hosts::ImportCmd),
    /// Manage the encrypted credential vault.
    #[command(subcommand)]
    Vault(hosts::VaultCmd),
    /// Show where Hostwick stores its files.
    Paths,
}

/// Connection options shared by commands that take a host.
#[derive(Args, Clone, Default)]
pub struct ConnOpts {
    /// Port (overrides the profile).
    #[arg(short = 'p', long)]
    port: Option<u16>,
    /// Login name (overrides the profile).
    #[arg(short = 'l', long = "login")]
    user: Option<String>,
    /// Private key file (repeatable).
    #[arg(short = 'i', long = "identity")]
    identity: Vec<std::path::PathBuf>,
    /// Jump host(s), comma-separated: `user@bastion:22,other`.
    #[arg(short = 'J', long = "jump")]
    jump: Option<String>,
    /// Allow legacy algorithms (ssh-rsa/SHA-1, old DH) for old devices.
    #[arg(long)]
    legacy: bool,
    /// Reach the (first) host through a proxy: `socks5://[user@]host:port`,
    /// `socks4://host:port` or `http://[user@]host:port`; `none` disables a
    /// saved one. The proxy password comes from the vault or a prompt.
    #[arg(long, value_name = "URL")]
    proxy: Option<String>,
    /// Kerberos (GSSAPI) login, forwarding the ticket to the server (like `ssh -K`).
    #[arg(short = 'K', long = "gssapi")]
    gssapi: bool,
}

#[derive(Args)]
struct SshArgs {
    #[command(flatten)]
    conn: ConnOpts,
    /// Local forward `[bind:]port:host:hostport` (repeatable).
    #[arg(short = 'L')]
    local: Vec<String>,
    /// Remote forward `[bind:]port:host:hostport` (repeatable).
    #[arg(short = 'R')]
    remote: Vec<String>,
    /// SOCKS5 proxy `[bind:]port` (repeatable).
    #[arg(short = 'D')]
    dynamic: Vec<String>,
    /// No shell or command; just keep the forwards open.
    #[arg(short = 'N')]
    no_command: bool,
    /// Force a PTY for a command (for interactive programs like `top`).
    #[arg(short = 't')]
    tty: bool,
    /// Forward the local SSH agent.
    #[arg(short = 'A')]
    agent: bool,
    /// Forward X11 so remote GUI programs show here (uses $DISPLAY, or the
    /// host's saved display; on Windows `localhost:0` for VcXsrv/Xming).
    #[arg(short = 'X')]
    x11: bool,
    /// Host or saved profile name: `[user@]host[:port]`.
    target: String,
    /// Command to run instead of a shell.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("hostwick: cannot start runtime: {e}");
            std::process::exit(1);
        }
    };
    let code = runtime.block_on(run(cli));
    // Don't wait for background tasks (e.g. a blocked stdin reader thread).
    runtime.shutdown_timeout(std::time::Duration::from_millis(200));
    std::process::exit(code);
}

async fn run(cli: Cli) -> i32 {
    let policy = prompt::Policy {
        batch: cli.batch,
        accept_new: cli.accept_new,
    };
    let result = async {
        let app = app::App::open(policy)?;
        match cli.cmd {
            Cmd::Ssh(a) => shell::run(&app, a).await,
            Cmd::Cp(a) => cp::run(&app, a).await,
            Cmd::Sync(a) => sync::run(&app, a).await,
            Cmd::Ls { target } => cp::ls(&app, &target).await,
            Cmd::Hosts(c) => hosts::run_hosts(&app, c),
            Cmd::Import(c) => hosts::run_import(&app, c),
            Cmd::Vault(c) => hosts::run_vault(&app, c),
            Cmd::Paths => {
                let p = &app.paths;
                println!("data:        {}", p.data_dir.display());
                println!("profiles:    {}", p.db.display());
                println!("vault:       {}", p.vault.display());
                println!("known_hosts: {}", p.known_hosts.display());
                println!("portable:    {}", p.portable);
                Ok(0)
            }
        }
    }
    .await;
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hostwick: {e:#}");
            255
        }
    }
}

fn init_logging(verbose: u8) {
    use tracing_subscriber::EnvFilter;
    let default = match verbose {
        0 => "warn",
        1 => "info,russh=warn",
        _ => "debug",
    };
    let filter =
        EnvFilter::try_from_env("HOSTWICK_LOG").unwrap_or_else(|_| EnvFilter::new(default));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

pub use app::App;
