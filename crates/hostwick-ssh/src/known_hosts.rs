use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD as B64, STANDARD_NO_PAD as B64_NOPAD};
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use russh::keys::ssh_key::sha2::{Digest, Sha256};
use russh::keys::{HashAlg, PublicKey};

use crate::SshError;
use crate::interaction::HostKeyStatus;

/// OpenSSH-format `known_hosts` file. Hostwick shares `~/.ssh/known_hosts`
/// with OpenSSH by default, so trust decisions carry over in both directions.
#[derive(Debug, Clone)]
pub struct KnownHosts {
    path: PathBuf,
}

impl KnownHosts {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `~/.ssh/known_hosts`.
    pub fn openssh_default() -> Option<Self> {
        dirs::home_dir().map(|h| Self::new(h.join(".ssh").join("known_hosts")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus, SshError> {
        if !self.path.exists() {
            return Ok(HostKeyStatus::Unknown);
        }
        match russh::keys::check_known_hosts_path(host, port, key, &self.path) {
            Ok(true) => Ok(HostKeyStatus::Known),
            Ok(false) => Ok(HostKeyStatus::Unknown),
            Err(russh::keys::Error::KeyChanged { line }) => Ok(HostKeyStatus::Changed { line }),
            Err(e) => Err(SshError::KnownHosts(e.to_string())),
        }
    }

    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), SshError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        russh::keys::known_hosts::learn_known_hosts_path(host, port, key, &self.path)
            .map_err(|e| SshError::KnownHosts(e.to_string()))
    }

    /// Replace the key recorded on `line` (1-based, as reported by
    /// [`HostKeyStatus::Changed`]) with `key`.
    pub fn replace(
        &self,
        line: usize,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<(), SshError> {
        self.rewrite(|n, _| if n == line { Edit::Drop } else { Edit::Keep })?;
        self.learn(host, port, key)
    }

    /// Every host-key line, in file order. A missing file has no entries.
    /// Comments, blank lines and lines that are not host keys are skipped
    /// (and always preserved by the editing methods).
    pub fn entries(&self) -> Result<Vec<KnownHostEntry>, SshError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(text
            .lines()
            .enumerate()
            .filter_map(|(i, l)| parse_line(i + 1, l))
            .collect())
    }

    /// Entries that name `host` on `port` (plain or hashed patterns).
    /// Wildcard patterns are not expanded.
    pub fn entries_for(&self, host: &str, port: u16) -> Result<Vec<KnownHostEntry>, SshError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let names = host_names(host, port);
        Ok(text
            .lines()
            .enumerate()
            .filter(|(_, l)| line_names_host(l, &names))
            .filter_map(|(i, l)| parse_line(i + 1, l))
            .collect())
    }

    /// Forget `host` on `port`, like `ssh-keygen -R`: removes plain and hashed
    /// entries for it. On a line listing several hosts (`web,10.0.0.5 ...`)
    /// only this host is dropped from the list. `@cert-authority` and
    /// `@revoked` lines and wildcard patterns are left alone (remove those
    /// by line number). Returns how many entries were changed.
    pub fn remove_host(&self, host: &str, port: u16) -> Result<usize, SshError> {
        let names = host_names(host, port);
        let mut changed = 0;
        self.rewrite(|_, line| {
            let Some((marker, patterns, rest)) = split_line(line) else {
                return Edit::Keep;
            };
            if marker.is_some() {
                return Edit::Keep;
            }
            let pats: Vec<&str> = patterns.split(',').collect();
            let keep: Vec<&str> = pats
                .iter()
                .copied()
                .filter(|p| !pattern_names_host(p, &names))
                .collect();
            if keep.len() == pats.len() {
                Edit::Keep
            } else if keep.is_empty() {
                changed += 1;
                Edit::Drop
            } else {
                changed += 1;
                Edit::Replace(format!("{} {rest}", keep.join(",")))
            }
        })?;
        Ok(changed)
    }

    /// Remove whole lines by their 1-based number (as [`entries`] reports
    /// them). Returns how many were removed; numbers past the end are ignored.
    ///
    /// [`entries`]: Self::entries
    pub fn remove_lines(&self, lines: &[usize]) -> Result<usize, SshError> {
        let mut removed = 0;
        self.rewrite(|n, _| {
            if lines.contains(&n) {
                removed += 1;
                Edit::Drop
            } else {
                Edit::Keep
            }
        })?;
        Ok(removed)
    }

    /// Trust `key` for `host` on `port` (`[host]:port` when not 22). Returns
    /// `false` if exactly this key is already recorded for the host.
    pub fn add(&self, host: &str, port: u16, key: &PublicKey) -> Result<bool, SshError> {
        let blob = key
            .to_bytes()
            .map_err(|e| SshError::KnownHosts(e.to_string()))?;
        let fp = blob_fingerprint(&blob);
        if self
            .entries_for(host, port)?
            .iter()
            .any(|e| e.marker.is_none() && e.fingerprint == fp)
        {
            return Ok(false);
        }
        self.learn(host, port, key)?;
        Ok(true)
    }

    /// Rewrite the file atomically (temp file + rename), deciding per line.
    /// Kept lines are copied byte for byte, line endings included.
    fn rewrite(&self, mut decide: impl FnMut(usize, &str) -> Edit) -> Result<(), SshError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut out = String::with_capacity(text.len());
        let mut dirty = false;
        for (i, raw) in text.split_inclusive('\n').enumerate() {
            let body = raw.trim_end_matches(['\r', '\n']);
            match decide(i + 1, body) {
                Edit::Keep => out.push_str(raw),
                Edit::Drop => dirty = true,
                Edit::Replace(new) => {
                    dirty = true;
                    out.push_str(&new);
                    out.push_str(&raw[body.len()..]);
                }
            }
        }
        if !dirty {
            return Ok(());
        }
        let dir = self
            .path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "known_hosts".into());
        let tmp = dir.join(format!(".{name}.{}.hostwick-tmp", std::process::id()));
        let result = (|| -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(out.as_bytes())?;
            f.sync_all()?;
            drop(f);
            if let Ok(meta) = std::fs::metadata(&self.path) {
                std::fs::set_permissions(&tmp, meta.permissions())?;
            }
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        Ok(result?)
    }
}

enum Edit {
    Keep,
    Drop,
    Replace(String),
}

/// `@cert-authority` / `@revoked` in front of a known_hosts line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownHostMarker {
    CertAuthority,
    Revoked,
}

impl KnownHostMarker {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CertAuthority => "@cert-authority",
            Self::Revoked => "@revoked",
        }
    }
}

/// One host-key line of a known_hosts file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostEntry {
    /// 1-based line number.
    pub line: usize,
    pub marker: Option<KnownHostMarker>,
    /// Host patterns (`web`, `[db]:2222`, `*.example.com`); empty when
    /// `hashed`.
    pub hosts: Vec<String>,
    /// The host names are stored hashed (`|1|...`, OpenSSH `HashKnownHosts`).
    pub hashed: bool,
    /// `ssh-ed25519`, `ecdsa-sha2-nistp256`, `ssh-rsa`, ...
    pub key_type: String,
    /// `SHA256:...`, or empty if the key data is not valid base64.
    pub fingerprint: String,
    pub comment: String,
}

impl KnownHostEntry {
    /// Comma-separated patterns, or `(hashed)`.
    pub fn hosts_display(&self) -> String {
        if self.hashed && self.hosts.is_empty() {
            "(hashed)".into()
        } else {
            self.hosts.join(",")
        }
    }
}

/// `(marker, patterns, "keytype base64 [comment]")` for a host-key line.
fn split_line(line: &str) -> Option<(Option<KnownHostMarker>, &str, &str)> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let (marker, t) = match t.split_once(char::is_whitespace) {
        Some(("@cert-authority", r)) => (Some(KnownHostMarker::CertAuthority), r.trim_start()),
        Some(("@revoked", r)) => (Some(KnownHostMarker::Revoked), r.trim_start()),
        Some((m, _)) if m.starts_with('@') => return None,
        _ => (None, t),
    };
    let (patterns, rest) = t.split_once(char::is_whitespace)?;
    let rest = rest.trim_start();
    // Must at least have a key type and key data.
    rest.split_whitespace().nth(1)?;
    Some((marker, patterns, rest))
}

fn parse_line(n: usize, line: &str) -> Option<KnownHostEntry> {
    let (marker, patterns, rest) = split_line(line)?;
    let mut parts = rest.splitn(3, char::is_whitespace);
    let key_type = parts.next()?.to_owned();
    let data = parts.next()?;
    let comment = parts.next().unwrap_or("").trim().to_owned();
    let fingerprint = B64
        .decode(data)
        .map(|blob| blob_fingerprint(&blob))
        .unwrap_or_default();
    let mut hosts = Vec::new();
    let mut hashed = false;
    for p in patterns.split(',').filter(|p| !p.is_empty()) {
        if p.starts_with("|1|") {
            hashed = true;
        } else {
            hosts.push(p.to_owned());
        }
    }
    Some(KnownHostEntry {
        line: n,
        marker,
        hosts,
        hashed,
        key_type,
        fingerprint,
        comment,
    })
}

fn blob_fingerprint(blob: &[u8]) -> String {
    format!("SHA256:{}", B64_NOPAD.encode(Sha256::digest(blob)))
}

/// The names OpenSSH would record for `host` on `port`.
fn host_names(host: &str, port: u16) -> Vec<String> {
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    if port == hostwick_core::DEFAULT_PORT {
        vec![host.clone(), format!("[{host}]:{port}")]
    } else {
        vec![format!("[{host}]:{port}")]
    }
}

fn line_names_host(line: &str, names: &[String]) -> bool {
    split_line(line)
        .is_some_and(|(_, patterns, _)| patterns.split(',').any(|p| pattern_names_host(p, names)))
}

/// Exact (case-insensitive) or hashed match; wildcards and negations never
/// match here.
fn pattern_names_host(pattern: &str, names: &[String]) -> bool {
    if let Some(hashed) = pattern.strip_prefix("|1|") {
        let Some((salt, hash)) = hashed.split_once('|') else {
            return false;
        };
        let (Ok(salt), Ok(hash)) = (B64.decode(salt), B64.decode(hash)) else {
            return false;
        };
        names.iter().any(|name| {
            <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&salt).is_ok_and(|mac| {
                mac.chain_update(name.as_bytes())
                    .verify_slice(&hash)
                    .is_ok()
            })
        })
    } else {
        names.iter().any(|n| n.eq_ignore_ascii_case(pattern))
    }
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b64: &str) -> PublicKey {
        russh::keys::parse_public_key_base64(b64).unwrap_or_else(|e| panic!("{e}"))
    }

    // Two arbitrary, valid ed25519 public keys.
    const K1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAvL1GGwODl262OAAcFSein2LSIyeWeK4bCI913E0PMB";
    const K2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIIdL8bVLpSDTnT06ap4qNlK/6WoKb3qqsRlFQednSzHA";

    #[test]
    fn tofu_learn_then_detect_change_then_replace() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let kh = KnownHosts::new(dir.path().join("known_hosts"));
        let (k1, k2) = (key(K1), key(K2));

        assert_eq!(kh.check("srv", 22, &k1).ok(), Some(HostKeyStatus::Unknown));
        kh.learn("srv", 22, &k1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(kh.check("srv", 22, &k1).ok(), Some(HostKeyStatus::Known));
        // Non-default port is a different host entry.
        assert_eq!(
            kh.check("srv", 2222, &k1).ok(),
            Some(HostKeyStatus::Unknown)
        );

        let status = kh.check("srv", 22, &k2).unwrap_or_else(|e| panic!("{e}"));
        let HostKeyStatus::Changed { line } = status else {
            panic!("expected Changed, got {status:?}")
        };
        kh.replace(line, "srv", 22, &k2)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(kh.check("srv", 22, &k2).ok(), Some(HostKeyStatus::Known));
        assert!(matches!(
            kh.check("srv", 22, &k1),
            Ok(HostKeyStatus::Changed { .. })
        ));
    }

    fn hashed(salt: &[u8], name: &str) -> String {
        let mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(salt)
            .unwrap_or_else(|e| panic!("{e}"))
            .chain_update(name.as_bytes())
            .finalize()
            .into_bytes();
        format!("|1|{}|{}", B64.encode(salt), B64.encode(mac))
    }

    fn fixture() -> String {
        format!(
            "# my hosts\r\n\
             web,10.0.0.5 ssh-ed25519 {K1} web key\r\n\
             \n\
             [db]:2222 ssh-ed25519 {K2}\n\
             {} ssh-ed25519 {K2}\n\
             @cert-authority *.example.com ssh-ed25519 {K1} ca\n\
             @revoked web ssh-ed25519 {K2}\n\
             *.lan ssh-ed25519 {K1}\n\
             garbage\n\
             db ssh-ed25519 {K1}",
            hashed(b"0123456789abcdef0123", "db")
        )
    }

    #[test]
    fn lists_entries_with_markers_hashes_and_fingerprints() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::new(&path);
        assert!(kh.entries().unwrap_or_else(|e| panic!("{e}")).is_empty());
        std::fs::write(&path, fixture()).unwrap_or_else(|e| panic!("{e}"));
        let e = kh.entries().unwrap_or_else(|e| panic!("{e}"));
        let lines: Vec<usize> = e.iter().map(|e| e.line).collect();
        assert_eq!(lines, vec![2, 4, 5, 6, 7, 8, 10]);
        assert_eq!(e[0].hosts, vec!["web", "10.0.0.5"]);
        assert_eq!(e[0].comment, "web key");
        assert_eq!(e[0].key_type, "ssh-ed25519");
        assert_eq!(e[0].fingerprint, fingerprint(&key(K1)));
        assert_eq!(e[1].hosts_display(), "[db]:2222");
        assert!(e[2].hashed && e[2].hosts.is_empty());
        assert_eq!(e[2].hosts_display(), "(hashed)");
        assert_eq!(e[3].marker, Some(KnownHostMarker::CertAuthority));
        assert_eq!(e[3].comment, "ca");
        assert_eq!(e[4].marker, Some(KnownHostMarker::Revoked));

        let db: Vec<usize> = kh
            .entries_for("db", 22)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(|e| e.line)
            .collect();
        assert_eq!(db, vec![5, 10]);
        let db2222: Vec<usize> = kh
            .entries_for("db", 2222)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(|e| e.line)
            .collect();
        assert_eq!(db2222, vec![4]);
    }

    #[test]
    fn remove_by_host_keeps_everything_else_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::new(&path);
        let original = fixture();
        std::fs::write(&path, &original).unwrap_or_else(|e| panic!("{e}"));

        // Hashed + plain entries for db:22; [db]:2222 is a different host.
        assert_eq!(
            kh.remove_host("db", 22).unwrap_or_else(|e| panic!("{e}")),
            2
        );
        let after = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let expected: String = original
            .split_inclusive('\n')
            .enumerate()
            .filter(|(i, _)| ![4, 9].contains(i))
            .map(|(_, l)| l)
            .collect();
        assert_eq!(after, expected);

        // A multi-host line loses only that host, keeping its CRLF; the
        // @revoked line for `web` stays.
        assert_eq!(
            kh.remove_host("WEB", 22).unwrap_or_else(|e| panic!("{e}")),
            1
        );
        let after = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        assert!(after.contains(&format!("\r\n10.0.0.5 ssh-ed25519 {K1} web key\r\n")));
        assert!(after.contains("@revoked web"));
        assert!(after.starts_with("# my hosts\r\n"));
        assert_eq!(
            kh.remove_host("web", 22).unwrap_or_else(|e| panic!("{e}")),
            0
        );

        assert_eq!(
            kh.remove_host("db", 2222).unwrap_or_else(|e| panic!("{e}")),
            1
        );
        // Wildcards are only removed by line.
        assert_eq!(
            kh.remove_host("x.lan", 22)
                .unwrap_or_else(|e| panic!("{e}")),
            0
        );
        let wild = kh
            .entries()
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .find(|e| e.hosts == ["*.lan"])
            .unwrap_or_else(|| panic!("no wildcard entry"));
        assert_eq!(
            kh.remove_lines(&[wild.line, 999])
                .unwrap_or_else(|e| panic!("{e}")),
            1
        );
        let left: Vec<String> = kh
            .entries()
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(KnownHostEntry::hosts_display)
            .collect();
        assert_eq!(left, vec!["10.0.0.5", "*.example.com", "web"]);
        // No temp files left behind.
        assert_eq!(
            std::fs::read_dir(dir.path()).map(Iterator::count).ok(),
            Some(1)
        );
    }

    #[test]
    fn add_is_idempotent_and_checkable() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let kh = KnownHosts::new(dir.path().join("sub").join("known_hosts"));
        assert!(
            kh.add("srv", 2200, &key(K1))
                .unwrap_or_else(|e| panic!("{e}"))
        );
        assert!(
            !kh.add("srv", 2200, &key(K1))
                .unwrap_or_else(|e| panic!("{e}"))
        );
        assert_eq!(
            kh.check("srv", 2200, &key(K1)).ok(),
            Some(HostKeyStatus::Known)
        );
        // A second key type for the same host is fine.
        assert!(
            kh.add("srv", 2200, &key(K2))
                .unwrap_or_else(|e| panic!("{e}"))
        );
        let e = kh.entries().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].hosts, vec!["[srv]:2200"]);
    }

    #[test]
    fn removes_entries_hashed_by_ssh_keygen() {
        let Ok(out) = std::process::Command::new("ssh-keygen").arg("-?").output() else {
            eprintln!("skipping: ssh-keygen not installed");
            return;
        };
        drop(out);
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("known_hosts");
        std::fs::write(
            &path,
            format!("keep ssh-ed25519 {K1}\nsrv ssh-ed25519 {K1}\n[srv]:2022 ssh-ed25519 {K2}\n"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let st = std::process::Command::new("ssh-keygen")
            .arg("-H")
            .arg("-f")
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(st.status.success());
        let kh = KnownHosts::new(&path);
        assert!(
            kh.entries()
                .unwrap_or_else(|e| panic!("{e}"))
                .iter()
                .all(|e| e.hashed)
        );
        assert_eq!(
            kh.remove_host("srv", 2022)
                .unwrap_or_else(|e| panic!("{e}")),
            1
        );
        assert_eq!(
            kh.remove_host("srv", 22).unwrap_or_else(|e| panic!("{e}")),
            1
        );
        assert_eq!(kh.entries().unwrap_or_else(|e| panic!("{e}")).len(), 1);
        assert_eq!(
            kh.check("keep", 22, &key(K1)).ok(),
            Some(HostKeyStatus::Known)
        );
    }

    #[test]
    fn fingerprint_is_openssh_style() {
        assert!(fingerprint(&key(K1)).starts_with("SHA256:"));
    }
}
