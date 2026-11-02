use std::path::{Path, PathBuf};

/// Where Hostwick keeps its files.
///
/// Portable mode (like PuTTY users run from a USB stick): if a file named
/// `hostwick.portable` sits next to the executable, everything — including
/// `known_hosts` — lives in a `data` folder beside it and nothing is written
/// to the user profile.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub portable: bool,
    pub data_dir: PathBuf,
    pub db: PathBuf,
    pub vault: PathBuf,
    pub known_hosts: PathBuf,
    pub logs: PathBuf,
}

pub const PORTABLE_MARKER: &str = "hostwick.portable";

impl AppPaths {
    pub fn detect() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(dir) = exe_dir.filter(|d| d.join(PORTABLE_MARKER).exists()) {
            return Self::portable_in(&dir);
        }
        // HOSTWICK_HOME makes everything self-contained (CI, multiple setups).
        if let Some(home) = std::env::var_os("HOSTWICK_HOME") {
            return Self::rooted(Path::new(&home));
        }
        let data_dir = dirs::data_dir()
            .map(|d| d.join("Hostwick"))
            .unwrap_or_else(|| PathBuf::from(".hostwick"));
        let known_hosts = dirs::home_dir()
            .map(|h| h.join(".ssh").join("known_hosts"))
            .unwrap_or_else(|| data_dir.join("known_hosts"));
        Self::with(data_dir, known_hosts, false)
    }

    pub fn portable_in(exe_dir: &Path) -> Self {
        let data_dir = exe_dir.join("data");
        let kh = data_dir.join("known_hosts");
        Self::with(data_dir, kh, true)
    }

    /// All files under one directory (tests, `HOSTWICK_HOME`).
    pub fn rooted(dir: &Path) -> Self {
        Self::with(dir.to_path_buf(), dir.join("known_hosts"), false)
    }

    fn with(data_dir: PathBuf, known_hosts: PathBuf, portable: bool) -> Self {
        Self {
            portable,
            db: data_dir.join("hostwick.db"),
            vault: data_dir.join("vault.json"),
            logs: data_dir.join("logs"),
            known_hosts,
            data_dir,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.logs)
    }
}
