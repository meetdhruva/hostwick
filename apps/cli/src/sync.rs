//! `hostwick sync` — WinSCP's "Synchronize" and "Keep remote directory up to
//! date", for either direction.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use hostwick_sftp::{LocalFs, RemoteFs};
use hostwick_transfer::{
    EngineConfig, SyncMode, SyncOptions, SyncReport, TransferEngine, sync_once, watch,
};
use tokio_util::sync::CancellationToken;

use crate::ConnOpts;
use crate::app::App;
use crate::cp::{Loc, absolute_local, human, open_remote, parse_loc};

#[derive(Args)]
pub struct SyncArgs {
    #[command(flatten)]
    conn: ConnOpts,
    /// Delete destination files that are not in the source (mirror).
    #[arg(long)]
    delete: bool,
    /// Keep running and sync whenever the source changes (Ctrl+C to stop).
    #[arg(short = 'w', long)]
    watch: bool,
    /// Seconds between checks in --watch mode.
    #[arg(long, default_value_t = 2.0)]
    interval: f64,
    /// Skip matching files, e.g. `-x '*.tmp' -x .git -x node_modules` (repeatable).
    #[arg(short = 'x', long = "exclude")]
    exclude: Vec<String>,
    /// Show what would change without changing anything.
    #[arg(short = 'n', long)]
    dry_run: bool,
    /// Files copied in parallel.
    #[arg(short = 'j', long, default_value_t = 4)]
    parallel: usize,
    /// Bandwidth cap in KiB/s.
    #[arg(long)]
    limit: Option<u64>,
    /// Source directory: local path or HOST:PATH.
    src: String,
    /// Destination directory: local path or HOST:PATH.
    dst: String,
}

fn print_report(r: &SyncReport, dry_run: bool) {
    let (copy, delete) = if dry_run {
        ("would copy  ", "would delete")
    } else {
        ("copied      ", "deleted     ")
    };
    for p in &r.copied {
        println!("{copy} {p}");
    }
    for p in &r.deleted {
        println!("{delete} {p}");
    }
    for (p, e) in &r.failed {
        eprintln!("FAILED       {p}: {e}");
    }
    if r.is_noop() {
        eprintln!("up to date");
    } else if !dry_run {
        eprintln!(
            "{} copied ({}), {} deleted, {} failed",
            r.copied.len(),
            human(r.bytes),
            r.deleted.len(),
            r.failed.len()
        );
    }
}

pub async fn run(app: &App, a: SyncArgs) -> Result<i32> {
    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
    let (src, src_root, dst, dst_root) = match (parse_loc(&a.src), parse_loc(&a.dst)) {
        (Loc::Local(l), Loc::Remote { host, path }) => {
            let remote = open_remote(app, &host, &a.conn).await?;
            (
                local,
                absolute_local(&l)?,
                remote,
                if path.is_empty() { ".".into() } else { path },
            )
        }
        (Loc::Remote { host, path }, Loc::Local(l)) => {
            let remote = open_remote(app, &host, &a.conn).await?;
            (
                remote,
                if path.is_empty() { ".".into() } else { path },
                local,
                absolute_local(&l)?,
            )
        }
        _ => bail!("exactly one side must be remote (HOST:PATH)"),
    };
    match src.stat(&src_root).await? {
        Some(e) if e.is_dir() => {}
        Some(_) => bail!("{src_root} is not a directory (use `hostwick cp` for single files)"),
        None => bail!("{src_root}: not found"),
    }

    let engine = TransferEngine::new(EngineConfig {
        max_concurrent: a.parallel.max(1),
        rate_limit: a.limit.map(|k| k * 1024),
        ..Default::default()
    });
    let opts = SyncOptions {
        mode: if a.delete {
            SyncMode::Mirror
        } else {
            SyncMode::Update
        },
        excludes: a.exclude.clone(),
        dry_run: a.dry_run,
    };

    if !a.watch || a.dry_run {
        let r = sync_once(&engine, src, &src_root, dst, &dst_root, &opts).await?;
        print_report(&r, a.dry_run);
        return Ok(if r.failed.is_empty() { 0 } else { 1 });
    }

    eprintln!("watching {src_root} → {dst_root} (Ctrl+C to stop)");
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop.cancel();
    });
    let interval = Duration::from_secs_f64(a.interval.max(0.2));
    watch(
        &engine,
        src,
        &src_root,
        dst,
        &dst_root,
        &opts,
        interval,
        cancel,
        |r| match r {
            Ok(report) if report.is_noop() => {}
            Ok(report) => print_report(report, false),
            Err(e) => eprintln!("sync error (will retry): {e}"),
        },
    )
    .await;
    Ok(0)
}
