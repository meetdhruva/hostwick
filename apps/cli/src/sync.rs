//! `hostwick sync` — WinSCP's "Synchronize" and "Keep remote directory up to
//! date", for either direction, or both (`--both`).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use hostwick_sftp::{LocalFs, RemoteFs};
use hostwick_transfer::{
    ActionKind, EngineConfig, PlanOptions, SyncDirection, SyncMode, SyncOptions, SyncPlan,
    SyncReport, TransferEngine, build_plan, execute_plan, sync_once, watch,
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
    #[arg(long, conflicts_with = "both")]
    delete: bool,
    /// Two-way: the newer copy of each file wins, in either direction; never
    /// deletes. Files changed on both sides are reported, not copied.
    #[arg(long, conflicts_with = "watch")]
    both: bool,
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
    /// Print a JSON summary (one object per sync round with --watch).
    #[arg(long)]
    json: bool,
    /// Source directory: local path or HOST:PATH.
    src: String,
    /// Destination directory: local path or HOST:PATH.
    dst: String,
}

fn report_json(r: &SyncReport, dry_run: bool, src: &str, dst: &str) -> serde_json::Value {
    serde_json::json!({
        "source": src,
        "destination": dst,
        "dry_run": dry_run,
        "copied": r.copied,
        "deleted": r.deleted,
        "failed": r.failed.iter().map(|(path, error)| serde_json::json!({"path": path, "error": error})).collect::<Vec<_>>(),
        "bytes": r.bytes,
        "up_to_date": r.is_noop(),
    })
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

fn print_plan(plan: &SyncPlan) {
    for it in &plan.items {
        let path = if it.is_dir {
            format!("{}/", it.rel_path)
        } else {
            it.rel_path.clone()
        };
        let what = match it.kind {
            ActionKind::Conflict => "CONFLICT",
            k => k.label(),
        };
        println!("{what:<14} {path}  ({})", it.reason.describe());
    }
}

/// `--both`: plan, show, run.
async fn run_both(
    a: &SyncArgs,
    local: Arc<dyn RemoteFs>,
    local_root: String,
    remote: Arc<dyn RemoteFs>,
    remote_root: String,
) -> Result<i32> {
    let plan = build_plan(
        local.as_ref(),
        &local_root,
        remote.as_ref(),
        &remote_root,
        PlanOptions {
            direction: SyncDirection::Both,
            delete: false,
            excludes: a.exclude.clone(),
        },
    )
    .await?;
    let conflicts = plan
        .items
        .iter()
        .filter(|i| i.kind == ActionKind::Conflict)
        .count();
    if a.dry_run {
        if a.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else if plan.is_empty() {
            eprintln!("up to date");
        } else {
            print_plan(&plan);
        }
        return Ok(0);
    }
    let engine = TransferEngine::new(EngineConfig {
        max_concurrent: a.parallel.max(1),
        rate_limit: a.limit.map(|k| k * 1024),
        ..Default::default()
    });
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop.cancel();
    });
    let report = execute_plan(&engine, local, remote, &plan, |_, _| true, &cancel).await;
    if a.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "local": local_root,
                "remote": remote_root,
                "report": report,
                "conflicts": plan.items.iter().filter(|i| i.kind == ActionKind::Conflict).map(|i| &i.rel_path).collect::<Vec<_>>(),
            }))?
        );
    } else {
        for p in &report.uploaded {
            println!("uploaded     {p}");
        }
        for p in &report.downloaded {
            println!("downloaded   {p}");
        }
        for it in plan.items.iter().filter(|i| i.kind == ActionKind::Conflict) {
            eprintln!("CONFLICT     {}: {}", it.rel_path, it.reason.describe());
        }
        for (p, e) in &report.failed {
            eprintln!("FAILED       {p}: {e}");
        }
        eprintln!("{} ({})", report.summary(), human(report.bytes));
    }
    Ok(
        if report.failed.is_empty() && conflicts == 0 && !report.cancelled {
            0
        } else {
            1
        },
    )
}

pub async fn run(app: &App, a: SyncArgs) -> Result<i32> {
    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
    if a.both {
        let (l, host, path) = match (parse_loc(&a.src), parse_loc(&a.dst)) {
            (Loc::Local(l), Loc::Remote { host, path })
            | (Loc::Remote { host, path }, Loc::Local(l)) => (l, host, path),
            _ => bail!("exactly one side must be remote (HOST:PATH)"),
        };
        let remote = open_remote(app, &host, &a.conn).await?;
        let remote_root = if path.is_empty() { ".".into() } else { path };
        let remote_root = remote
            .canonicalize(&remote_root)
            .await
            .unwrap_or(remote_root);
        return run_both(&a, local, absolute_local(&l)?, remote, remote_root).await;
    }
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
        if a.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report_json(&r, a.dry_run, &src_root, &dst_root))?
            );
        } else {
            print_report(&r, a.dry_run);
        }
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
            Ok(report) if a.json => {
                println!("{}", report_json(report, false, &src_root, &dst_root))
            }
            Ok(report) => print_report(report, false),
            Err(e) => eprintln!("sync error (will retry): {e}"),
        },
    )
    .await;
    Ok(0)
}
