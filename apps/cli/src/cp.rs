use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use hostwick_sftp::{LocalFs, RemoteFs, path_to_slash};
use hostwick_transfer::{
    EngineConfig, ExistingPolicy, Job, TransferEngine, TransferEvent, TransferState,
};

use crate::ConnOpts;
use crate::app::App;

#[derive(Args)]
pub struct CpArgs {
    #[command(flatten)]
    conn: ConnOpts,
    /// Copy directories recursively.
    #[arg(short = 'r', long)]
    recursive: bool,
    /// What to do when the destination exists.
    #[arg(long, value_enum, default_value_t = Existing::Overwrite)]
    existing: Existing,
    /// Files copied in parallel.
    #[arg(short = 'j', long, default_value_t = 4)]
    parallel: usize,
    /// Bandwidth cap in KiB/s.
    #[arg(long)]
    limit: Option<u64>,
    /// Source: local path or HOST:PATH.
    src: String,
    /// Destination: local path or HOST:PATH.
    dst: String,
}

#[derive(Clone, Copy, ValueEnum)]
enum Existing {
    /// Replace (resuming a matching partial file).
    Overwrite,
    /// Keep the existing file.
    Skip,
    /// Skip when size and modification time match.
    Newer,
}

impl From<Existing> for ExistingPolicy {
    fn from(e: Existing) -> Self {
        match e {
            Existing::Overwrite => Self::Overwrite,
            Existing::Skip => Self::Skip,
            Existing::Newer => Self::SkipIfSame,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Loc {
    Local(String),
    Remote { host: String, path: String },
}

/// `HOST:PATH` is remote; `C:\x`, `./a:b` and plain paths are local.
pub(crate) fn parse_loc(s: &str) -> Loc {
    let looks_like_drive = s.len() >= 2
        && s.as_bytes()[1] == b':'
        && s.as_bytes()[0].is_ascii_alphabetic()
        && (s.len() == 2 || matches!(s.as_bytes()[2], b'\\' | b'/'));
    if looks_like_drive || s.starts_with(['.', '/', '\\']) {
        return Loc::Local(s.to_owned());
    }
    // Skip a bracketed IPv6 host before looking for the separator.
    let search_from = if s.starts_with('[') {
        s.find(']').map_or(0, |i| i + 1)
    } else {
        0
    };
    match s[search_from..].find(':') {
        Some(i) => {
            let i = search_from + i;
            Loc::Remote {
                host: s[..i].to_owned(),
                path: s[i + 1..].to_owned(),
            }
        }
        None => Loc::Local(s.to_owned()),
    }
}

pub(crate) fn absolute_local(p: &str) -> Result<String> {
    Ok(path_to_slash(
        &std::path::absolute(Path::new(p)).with_context(|| format!("resolving {p}"))?,
    ))
}

fn base_name(p: &str) -> String {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(p)
        .to_owned()
}

pub(crate) async fn open_remote(
    app: &App,
    host: &str,
    opts: &ConnOpts,
) -> Result<Arc<dyn RemoteFs>> {
    let profile = app.resolve(host, opts)?;
    let ctx = app.context(&profile)?;
    let conn = hostwick_ssh::Connection::connect(&profile, &ctx)
        .await
        .with_context(|| format!("connecting to {}", profile.path()))?;
    remote_fs_for(app, &profile, &conn, &ctx).await
}

/// The host's file system over `conn`: its file protocol (SFTP, SCP or
/// auto) and sudo setting, asking for a sudo password when needed.
pub(crate) async fn remote_fs_for(
    app: &App,
    profile: &hostwick_core::HostProfile,
    conn: &hostwick_ssh::Connection,
    ctx: &hostwick_ssh::ConnectContext,
) -> Result<Arc<dyn RemoteFs>> {
    // Saved sudo password: its own credential, else the login password's.
    let mut saved = profile
        .sudo
        .password_credential
        .as_deref()
        .or_else(|| {
            profile.auth.iter().find_map(|a| match a {
                hostwick_core::AuthMethod::Password { credential } => credential.as_deref(),
                _ => None,
            })
        })
        .and_then(|id| ctx.credentials.get(id));
    let batch = app.policy.batch;
    let who = format!("{}@{}", profile.user_or_local(), profile.host);
    let sudo = profile.sudo.enabled.then_some(&profile.sudo);
    let fs = hostwick_sftp::open_remote_fs_with(conn, profile.file_protocol, sudo, |why| {
        let from_vault = saved.take();
        let who = who.clone();
        async move {
            if let Some(p) = from_vault {
                return Some(p);
            }
            if batch {
                return None;
            }
            if why.is_some() {
                eprintln!("sudo: wrong password, try again");
            }
            tokio::task::spawn_blocking(move || {
                crate::tty::read_secret(&format!("[sudo] password for {who}: ")).ok()
            })
            .await
            .ok()
            .flatten()
        }
    })
    .await
    .context("opening the remote file system")?;
    Ok(fs)
}

pub async fn run(app: &App, a: CpArgs) -> Result<i32> {
    let (src_loc, dst_loc) = (parse_loc(&a.src), parse_loc(&a.dst));
    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
    let (src_fs, src_path, dst_fs, dst_path) = match (src_loc, dst_loc) {
        (Loc::Remote { host, path }, Loc::Local(l)) => {
            let fs = open_remote(app, &host, &a.conn).await?;
            (
                fs,
                if path.is_empty() { ".".into() } else { path },
                local,
                absolute_local(&l)?,
            )
        }
        (Loc::Local(l), Loc::Remote { host, path }) => {
            let fs = open_remote(app, &host, &a.conn).await?;
            (
                local,
                absolute_local(&l)?,
                fs,
                if path.is_empty() { ".".into() } else { path },
            )
        }
        (Loc::Local(_), Loc::Local(_)) => bail!("both paths are local; use your OS copy command"),
        (Loc::Remote { .. }, Loc::Remote { .. }) => {
            bail!("remote-to-remote copy is not supported yet")
        }
    };

    let src_entry = src_fs
        .stat(&src_path)
        .await?
        .with_context(|| format!("{src_path}: not found"))?;
    // scp semantics: copying into an existing directory keeps the source name.
    let dst_is_dir = dst_fs.stat(&dst_path).await?.is_some_and(|e| e.is_dir());
    let target = if dst_is_dir {
        hostwick_sftp::join(&dst_path, &base_name(&src_path))
    } else {
        dst_path
    };

    let engine = TransferEngine::new(EngineConfig {
        max_concurrent: a.parallel.max(1),
        rate_limit: a.limit.map(|k| k * 1024),
        ..Default::default()
    });
    let events = engine.subscribe();
    let ids = if src_entry.is_dir() {
        if !a.recursive {
            bail!("{src_path} is a directory (use -r)");
        }
        engine
            .enqueue_tree(src_fs, &src_path, dst_fs, &target, a.existing.into())
            .await?
    } else {
        vec![engine.enqueue(Job {
            src: src_fs,
            src_path,
            dst: dst_fs,
            dst_path: target,
            policy: a.existing.into(),
        })]
    };

    let summary = drive(&engine, events, &ids).await;
    print_summary(&summary);
    Ok(if summary.failed > 0 { 1 } else { 0 })
}

/// Totals of one batch of transfers.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TransferSummary {
    pub files: usize,
    pub bytes: u64,
    pub failed: usize,
    pub secs: f64,
}

/// Wait until every transfer in `ids` has finished, drawing progress on a
/// terminal and reporting skips, resumes and failures on stderr.
pub(crate) async fn drive(
    engine: &TransferEngine,
    mut events: tokio::sync::broadcast::Receiver<TransferEvent>,
    ids: &[hostwick_transfer::TransferId],
) -> TransferSummary {
    let show_progress = std::io::stderr().is_terminal();
    let mut remaining = ids.len();
    let (mut failed, mut bytes) = (0usize, 0u64);
    let started = std::time::Instant::now();
    let mut labels = std::collections::HashMap::new();
    while remaining > 0 {
        match events.recv().await {
            Ok(TransferEvent::Queued { id, label }) => {
                labels.insert(id, label);
            }
            Ok(TransferEvent::Progress {
                done,
                total,
                bytes_per_sec,
                id,
            }) if show_progress => {
                let pct = if total == 0 {
                    100.0
                } else {
                    done as f64 * 100.0 / total as f64
                };
                let name = labels.get(&id).map(String::as_str).unwrap_or("");
                eprint!(
                    "\r\x1b[2K{pct:5.1}%  {:>10}  {:>10}/s  {name}",
                    human(done),
                    human(bytes_per_sec as u64)
                );
                let _ = std::io::stderr().flush();
            }
            Ok(TransferEvent::Finished { id, state }) => {
                remaining -= 1;
                let name = labels.get(&id).cloned().unwrap_or_default();
                if show_progress {
                    eprint!("\r\x1b[2K");
                }
                match state {
                    TransferState::Done {
                        bytes: b,
                        resumed_from,
                    } => {
                        bytes += b;
                        if resumed_from > 0 {
                            eprintln!("resumed  {name} (from {})", human(resumed_from));
                        }
                    }
                    TransferState::Skipped => eprintln!("skipped  {name}"),
                    TransferState::Failed { error } => {
                        failed += 1;
                        eprintln!("FAILED   {name}: {error}");
                    }
                    TransferState::Cancelled => failed += 1,
                    TransferState::Queued | TransferState::Running => {}
                }
            }
            Ok(_) => {}
            // Progress events dropped under load are fine; Finished are not
            // missed because the engine's state table is authoritative.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                remaining = ids
                    .iter()
                    .filter(|id| !engine.state(**id).is_some_and(|s| s.is_finished()))
                    .count();
            }
            Err(_) => break,
        }
    }
    TransferSummary {
        files: ids.len(),
        bytes,
        failed,
        secs: started.elapsed().as_secs_f64().max(1e-3),
    }
}

pub(crate) fn print_summary(s: &TransferSummary) {
    eprintln!(
        "{} file(s), {} in {:.1}s ({}/s){}",
        s.files,
        human(s.bytes),
        s.secs,
        human((s.bytes as f64 / s.secs) as u64),
        if s.failed > 0 {
            format!(", {} failed", s.failed)
        } else {
            String::new()
        }
    );
}

pub async fn ls(app: &App, target: &str, json: bool) -> Result<i32> {
    let Loc::Remote { host, path } = parse_loc(target) else {
        bail!("expected HOST:PATH")
    };
    let fs = open_remote(app, &host, &ConnOpts::default()).await?;
    let path = if path.is_empty() {
        ".".to_owned()
    } else {
        path
    };
    let entries = fs.list_dir(&path).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(0);
    }
    for e in entries {
        print_entry(&e);
    }
    Ok(0)
}

/// One `ls -l`-style line.
pub(crate) fn print_entry(e: &hostwick_sftp::Entry) {
    {
        let kind = match e.kind {
            hostwick_sftp::EntryKind::Dir => 'd',
            hostwick_sftp::EntryKind::Symlink => 'l',
            _ => '-',
        };
        let perms = e
            .permissions
            .map(mode_string)
            .unwrap_or_else(|| "?????????".into());
        println!(
            "{kind}{perms} {:>10} {}{}",
            e.size,
            e.name,
            if e.is_dir() { "/" } else { "" }
        );
    }
}

pub(crate) fn mode_string(m: u32) -> String {
    let mut s = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let b = (m >> shift) & 7;
        s.push(if b & 4 != 0 { 'r' } else { '-' });
        s.push(if b & 2 != 0 { 'w' } else { '-' });
        s.push(if b & 1 != 0 { 'x' } else { '-' });
    }
    s
}

pub fn human(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        let r = |h: &str, p: &str| Loc::Remote {
            host: h.into(),
            path: p.into(),
        };
        assert_eq!(parse_loc("web:/var/log"), r("web", "/var/log"));
        assert_eq!(parse_loc("ops@web:2222:/x"), r("ops@web", "2222:/x"));
        assert_eq!(parse_loc("web:"), r("web", ""));
        assert_eq!(parse_loc("[::1]:/tmp"), r("[::1]", "/tmp"));
        assert_eq!(parse_loc(r"C:\Users\me"), Loc::Local(r"C:\Users\me".into()));
        assert_eq!(parse_loc("C:/x"), Loc::Local("C:/x".into()));
        assert_eq!(parse_loc("./a:b"), Loc::Local("./a:b".into()));
        assert_eq!(parse_loc("plain.txt"), Loc::Local("plain.txt".into()));
        assert_eq!(base_name("/a/b/"), "b");
        assert_eq!(mode_string(0o754), "rwxr-xr--");
        assert_eq!(human(1536), "1.5 KiB");
    }
}
