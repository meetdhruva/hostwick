//! "Scan this computer": find the sessions other SSH tools saved here, and
//! the keys that are ready to use, for the first-run welcome screen.
//!
//! Only reads local files (and on Windows the registry); runs off the UI
//! thread. A source that is missing or empty simply is not reported.

use std::path::{Path, PathBuf};

use hostwick_import::ImportResult;

/// Where to look: the parts of the environment that matter, so tests can
/// point them at temporary folders.
#[derive(Debug, Clone, Default)]
pub struct ScanEnv {
    pub home: Option<PathBuf>,
    /// `%APPDATA%` (Windows).
    pub appdata: Option<PathBuf>,
    /// `%ProgramFiles%`, `%ProgramFiles(x86)%` (Windows).
    pub program_dirs: Vec<PathBuf>,
    /// Read PuTTY's and WinSCP's saved sessions from the Windows registry.
    pub registry: bool,
}

impl ScanEnv {
    pub fn from_system() -> Self {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            home: var("HOME").or_else(|| var("USERPROFILE")),
            appdata: var("APPDATA"),
            program_dirs: ["ProgramFiles", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|k| var(k))
                .collect(),
            registry: cfg!(windows),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    SshConfig,
    PuttyRegistry,
    WinScpRegistry,
    WinScpIni,
    SolarPutty,
}

/// Saved sessions found in one place.
#[derive(Debug)]
pub struct Source {
    pub kind: SourceKind,
    /// The file they came from (not for the registry).
    pub path: Option<PathBuf>,
    pub result: ImportResult,
}

/// Everything found.
#[derive(Debug, Default)]
pub struct Scan {
    pub sources: Vec<Source>,
    /// Private key files in `~/.ssh`.
    pub key_files: Vec<PathBuf>,
}

/// Look for saved sessions and key files (not the agent: see
/// [`hostwick_ssh::agent_key_names`]).
pub fn scan(env: &ScanEnv) -> Scan {
    let mut out = Scan::default();
    let mut push = |kind, path: Option<PathBuf>, result: ImportResult| {
        if !result.profiles.is_empty() {
            out.sources.push(Source { kind, path, result });
        }
    };
    if let Some(home) = &env.home {
        let config = home.join(".ssh").join("config");
        if let Ok(text) = hostwick_import::load_ssh_config(&config) {
            push(
                SourceKind::SshConfig,
                Some(config),
                hostwick_import::parse_ssh_config(&text),
            );
        }
    }
    if env.registry {
        if let Ok(r) = hostwick_import::putty_from_registry() {
            push(SourceKind::PuttyRegistry, None, r);
        }
        if let Ok(r) = hostwick_import::winscp_from_registry() {
            push(SourceKind::WinScpRegistry, None, r);
        }
    }
    // WinSCP.ini: in the roaming profile, or next to a portable WinSCP.
    let inis = env.appdata.iter().map(|a| a.join("WinSCP.ini")).chain(
        env.program_dirs
            .iter()
            .map(|p| p.join("WinSCP").join("WinSCP.ini")),
    );
    for ini in inis {
        if let Some(text) = read_small(&ini)
            && let Ok(r) = hostwick_import::parse_winscp_ini(&text)
        {
            push(SourceKind::WinScpIni, Some(ini), r);
            break;
        }
    }
    // A Solar-PuTTY export the person saved somewhere obvious.
    if let Some(home) = &env.home {
        'solar: for dir in ["Documents", "Downloads", "Desktop"] {
            for file in json_files_named_solar(&home.join(dir)) {
                if let Some(text) = read_small(&file)
                    && let Ok(r) = hostwick_import::parse_solarputty_json(&text)
                    && !r.profiles.is_empty()
                {
                    push(SourceKind::SolarPutty, Some(file), r);
                    break 'solar;
                }
            }
        }
        out.key_files = key_files(&home.join(".ssh"));
    }
    out
}

/// Settings and session files are small; anything bigger is not one.
const MAX_FILE: u64 = 8 * 1024 * 1024;

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    std::fs::read(path)
        .ok()
        .map(|b| hostwick_import::decode_text(&b))
}

/// `*.json` files in `dir` (not below) whose name mentions Solar.
fn json_files_named_solar(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name.ends_with(".json") && name.contains("solar")
        })
        .take(50)
        .collect();
    v.sort();
    v
}

/// Does this start like a private key (OpenSSH, PEM or PuTTY)?
pub fn looks_like_private_key(head: &[u8]) -> bool {
    let head = String::from_utf8_lossy(head);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with("PuTTY-User-Key-File-")
        || (head.starts_with("-----BEGIN ") && head.contains("PRIVATE KEY-----"))
}

/// Private key files in `dir` (`~/.ssh`), by name.
pub fn key_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            let small = std::fs::metadata(p).is_ok_and(|m| m.len() <= 64 * 1024);
            small && head(p).is_some_and(|h| looks_like_private_key(&h))
        })
        .take(100)
        .collect();
    out.sort();
    out
}

fn head(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut buf = vec![0u8; 64];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(path, text).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn nothing_found_in_an_empty_home() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let env = ScanEnv {
            home: Some(dir.path().to_owned()),
            ..Default::default()
        };
        let s = scan(&env);
        assert!(s.sources.is_empty());
        assert!(s.key_files.is_empty());
        // No home at all is fine too.
        let s = scan(&ScanEnv::default());
        assert!(s.sources.is_empty());
    }

    #[test]
    fn finds_ssh_config_winscp_solar_and_keys() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let home = dir.path().join("home");
        write(
            &home.join(".ssh/config"),
            "Host web\n  HostName 10.0.0.5\n  User deploy\n\nHost db\n  HostName 10.0.0.6\n\nHost *\n  ServerAliveInterval 30\n",
        );
        write(
            &home.join(".ssh/id_ed25519"),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3Blbn...\n-----END OPENSSH PRIVATE KEY-----\n",
        );
        write(
            &home.join(".ssh/id_ed25519.pub"),
            "ssh-ed25519 AAAA me@box\n",
        );
        write(
            &home.join(".ssh/old.ppk"),
            "PuTTY-User-Key-File-3: ssh-rsa\nEncryption: none\n",
        );
        write(
            &home.join(".ssh/legacy_rsa"),
            "-----BEGIN RSA PRIVATE KEY-----\nMII...\n",
        );
        write(
            &home.join(".ssh/known_hosts"),
            "github.com ssh-ed25519 AAAA\n",
        );
        write(&home.join(".ssh/config.d/x"), "Host x\n");
        let appdata = dir.path().join("appdata");
        write(
            &appdata.join("WinSCP.ini"),
            "[Sessions\\prod/files]\nHostName=files.example.com\nUserName=ftp\nPortNumber=2222\n",
        );
        write(
            &home.join("Downloads/solar-putty-export.json"),
            r#"{"Sessions":[{"Id":"1","SessionName":"router","Ip":"192.168.1.1","Port":22,"ConnectionType":"SSH"}]}"#,
        );
        write(&home.join("Downloads/other.json"), "{}");
        let env = ScanEnv {
            home: Some(home.clone()),
            appdata: Some(appdata),
            program_dirs: vec![dir.path().join("nowhere")],
            registry: false,
        };
        let s = scan(&env);
        let kinds: Vec<SourceKind> = s.sources.iter().map(|x| x.kind).collect();
        assert_eq!(kinds[0], SourceKind::SshConfig);
        assert!(kinds.contains(&SourceKind::WinScpIni), "{kinds:?}");
        let ssh = &s.sources[0];
        let names: Vec<&str> = ssh
            .result
            .profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["web", "db"], "wildcards are not hosts");
        let winscp = s
            .sources
            .iter()
            .find(|x| x.kind == SourceKind::WinScpIni)
            .map(|x| x.result.profiles.len());
        assert_eq!(winscp, Some(1));
        let solar = s.sources.iter().find(|x| x.kind == SourceKind::SolarPutty);
        assert_eq!(solar.map(|x| x.result.profiles.len()), Some(1));
        assert!(
            solar
                .and_then(|x| x.path.as_ref())
                .is_some_and(|p| p.ends_with("solar-putty-export.json"))
        );
        let keys: Vec<String> = s
            .key_files
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(keys, vec!["id_ed25519", "legacy_rsa", "old.ppk"]);
    }

    #[test]
    fn key_detection() {
        assert!(looks_like_private_key(
            b"-----BEGIN OPENSSH PRIVATE KEY-----\n"
        ));
        assert!(looks_like_private_key(b"-----BEGIN EC PRIVATE KEY-----"));
        assert!(looks_like_private_key(
            b"\xEF\xBB\xBFPuTTY-User-Key-File-2: ssh-rsa"
        ));
        assert!(!looks_like_private_key(b"ssh-ed25519 AAAA"));
        assert!(!looks_like_private_key(b"-----BEGIN PUBLIC KEY-----"));
        assert!(!looks_like_private_key(b""));
    }
}
