//! Settings sync through a folder that OneDrive, Dropbox, iCloud Drive,
//! Syncthing or git already keeps in step between computers.
//!
//! Everything goes into one encrypted file, `<folder>/hostwick-sync.hwsync`:
//! all saved hosts, deletions (tombstones with their time) and the app's own
//! settings as an opaque JSON string. It is sealed with a sync passphrase
//! (Argon2id + XChaCha20-Poly1305, the vault's construction, header
//! authenticated). Vault secrets are never included: hosts only refer to
//! credentials by id.
//!
//! Merging is per host id, newest `updated_ms` wins (the store stamps it on
//! every save); a deletion wins over any edit made before it. When two
//! computers write at once, the cloud client keeps both as a "conflicted
//! copy"; [`SettingsSync::sync`] merges every such sibling
//! (`hostwick-sync (… conflicted copy …).hwsync`,
//! `hostwick-sync.sync-conflict-….hwsync`, `hostwick-sync-PCNAME.hwsync`,
//! `hostwick-sync (1).hwsync`, …) into the result and then removes them.
//! Files are replaced atomically (temporary file + rename).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use hostwick_core::{HostProfile, ProfileId, ProfileStore, SecretString, StoreError};
pub use hostwick_vault::KdfParams;
use hostwick_vault::VaultError;
use hostwick_vault::sealed::{self, SealKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SqliteStore;
use crate::sqlite::now_ms;

/// The sync file's name inside the chosen folder.
pub const SYNC_FILE_NAME: &str = "hostwick-sync.hwsync";
const STEM: &str = "hostwick-sync";
const EXTENSION: &str = ".hwsync";
const PURPOSE: &str = "settings-sync";
const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(
        "wrong sync passphrase for {0} (or the file was changed by something other than Hostwick)"
    )]
    WrongPassphrase(PathBuf),
    #[error("sync folder {0} does not exist; pick the folder your cloud drive keeps in sync")]
    NoFolder(PathBuf),
    #[error("nothing to pull: {0} does not exist yet (run a sync or push first)")]
    NothingToPull(PathBuf),
    #[error("{path} is not a Hostwick sync file: {reason}")]
    Corrupt { path: PathBuf, reason: String },
    #[error("{0} was written by a newer Hostwick; update Hostwick on this computer")]
    TooNew(PathBuf),
    #[error("{0} keeps changing while Hostwick writes it; try again in a moment")]
    Busy(PathBuf),
    #[error("encryption: {0}")]
    Crypto(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> SyncError + '_ {
    move |source| SyncError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SettingsBlob {
    json: String,
    updated_ms: u64,
}

/// What a sync file (or this computer) holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct State {
    /// Profile id → profile JSON (kept as JSON so fields added by a newer
    /// Hostwick survive a round trip through an older one).
    #[serde(default)]
    profiles: BTreeMap<String, Value>,
    /// Deleted profile id → deletion time (Unix ms).
    #[serde(default)]
    tombstones: BTreeMap<String, u64>,
    #[serde(default)]
    app_settings: Option<SettingsBlob>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    format: u32,
    #[serde(default)]
    written_by: String,
    #[serde(default)]
    written_ms: u64,
    #[serde(flatten)]
    state: State,
}

fn profile_ms(v: &Value) -> u64 {
    v.get("updated_ms").and_then(Value::as_u64).unwrap_or(0)
}

/// The newer of two versions; ties are broken by content so every computer
/// picks the same one.
fn newer<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    match profile_ms(a).cmp(&profile_ms(b)) {
        std::cmp::Ordering::Greater => a,
        std::cmp::Ordering::Less => b,
        std::cmp::Ordering::Equal => {
            if a.to_string().cmp(&b.to_string()).is_ge() {
                a
            } else {
                b
            }
        }
    }
}

fn merge(a: &State, b: &State) -> State {
    let mut tombstones = a.tombstones.clone();
    for (id, ms) in &b.tombstones {
        let t = tombstones.entry(id.clone()).or_insert(0);
        *t = (*t).max(*ms);
    }
    let ids: BTreeSet<&String> = a.profiles.keys().chain(b.profiles.keys()).collect();
    let mut profiles = BTreeMap::new();
    for id in ids {
        let v = match (a.profiles.get(id), b.profiles.get(id)) {
            (Some(x), Some(y)) => newer(x, y),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => continue,
        };
        // A deletion beats every edit made up to that moment.
        if tombstones.get(id).is_none_or(|t| profile_ms(v) > *t) {
            profiles.insert(id.clone(), v.clone());
        }
    }
    // An edit after a deletion brings the host back; forget the deletion.
    tombstones.retain(|id, _| !profiles.contains_key(id));
    let app_settings = match (&a.app_settings, &b.app_settings) {
        (Some(x), Some(y)) => Some(
            if (x.updated_ms, &x.json) >= (y.updated_ms, &y.json) {
                x
            } else {
                y
            }
            .clone(),
        ),
        (x, y) => x.clone().or_else(|| y.clone()),
    };
    State {
        profiles,
        tombstones,
        app_settings,
    }
}

/// How a sync run went, for display and for applying settings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    /// `sync`, `push` or `pull`.
    pub mode: &'static str,
    pub file: PathBuf,
    /// Hosts added / changed / removed on this computer (as `folder/name`).
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    /// New app settings JSON received from another computer: apply it.
    pub app_settings: Option<String>,
    /// The sync file was (re)written.
    pub uploaded: bool,
    /// Hosts in the synced state afterwards.
    pub profiles: usize,
    /// Conflicted copies that were merged and removed.
    pub merged_copies: Vec<PathBuf>,
    /// Conflicted copies left alone because they could not be read (another
    /// passphrase, damaged); with the reason.
    pub skipped_copies: Vec<(PathBuf, String)>,
}

impl SyncReport {
    /// Nothing changed on this computer.
    pub fn local_unchanged(&self) -> bool {
        self.added.is_empty()
            && self.updated.is_empty()
            && self.deleted.is_empty()
            && self.app_settings.is_none()
    }
}

/// Settings sync for one store and folder.
///
/// The desktop app records its settings with
/// [`SqliteStore::set_app_settings`] whenever they change, calls
/// [`sync`](Self::sync) at start-up, after editing hosts or settings, and
/// every few minutes, and applies [`SyncReport::app_settings`] when present.
/// All calls do blocking file I/O and Argon2 (≈0.5 s): run them off the UI
/// thread.
pub struct SettingsSync<'a> {
    store: &'a SqliteStore,
    folder: PathBuf,
    passphrase: SecretString,
    kdf: KdfParams,
    device: String,
}

struct Remote {
    /// The main file's bytes as read (None: it does not exist).
    main_bytes: Option<Vec<u8>>,
    /// Main file and readable conflicted copies, merged.
    state: State,
    /// Main file's state alone.
    main_state: Option<State>,
    key: Option<SealKey>,
    copies: Vec<PathBuf>,
    skipped: Vec<(PathBuf, String)>,
}

impl<'a> SettingsSync<'a> {
    pub fn new(
        store: &'a SqliteStore,
        folder: impl Into<PathBuf>,
        passphrase: SecretString,
    ) -> Self {
        Self {
            store,
            folder: folder.into(),
            passphrase,
            kdf: KdfParams::default(),
            device: device_name(),
        }
    }

    /// Argon2id cost for newly created sync files (tests use a cheap one).
    pub fn with_kdf(mut self, kdf: KdfParams) -> Self {
        self.kdf = kdf;
        self
    }

    /// Name recorded as the last writer (defaults to the computer name).
    pub fn with_device_name(mut self, name: impl Into<String>) -> Self {
        self.device = name.into();
        self
    }

    /// `<folder>/hostwick-sync.hwsync`
    pub fn file(&self) -> PathBuf {
        self.folder.join(SYNC_FILE_NAME)
    }

    /// Two-way merge: newer edits and deletions from either side win; this
    /// computer and the sync file both end up with the merged result.
    pub fn sync(&self) -> Result<SyncReport, SyncError> {
        self.check_folder()?;
        let mut cache = None;
        let mut report = self.report("sync");
        for _ in 0..3 {
            let remote = self.read_remote(cache.take())?;
            let local = self.local_state()?;
            let merged = merge(&local, &remote.state);
            self.apply(&local, &merged, &mut report)?;
            report.profiles = merged.profiles.len();
            report.skipped_copies = remote.skipped.clone();
            let needs_write =
                remote.main_state.as_ref() != Some(&merged) || !remote.copies.is_empty();
            if !needs_write {
                return Ok(report);
            }
            match self.write(&merged, remote.key.as_ref(), remote.main_bytes.as_deref())? {
                Some(_) => {
                    report.uploaded = true;
                    report.merged_copies = self.remove_copies(&remote.copies);
                    return Ok(report);
                }
                // The file changed under us (the cloud client delivered a new
                // version): merge again.
                None => cache = remote.key,
            }
        }
        Err(SyncError::Busy(self.file()))
    }

    /// Make the sync file match this computer, replacing what it held
    /// (including changes other computers made since they last synced).
    /// Also the way to change the sync passphrase. Readable conflicted
    /// copies are removed.
    pub fn push(&self) -> Result<SyncReport, SyncError> {
        self.check_folder()?;
        let local = self.local_state()?;
        let mut report = self.report("push");
        let (copies, skipped) = self.readable_copies(None);
        // Overwrite unconditionally, whatever is there now; a fresh salt, so
        // this also re-keys the file under a new passphrase.
        self.write(&local, None, None)?;
        report.uploaded = true;
        report.profiles = local.profiles.len();
        report.merged_copies = self.remove_copies(&copies);
        report.skipped_copies = skipped;
        Ok(report)
    }

    /// Make this computer match the sync file (and any readable conflicted
    /// copies): hosts only on this computer are removed, settings replaced.
    /// The sync file is not changed.
    pub fn pull(&self) -> Result<SyncReport, SyncError> {
        self.check_folder()?;
        let remote = self.read_remote(None)?;
        if remote.main_bytes.is_none() && remote.copies.is_empty() {
            return Err(SyncError::NothingToPull(self.file()));
        }
        let local = self.local_state()?;
        let mut report = self.report("pull");
        let mut target = remote.state.clone();
        // Keep local settings if the file has none.
        if target.app_settings.is_none() {
            target.app_settings.clone_from(&local.app_settings);
        }
        self.apply(&local, &target, &mut report)?;
        report.profiles = target.profiles.len();
        report.skipped_copies = remote.skipped;
        Ok(report)
    }

    fn report(&self, mode: &'static str) -> SyncReport {
        SyncReport {
            mode,
            file: self.file(),
            ..Default::default()
        }
    }

    fn check_folder(&self) -> Result<(), SyncError> {
        if self.folder.is_dir() {
            Ok(())
        } else {
            Err(SyncError::NoFolder(self.folder.clone()))
        }
    }

    fn local_state(&self) -> Result<State, SyncError> {
        let mut profiles = BTreeMap::new();
        for p in self.store.list()? {
            let v = serde_json::to_value(&p).map_err(|e| StoreError::Corrupt(e.to_string()))?;
            profiles.insert(p.id.to_string(), v);
        }
        let tombstones = self
            .store
            .tombstones()?
            .into_iter()
            .map(|(id, ms)| (id.to_string(), ms))
            .collect();
        let app_settings = self
            .store
            .app_settings()?
            .map(|(json, updated_ms)| SettingsBlob { json, updated_ms });
        Ok(State {
            profiles,
            tombstones,
            app_settings,
        })
    }

    /// Bring the store to `target`, recording what changed.
    fn apply(
        &self,
        local: &State,
        target: &State,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        let mut upserts = Vec::new();
        for (id, v) in &target.profiles {
            let incoming: HostProfile = match serde_json::from_value(v.clone()) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(%id, error = %e, "skipping unreadable synced host");
                    continue;
                }
            };
            let current: Option<HostProfile> = local
                .profiles
                .get(id)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            match current {
                Some(c) if c == incoming => {}
                Some(_) => {
                    report.updated.push(incoming.path());
                    upserts.push(incoming);
                }
                None => {
                    report.added.push(incoming.path());
                    upserts.push(incoming);
                }
            }
        }
        let mut deletes = Vec::new();
        for (id, v) in &local.profiles {
            if !target.profiles.contains_key(id)
                && let Ok(pid) = id.parse::<ProfileId>()
            {
                let name = serde_json::from_value::<HostProfile>(v.clone())
                    .map(|p| p.path())
                    .unwrap_or_else(|_| id.clone());
                report.deleted.push(name);
                deletes.push(pid);
            }
        }
        let tombstones: Vec<(ProfileId, u64)> = target
            .tombstones
            .iter()
            .filter_map(|(id, ms)| Some((id.parse().ok()?, *ms)))
            .collect();
        let settings = match (&target.app_settings, &local.app_settings) {
            (Some(t), l) if l.as_ref() != Some(t) => {
                if l.as_ref().map(|l| &l.json) != Some(&t.json) {
                    report.app_settings = Some(t.json.clone());
                }
                Some((t.json.as_str(), t.updated_ms))
            }
            _ => None,
        };
        let tombstones_changed = local.tombstones != target.tombstones;
        if !upserts.is_empty() || !deletes.is_empty() || settings.is_some() || tombstones_changed {
            self.store
                .apply_sync(&upserts, &deletes, &tombstones, settings)?;
        }
        Ok(())
    }

    fn open_file(
        &self,
        path: &Path,
        bytes: &[u8],
        cache: Option<&SealKey>,
    ) -> Result<(State, SealKey), SyncError> {
        let (plain, key) =
            sealed::open_cached(bytes, &self.passphrase, PURPOSE, cache).map_err(|e| match e {
                VaultError::WrongPassword => SyncError::WrongPassphrase(path.to_path_buf()),
                VaultError::UnsupportedVersion(_) => SyncError::TooNew(path.to_path_buf()),
                other => SyncError::Corrupt {
                    path: path.to_path_buf(),
                    reason: other.to_string(),
                },
            })?;
        let payload: Payload = serde_json::from_slice(&plain).map_err(|e| SyncError::Corrupt {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        if payload.format > FORMAT {
            return Err(SyncError::TooNew(path.to_path_buf()));
        }
        Ok((payload.state, key))
    }

    fn read_remote(&self, cache: Option<SealKey>) -> Result<Remote, SyncError> {
        let main_path = self.file();
        let main_bytes = match std::fs::read(&main_path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err(&main_path)(e)),
        };
        let (main_state, mut key) = match &main_bytes {
            Some(b) => {
                let (s, k) = self.open_file(&main_path, b, cache.as_ref())?;
                (Some(s), Some(k))
            }
            None => (None, cache),
        };
        let mut state = main_state.clone().unwrap_or_default();
        let mut copies = Vec::new();
        let mut skipped = Vec::new();
        for path in self.conflict_copies() {
            let res = std::fs::read(&path)
                .map_err(io_err(&path))
                .and_then(|b| self.open_file(&path, &b, key.as_ref()));
            match res {
                Ok((s, k)) => {
                    state = merge(&state, &s);
                    key.get_or_insert(k);
                    copies.push(path);
                }
                Err(e) => skipped.push((path, e.to_string())),
            }
        }
        Ok(Remote {
            main_bytes,
            state,
            main_state,
            key,
            copies,
            skipped,
        })
    }

    /// Conflicted copies that open with our passphrase, and the rest.
    #[allow(clippy::type_complexity)]
    fn readable_copies(&self, mut key: Option<SealKey>) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
        let (mut ok, mut bad) = (Vec::new(), Vec::new());
        for path in self.conflict_copies() {
            match std::fs::read(&path)
                .map_err(io_err(&path))
                .and_then(|b| self.open_file(&path, &b, key.as_ref()))
            {
                Ok((_, k)) => {
                    key.get_or_insert(k);
                    ok.push(path);
                }
                Err(e) => bad.push((path, e.to_string())),
            }
        }
        (ok, bad)
    }

    fn conflict_copies(&self) -> Vec<PathBuf> {
        let Ok(dir) = std::fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = dir
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| is_conflict_copy(&e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        out.sort();
        out
    }

    fn remove_copies(&self, copies: &[PathBuf]) -> Vec<PathBuf> {
        copies
            .iter()
            .filter(|p| match std::fs::remove_file(p) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(path = %p.display(), error = %e, "cannot remove merged sync copy");
                    false
                }
            })
            .cloned()
            .collect()
    }

    /// Seal and atomically replace the sync file. With `expect`, the file
    /// must still hold exactly those bytes (else `Ok(None)`: re-merge).
    fn write(
        &self,
        state: &State,
        key: Option<&SealKey>,
        expect: Option<&[u8]>,
    ) -> Result<Option<()>, SyncError> {
        let payload = Payload {
            format: FORMAT,
            written_by: self.device.clone(),
            written_ms: now_ms(),
            state: state.clone(),
        };
        let plain = zeroize::Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|e| SyncError::Crypto(e.to_string()))?,
        );
        let fresh;
        let key = match key {
            Some(k) => k,
            None => {
                fresh = SealKey::new(&self.passphrase, self.kdf)
                    .map_err(|e| SyncError::Crypto(e.to_string()))?;
                &fresh
            }
        };
        let sealed = key
            .seal(&plain, PURPOSE)
            .map_err(|e| SyncError::Crypto(e.to_string()))?;

        let path = self.file();
        let tmp = self.folder.join(format!(
            ".{SYNC_FILE_NAME}.{}-{}.tmp",
            std::process::id(),
            now_ms()
        ));
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&sealed)?;
            f.sync_all()
        };
        if let Err(e) = write() {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err(&tmp)(e));
        }
        if let Some(expected) = expect {
            let now = std::fs::read(&path).ok();
            if now.as_deref() != Some(expected) {
                let _ = std::fs::remove_file(&tmp);
                return Ok(None);
            }
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err(&path)(e));
        }
        Ok(Some(()))
    }
}

/// `hostwick-sync (… conflicted copy …).hwsync` (Dropbox, Nextcloud),
/// `hostwick-sync.sync-conflict-….hwsync` (Syncthing),
/// `hostwick-sync-PCNAME.hwsync` (OneDrive), `hostwick-sync (1).hwsync`
/// (Google Drive), `hostwick-sync 2.hwsync` (iCloud).
pub fn is_conflict_copy(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower != SYNC_FILE_NAME && lower.starts_with(STEM) && lower.ends_with(EXTENSION)
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests;
