//! Diagnostic logging: stderr plus a daily-rotated file in the app's log
//! folder, with a level that the Settings dialog can change at runtime.

use std::path::Path;
use std::sync::OnceLock;

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

static RELOAD: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();
static GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

pub const LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

/// `HOSTWICK_LOG` (an `EnvFilter` string) wins over the saved level.
fn filter_for(level: &str) -> EnvFilter {
    if let Ok(f) = EnvFilter::try_from_env("HOSTWICK_LOG") {
        return f;
    }
    let level = if LEVELS.contains(&level) {
        level
    } else {
        "info"
    };
    EnvFilter::new(format!("warn,hostwick={level},hostwick_desktop={level}"))
}

pub fn init(dir: &Path, level: &str) {
    let (filter, handle) = reload::Layer::new(filter_for(level));
    let file_layer = std::fs::create_dir_all(dir).ok().map(|()| {
        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("hostwick")
            .filename_suffix("log")
            .max_log_files(14)
            .build(dir);
        appender.map(|a| {
            let (writer, guard) = tracing_appender::non_blocking(a);
            let _ = GUARD.set(guard);
            fmt::layer().with_ansi(false).with_writer(writer)
        })
    });
    let file_layer = file_layer.and_then(Result::ok);
    let ok = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .try_init()
        .is_ok();
    if ok {
        let _ = RELOAD.set(handle);
    }
}

pub fn set_level(level: &str) {
    if let Some(h) = RELOAD.get() {
        let _ = h.reload(filter_for(level));
    }
    tracing::info!(level, "log level changed");
}
