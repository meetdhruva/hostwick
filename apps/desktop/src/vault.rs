//! Saved passwords for the desktop app: the shared credential vault (the
//! same file `hostwick vault` uses), kept unlocked for the app session.
//!
//! Unlocking and creating run Argon2 (≈0.3 s): callers run them off the UI
//! thread. Secrets are never logged; ids name what they are for.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use gpui_kit::SharedString;
use hostwick_core::{AuthMethod, ExposeSecret, HostProfile, ProfileId, SecretString};
use hostwick_ssh::Credentials;
use hostwick_vault::{Vault, VaultError};

/// What a saved secret is for, and so its credential id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CredKind {
    /// The host's login password.
    Password,
    /// Passphrase of a key file (shared by every host using that key).
    KeyPassphrase(PathBuf),
    /// sudo password for the file browser.
    Sudo,
    /// Password for the host's SOCKS/HTTP proxy.
    Proxy,
}

/// The id a newly saved secret gets. Stable across renames: hosts by their
/// id, keys by their path (so one saved passphrase serves every host that
/// uses the key).
pub fn credential_id(profile: ProfileId, kind: &CredKind) -> String {
    match kind {
        CredKind::Password => format!("host:{}:password", profile.0),
        CredKind::Sudo => format!("host:{}:sudo", profile.0),
        CredKind::Proxy => format!("host:{}:proxy", profile.0),
        CredKind::KeyPassphrase(path) => format!("key:{}", path.display()),
    }
}

/// Whether credential `id` is one this app minted for profile `pid`
/// (`host:<pid>:…`), so it may be replaced or deleted on its behalf.
pub fn owned_by(id: &str, pid: ProfileId) -> bool {
    id.strip_prefix("host:")
        .and_then(|r| r.strip_prefix(&pid.0.to_string()))
        .is_some_and(|r| r.starts_with(':'))
}

/// Whether a remembered secret for profile `pid` may be written into the
/// existing credential `id`: its own ids, and key passphrases (one per key
/// file, shared on purpose). Anything else — another host's id after
/// "Duplicate", or an id shared through the CLI — gets a fresh id instead.
pub fn reusable_for(id: &str, pid: ProfileId) -> bool {
    owned_by(id, pid) || id.starts_with("key:")
}

/// Forget the `host:` credential ids that belong to another profile (a
/// duplicated host keeps its source's references otherwise, and would
/// overwrite or delete the source's saved passwords).
pub fn drop_foreign_host_ids(p: &mut HostProfile) {
    for (kind, id) in profile_ids(p) {
        if id.starts_with("host:") && !owned_by(&id, p.id) {
            assign_id(p, &kind, None);
        }
    }
}

/// The credential id the profile already uses for `kind`, if any.
pub fn existing_id(p: &HostProfile, kind: &CredKind) -> Option<String> {
    match kind {
        CredKind::Password => p.auth.iter().find_map(|a| match a {
            AuthMethod::Password { credential } => credential.clone(),
            _ => None,
        }),
        CredKind::KeyPassphrase(path) => p.auth.iter().find_map(|a| match a {
            AuthMethod::KeyFile {
                path: k,
                passphrase_credential,
            } if k == path => passphrase_credential.clone(),
            _ => None,
        }),
        CredKind::Sudo => p.sudo.password_credential.clone(),
        CredKind::Proxy => p.proxy.as_ref().and_then(|x| x.password_credential.clone()),
    }
}

/// Point the profile's `kind` at credential `id`. Returns false when the
/// profile has nowhere to put it (e.g. the key file was removed from it).
pub fn assign_id(p: &mut HostProfile, kind: &CredKind, id: Option<String>) -> bool {
    match kind {
        CredKind::Password => {
            if let Some(AuthMethod::Password { credential }) = p
                .auth
                .iter_mut()
                .find(|a| matches!(a, AuthMethod::Password { .. }))
            {
                *credential = id;
                return true;
            }
            // Password auth was not listed: add it before keyboard-interactive.
            if id.is_some() {
                let at = p
                    .auth
                    .iter()
                    .position(|a| matches!(a, AuthMethod::KeyboardInteractive))
                    .unwrap_or(p.auth.len());
                p.auth.insert(at, AuthMethod::Password { credential: id });
            }
            true
        }
        CredKind::KeyPassphrase(path) => {
            for a in &mut p.auth {
                if let AuthMethod::KeyFile {
                    path: k,
                    passphrase_credential,
                } = a
                    && k == path
                {
                    *passphrase_credential = id;
                    return true;
                }
            }
            false
        }
        CredKind::Sudo => {
            p.sudo.password_credential = id;
            true
        }
        CredKind::Proxy => match p.proxy.as_mut() {
            Some(x) => {
                x.password_credential = id;
                true
            }
            None => false,
        },
    }
}

/// Every credential id the profile refers to, with what it is for.
pub fn profile_ids(p: &HostProfile) -> Vec<(CredKind, String)> {
    let mut out = Vec::new();
    for a in &p.auth {
        match a {
            AuthMethod::Password {
                credential: Some(id),
            } => out.push((CredKind::Password, id.clone())),
            AuthMethod::KeyFile {
                path,
                passphrase_credential: Some(id),
            } => out.push((CredKind::KeyPassphrase(path.clone()), id.clone())),
            _ => {}
        }
    }
    if let Some(id) = &p.sudo.password_credential {
        out.push((CredKind::Sudo, id.clone()));
    }
    if let Some(id) = p.proxy.as_ref().and_then(|x| x.password_credential.clone()) {
        out.push((CredKind::Proxy, id));
    }
    out
}

/// A human name for a saved credential: which hosts use it and for what.
pub fn describe(id: &str, profiles: &[HostProfile]) -> String {
    if id == SYNC_PASSPHRASE_ID {
        return t!("cred-sync-passphrase").into();
    }
    let mut users = Vec::new();
    let mut what = None;
    for p in profiles {
        for (kind, pid) in profile_ids(p) {
            if pid == id {
                users.push(p.path());
                what.get_or_insert(match kind {
                    CredKind::Password => t!("cred-login-password").to_string(),
                    CredKind::Sudo => t!("cred-sudo-password").to_string(),
                    CredKind::Proxy => t!("cred-proxy-password").to_string(),
                    CredKind::KeyPassphrase(path) => t!(
                        "cred-key-passphrase",
                        file = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string())
                    )
                    .to_string(),
                });
            }
        }
    }
    users.sort();
    users.dedup();
    match (what, users.len()) {
        (Some(w), 1) => format!("{} — {w}", users[0]),
        (Some(w), n) if n > 1 => format!("{w} ({})", users.join(", ")),
        _ => match id.strip_prefix("key:") {
            Some(path) => t!("cred-key-unused", path = path).into(),
            None if id.starts_with("host:") => t!("cred-host-deleted").into(),
            None => id.to_owned(),
        },
    }
}

/// Vault id of the settings-sync passphrase.
pub const SYNC_PASSPHRASE_ID: &str = "hostwick:settings-sync";

/// Rough strength of a new master password, for the hint under the field.
pub fn strength_hint(pw: &str) -> (u8, SharedString) {
    let len = pw.chars().count();
    if len == 0 {
        return (0, SharedString::default());
    }
    let classes = [
        pw.chars().any(|c| c.is_ascii_lowercase()),
        pw.chars().any(|c| c.is_ascii_uppercase()),
        pw.chars().any(|c| c.is_ascii_digit()),
        pw.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    let words = pw.split_whitespace().count();
    match (len, classes, words) {
        (0..=7, _, _) => (1, t!("strength-too-short")),
        (_, _, w) if w >= 4 && len >= 16 => (3, t!("strength-strong-passphrase")),
        (8..=11, 0..=2, _) => (1, t!("strength-weak")),
        (8..=11, _, _) | (12..=15, 0..=2, _) => (2, t!("strength-fair")),
        _ => (3, t!("strength-strong")),
    }
}

struct Open {
    vault: Vault,
    /// Kept to re-read the file when the CLI changed it meanwhile.
    master: SecretString,
    loaded: Option<SystemTime>,
}

/// The vault for the app session.
pub struct VaultManager {
    path: PathBuf,
    open: Mutex<Option<Open>>,
    last_used: Mutex<Instant>,
    /// Ids whose secret was rejected this session: not used again until
    /// replaced, so a stale saved password prompts instead of failing.
    stale: Mutex<HashSet<String>>,
    /// The person said "not now" to unlocking; don't ask on every connect.
    declined: Mutex<bool>,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl VaultManager {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            open: Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
            stale: Mutex::default(),
            declined: Mutex::new(false),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        Vault::exists(&self.path)
    }

    pub fn is_unlocked(&self) -> bool {
        self.open.lock().map(|o| o.is_some()).unwrap_or(false)
    }

    fn touch(&self) {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
    }

    /// Unlock with the master password. Blocking (Argon2).
    pub fn unlock(&self, master: SecretString) -> Result<(), VaultError> {
        let vault = Vault::unlock(&self.path, &master)?;
        self.install(vault, master);
        Ok(())
    }

    /// Create the vault. Blocking (Argon2).
    pub fn create(&self, master: SecretString) -> Result<(), VaultError> {
        let vault = Vault::create(&self.path, &master)?;
        self.install(vault, master);
        Ok(())
    }

    fn install(&self, vault: Vault, master: SecretString) {
        let loaded = mtime(&self.path);
        if let Ok(mut o) = self.open.lock() {
            *o = Some(Open {
                vault,
                master,
                loaded,
            });
        }
        if let Ok(mut d) = self.declined.lock() {
            *d = false;
        }
        self.touch();
        tracing::info!("vault unlocked");
    }

    pub fn lock(&self) {
        if let Ok(mut o) = self.open.lock()
            && o.take().is_some()
        {
            tracing::info!("vault locked");
        }
    }

    /// Lock when unused for `minutes` (0 = never).
    pub fn lock_if_idle(&self, minutes: u32) -> bool {
        if minutes == 0 || !self.is_unlocked() {
            return false;
        }
        let idle = self
            .last_used
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default();
        if idle.as_secs() >= u64::from(minutes) * 60 {
            self.lock();
            return true;
        }
        false
    }

    pub fn set_declined(&self, v: bool) {
        if let Ok(mut d) = self.declined.lock() {
            *d = v;
        }
    }

    pub fn declined(&self) -> bool {
        self.declined.lock().map(|d| *d).unwrap_or(false)
    }

    /// Run `f` on the open vault, first re-reading the file if something
    /// else (the CLI) changed it. If the changed file cannot be read with
    /// our master password (it was changed elsewhere, or the file is
    /// damaged), the vault is locked instead: writing the stale copy back
    /// would undo the other change. The next use asks to unlock again.
    fn with_open<T>(&self, f: impl FnOnce(&mut Vault) -> T) -> Option<T> {
        let mut guard = self.open.lock().ok()?;
        let open = guard.as_mut()?;
        let now = mtime(&self.path);
        if now != open.loaded {
            match Vault::unlock(&self.path, &open.master) {
                Ok(v) => {
                    open.vault = v;
                    open.loaded = now;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the vault changed and cannot be re-read; locking it");
                    *guard = None;
                    return None;
                }
            }
        }
        let out = f(&mut open.vault);
        open.loaded = mtime(&self.path);
        drop(guard);
        self.touch();
        Some(out)
    }

    pub fn get(&self, id: &str) -> Option<SecretString> {
        if self.is_stale(id) {
            return None;
        }
        self.with_open(|v| v.get(id)).flatten()
    }

    /// Whether the open vault holds `id` (as loaded; cheap enough for
    /// rendering — never re-reads the file).
    pub fn has(&self, id: &str) -> bool {
        self.open
            .lock()
            .ok()
            .and_then(|o| o.as_ref().map(|o| o.vault.get(id).is_some()))
            .unwrap_or(false)
    }

    pub fn set(&self, id: &str, secret: SecretString) -> Result<(), String> {
        let res = self
            .with_open(|v| v.set(id, secret))
            .ok_or_else(|| t!("vault-locked-error").to_string())?
            .map_err(|e| e.to_string());
        if res.is_ok()
            && let Ok(mut s) = self.stale.lock()
        {
            s.remove(id);
        }
        res
    }

    pub fn remove(&self, id: &str) -> Result<bool, String> {
        self.with_open(|v| v.remove(id))
            .ok_or_else(|| t!("vault-locked-error").to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn ids(&self) -> Vec<String> {
        self.with_open(|v| v.ids().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Check the current master password and switch to a new one.
    /// Blocking (Argon2, twice).
    pub fn change_master(&self, old: &SecretString, new: SecretString) -> Result<(), String> {
        let mut fresh = Vault::unlock(&self.path, old).map_err(|e| match e {
            VaultError::WrongPassword => t!("vault-current-wrong").to_string(),
            e => e.to_string(),
        })?;
        fresh.change_master(&new).map_err(|e| e.to_string())?;
        self.install(fresh, new);
        Ok(())
    }

    pub fn mark_stale(&self, id: &str) {
        if let Ok(mut s) = self.stale.lock() {
            s.insert(id.to_owned());
        }
    }

    pub fn is_stale(&self, id: &str) -> bool {
        self.stale.lock().map(|s| s.contains(id)).unwrap_or(false)
    }
}

/// [`Credentials`] backed by the session's vault (nothing while locked).
pub struct VaultCredentials(pub std::sync::Arc<VaultManager>);

impl Credentials for VaultCredentials {
    fn get(&self, id: &str) -> Option<SecretString> {
        self.0.get(id)
    }
}

/// Same secret? (constant-time is not needed: both sides are ours).
pub fn same_secret(a: &SecretString, b: &SecretString) -> bool {
    a.expose_secret() == b.expose_secret()
}

#[cfg(test)]
mod tests {
    use super::{CredKind, assign_id, credential_id, describe, existing_id, strength_hint};
    use super::{SecretString, VaultManager};
    use hostwick_core::{AuthMethod, HostProfile, ProxyKind, ProxySpec};

    #[test]
    fn ids_are_stable_and_assignable() {
        let mut p = HostProfile::new("web", "10.0.0.1");
        let key = std::path::PathBuf::from("/home/me/.ssh/id_ed25519");
        p.auth = vec![
            AuthMethod::Agent,
            AuthMethod::KeyFile {
                path: key.clone(),
                passphrase_credential: None,
            },
            AuthMethod::KeyboardInteractive,
        ];
        let pid = credential_id(p.id, &CredKind::Password);
        assert_eq!(pid, format!("host:{}:password", p.id.0));
        assert_eq!(
            credential_id(p.id, &CredKind::KeyPassphrase(key.clone())),
            "key:/home/me/.ssh/id_ed25519"
        );
        // Password auth missing: added before keyboard-interactive.
        assert!(assign_id(&mut p, &CredKind::Password, Some(pid.clone())));
        assert!(matches!(
            p.auth[2],
            AuthMethod::Password { credential: Some(ref c) } if *c == pid
        ));
        assert_eq!(existing_id(&p, &CredKind::Password), Some(pid.clone()));
        let kid = credential_id(p.id, &CredKind::KeyPassphrase(key.clone()));
        assert!(assign_id(
            &mut p,
            &CredKind::KeyPassphrase(key.clone()),
            Some(kid.clone())
        ));
        assert_eq!(
            existing_id(&p, &CredKind::KeyPassphrase(key)),
            Some(kid.clone())
        );
        // No proxy: nowhere to store a proxy password.
        assert!(!assign_id(&mut p, &CredKind::Proxy, Some("x".into())));
        p.proxy = Some(ProxySpec {
            kind: ProxyKind::Socks5,
            host: "proxy".into(),
            port: 1080,
            user: Some("u".into()),
            password_credential: None,
        });
        assert!(assign_id(&mut p, &CredKind::Proxy, Some("x".into())));
        assert!(assign_id(&mut p, &CredKind::Sudo, Some("s".into())));
        // Forget.
        assert!(assign_id(&mut p, &CredKind::Password, None));
        assert_eq!(existing_id(&p, &CredKind::Password), None);

        let profiles = vec![p.clone()];
        assert_eq!(describe("x", &profiles), "web — proxy password");
        assert_eq!(describe(&kid, &profiles), "web — passphrase for id_ed25519");
        assert_eq!(
            describe("host:123:password", &profiles),
            "Unused (its host was deleted)"
        );
        assert_eq!(describe("prod-db", &profiles), "prod-db");
    }

    /// The CLI changed the master password while the app had the vault
    /// open: the app must lock instead of writing the old copy back.
    #[test]
    fn vault_changed_elsewhere_locks_instead_of_overwriting() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("vault.json");
        let m = VaultManager::new(path.clone());
        m.create(SecretString::from("old master"))
            .unwrap_or_else(|e| panic!("{e}"));
        m.set("a", SecretString::from("1"))
            .unwrap_or_else(|e| panic!("{e}"));
        // `hostwick vault passwd` + `vault set b` in another process.
        let mut other = hostwick_vault::Vault::unlock(&path, &SecretString::from("old master"))
            .unwrap_or_else(|e| panic!("{e}"));
        other
            .change_master(&SecretString::from("new master"))
            .unwrap_or_else(|e| panic!("{e}"));
        other
            .set("b", SecretString::from("2"))
            .unwrap_or_else(|e| panic!("{e}"));
        // Make sure the change is visible even with coarse timestamps.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(later))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.set("c", SecretString::from("3")).is_err());
        assert!(!m.is_unlocked());
        let v = hostwick_vault::Vault::unlock(&path, &SecretString::from("new master"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(v.get("b").is_some() && v.get("c").is_none());
    }

    #[test]
    fn duplicated_hosts_do_not_share_saved_passwords() {
        use super::{drop_foreign_host_ids, owned_by, profile_ids, reusable_for};
        let mut src = HostProfile::new("web", "10.0.0.1");
        let pid = credential_id(src.id, &CredKind::Password);
        assert!(assign_id(&mut src, &CredKind::Password, Some(pid.clone())));
        src.sudo.password_credential = Some(credential_id(src.id, &CredKind::Sudo));
        let key = std::path::PathBuf::from("/k");
        src.auth.push(AuthMethod::KeyFile {
            path: key.clone(),
            passphrase_credential: Some(credential_id(src.id, &CredKind::KeyPassphrase(key))),
        });
        assert!(owned_by(&pid, src.id));
        let mut copy = src.clone();
        copy.id = hostwick_core::ProfileId::new();
        assert!(!owned_by(&pid, copy.id));
        assert!(!reusable_for(&pid, copy.id));
        assert!(reusable_for("key:/k", copy.id));
        assert!(!reusable_for("prod-db", copy.id));
        drop_foreign_host_ids(&mut copy);
        let left: Vec<String> = profile_ids(&copy).into_iter().map(|(_, id)| id).collect();
        assert_eq!(left, vec!["key:/k".to_owned()]);
        // A prefix of another id is not ownership.
        assert!(!owned_by(&format!("host:{}0:password", src.id.0), src.id));
    }

    #[test]
    fn strength() {
        assert_eq!(strength_hint("").0, 0);
        assert_eq!(strength_hint("abc").0, 1);
        assert_eq!(strength_hint("abcdefgh").0, 1);
        assert_eq!(strength_hint("abcdefgh1!").0, 2);
        assert_eq!(strength_hint("correct horse battery staple").0, 3);
        assert_eq!(strength_hint("Tr0ub4dor&3xyz!").0, 3);
    }

    #[test]
    fn vault_session_lifecycle() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let m = VaultManager::new(dir.path().join("vault.json"));
        assert!(!m.exists() && !m.is_unlocked());
        m.create(SecretString::from("master-pass-1"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.is_unlocked());
        m.set("host:1:password", SecretString::from("pw"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.get("host:1:password").is_some());
        // A rejected secret is not handed out again until replaced.
        m.mark_stale("host:1:password");
        assert!(m.get("host:1:password").is_none());
        m.set("host:1:password", SecretString::from("pw2"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.get("host:1:password").is_some());
        m.lock();
        assert!(m.get("host:1:password").is_none());
        assert!(m.unlock(SecretString::from("wrong")).is_err());
        m.unlock(SecretString::from("master-pass-1"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.ids(), vec!["host:1:password".to_owned()]);
        assert!(
            m.change_master(&SecretString::from("nope"), SecretString::from("x"))
                .is_err()
        );
        m.change_master(
            &SecretString::from("master-pass-1"),
            SecretString::from("master-pass-2"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        m.lock();
        m.unlock(SecretString::from("master-pass-2"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.remove("host:1:password").unwrap_or(false));
        assert!(!m.lock_if_idle(0));
    }
}
