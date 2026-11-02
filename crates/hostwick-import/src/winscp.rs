use hostwick_core::{HostProfile, ProfileSource};

use crate::raw::{RawSession, Value};
use crate::{ImportError, ImportResult, auth_with_key, percent_decode, split_folder};

/// Parse `WinSCP.ini` (WinSCP's portable/INI storage mode).
pub fn parse_winscp_ini(text: &str) -> Result<ImportResult, ImportError> {
    let mut sessions = Vec::new();
    let mut current: Option<RawSession> = None;
    let mut saw_sessions = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if let Some(s) = current.take() {
                sessions.push(s);
            }
            current = section.strip_prefix("Sessions\\").map(|n| {
                saw_sessions = true;
                RawSession {
                    name: percent_decode(n),
                    ..Default::default()
                }
            });
            continue;
        }
        if let (Some(cur), Some((k, v))) = (current.as_mut(), line.split_once('=')) {
            cur.insert(k.trim(), Value::Str(percent_decode(v.trim())));
        }
    }
    if let Some(s) = current.take() {
        sessions.push(s);
    }
    if !saw_sessions {
        return Err(ImportError::Format("WinSCP.ini"));
    }
    Ok(convert(sessions))
}

/// Read sessions from WinSCP's registry storage (Windows only).
pub fn winscp_from_registry() -> Result<ImportResult, ImportError> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let root = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Martin Prikryl\WinSCP 2\Sessions")?;
        let mut sessions = Vec::new();
        for name in root.enum_keys().flatten() {
            let Ok(key) = root.open_subkey(&name) else {
                continue;
            };
            let mut raw = RawSession {
                name: percent_decode(&name),
                ..Default::default()
            };
            for field in ["HostName", "UserName", "PublicKeyFile", "Password"] {
                if let Ok(v) = key.get_value::<String, _>(field) {
                    raw.insert(field, Value::Str(percent_decode(&v)));
                }
            }
            for field in ["PortNumber", "FSProtocol"] {
                if let Ok(v) = key.get_value::<u32, _>(field) {
                    raw.insert(field, Value::Dword(v));
                }
            }
            sessions.push(raw);
        }
        Ok(convert(sessions))
    }
    #[cfg(not(windows))]
    {
        Err(ImportError::Unsupported(
            "reading the WinSCP registry; use WinSCP.ini instead",
        ))
    }
}

fn convert(sessions: Vec<RawSession>) -> ImportResult {
    let mut out = ImportResult::default();
    for s in sessions {
        if s.name == "Default Settings" {
            continue;
        }
        // FSProtocol: 0 SCP, 1 SFTP with SCP fallback, 2 SFTP; 5+ are FTP,
        // WebDAV and S3, which are not SSH.
        if let Some(fs) = s.num("FSProtocol").filter(|p| *p >= 5) {
            out.warnings
                .push(format!("{}: skipped (non-SSH protocol {fs})", s.name));
            continue;
        }
        let Some(host) = s.str("HostName") else {
            out.warnings
                .push(format!("{}: skipped (no host name)", s.name));
            continue;
        };
        let (folder, name) = split_folder(&s.name);
        let mut p = HostProfile::new(name, host);
        p.folder = folder;
        p.port = s
            .num("PortNumber")
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n != 0)
            .unwrap_or(22);
        p.user = s.str("UserName");
        p.auth = auth_with_key(s.str("PublicKeyFile"));
        p.source = ProfileSource::WinScp {
            session: s.name.clone(),
        };
        if s.str("Password").is_some() {
            out.warnings.push(format!(
                "{}: has a saved WinSCP password, which is not imported; store it in the Hostwick vault",
                s.name
            ));
        }
        out.profiles.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = r"[Configuration\Interface]
Theme=Dark

[Sessions\Default%20Settings]
HostName=

[Sessions\Clients/Acme/files]
HostName=sftp.acme.example
PortNumber=2222
UserName=acme
PublicKeyFile=C:%5CUsers%5Cme%5Cacme.ppk
FSProtocol=2
Password=A35C4E5A3333

[Sessions\ftp-site]
HostName=ftp.example.com
FSProtocol=5

[Sessions\plain]
HostName=10.0.0.9
";

    #[test]
    fn parses_ini() {
        let r = parse_winscp_ini(INI).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.profiles.len(), 2);
        let a = &r.profiles[0];
        assert_eq!(
            (a.folder.as_str(), a.name.as_str()),
            ("Clients/Acme", "files")
        );
        assert_eq!(
            (a.host.as_str(), a.port, a.user.as_deref()),
            ("sftp.acme.example", 2222, Some("acme"))
        );
        assert!(format!("{:?}", a.auth[1]).contains(r"C:\\Users\\me\\acme.ppk"));
        assert_eq!(r.profiles[1].port, 22);
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
        assert!(parse_winscp_ini("[Configuration]\nx=1").is_err());
    }
}
