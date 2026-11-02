//! `hostwick settings-sync push|pull|sync`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use hostwick_core::{ExposeSecret, SecretString};
use hostwick_store::{SettingsSync, SyncReport};

use crate::app::App;

const FOLDER_SETTING: &str = "sync.folder";
pub const PASSPHRASE_ENV: &str = "HOSTWICK_SYNC_PASSPHRASE";

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Action {
    /// Merge both ways (use this normally).
    Sync,
    /// Replace the sync file with this computer's hosts and settings (also
    /// changes the sync passphrase).
    Push,
    /// Replace this computer's hosts and settings with the sync file's.
    Pull,
}

#[derive(Args)]
pub struct SettingsSyncArgs {
    #[arg(value_enum)]
    action: Action,
    /// A folder kept in sync by OneDrive, Dropbox, iCloud Drive, Syncthing
    /// or git. Remembered after the first successful use.
    #[arg(long)]
    folder: Option<PathBuf>,
    /// Machine-readable summary.
    #[arg(long)]
    json: bool,
}

fn passphrase(app: &App, confirm: bool) -> Result<SecretString> {
    if let Some(p) = std::env::var_os(PASSPHRASE_ENV) {
        let p = p
            .into_string()
            .map_err(|_| anyhow::anyhow!("{PASSPHRASE_ENV} is not valid UTF-8"))?;
        if p.is_empty() {
            bail!("{PASSPHRASE_ENV} is empty");
        }
        return Ok(SecretString::from(p));
    }
    if app.policy.batch {
        bail!("--batch: set {PASSPHRASE_ENV} to the sync passphrase");
    }
    if !confirm {
        return crate::tty::read_secret("Sync passphrase: ");
    }
    eprintln!("Choose a sync passphrase; every computer that syncs needs it.");
    let p = crate::tty::read_secret("New sync passphrase: ")?;
    if p.expose_secret().chars().count() < 8 {
        bail!("use at least 8 characters");
    }
    let again = crate::tty::read_secret("Repeat: ")?;
    if p.expose_secret() != again.expose_secret() {
        bail!("passphrases did not match");
    }
    Ok(p)
}

pub fn run(app: &App, a: SettingsSyncArgs) -> Result<i32> {
    let folder = match a.folder {
        Some(f) => f,
        None => app
            .store
            .setting(FOLDER_SETTING)?
            .map(PathBuf::from)
            .context("give the sync folder with --folder (it is remembered afterwards)")?,
    };
    let folder = std::path::absolute(&folder).unwrap_or(folder);
    let file_exists = folder.join(hostwick_store::SYNC_FILE_NAME).exists();
    // A new file (or a re-keying push) fixes the passphrase for every
    // computer: ask twice.
    let confirm = !file_exists || a.action == Action::Push;
    let pass = passphrase(app, confirm)?;
    let sync = SettingsSync::new(&app.store, &folder, pass);
    let report = match a.action {
        Action::Sync => sync.sync(),
        Action::Push => sync.push(),
        Action::Pull => sync.pull(),
    }?;
    app.store
        .set_setting(FOLDER_SETTING, &folder.to_string_lossy())?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(0)
}

fn print_report(r: &SyncReport) {
    println!("{}: {}", r.mode, r.file.display());
    for n in &r.added {
        println!("  + {n}");
    }
    for n in &r.updated {
        println!("  ~ {n}");
    }
    for n in &r.deleted {
        println!("  - {n}");
    }
    if r.app_settings.is_some() {
        println!("  settings updated from another computer");
    }
    for p in &r.merged_copies {
        println!("  merged and removed {}", p.display());
    }
    for (p, why) in &r.skipped_copies {
        eprintln!("warning: left {} alone: {why}", p.display());
    }
    let here = if r.local_unchanged() {
        "no changes here".to_owned()
    } else {
        format!(
            "{} added, {} updated, {} deleted here",
            r.added.len(),
            r.updated.len(),
            r.deleted.len()
        )
    };
    println!(
        "{} host(s) in sync; {here}; sync file {}",
        r.profiles,
        if r.uploaded { "updated" } else { "unchanged" }
    );
}
