use serde::{Deserialize, Serialize};

/// What a session log records (PuTTY's "Session logging" options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogMode {
    /// Text as it appeared, with escape sequences stripped.
    #[default]
    Printable,
    /// Every byte the server sent, escape sequences included.
    Raw,
}

/// Log a session's terminal output to a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLog {
    /// File name template. Tokens (as in PuTTY): `&H` host, `&P` port,
    /// `&U` user, `&S` session name, `&Y` year, `&M` month, `&D` day,
    /// `&T` time `HHMMSS`, `&&` a literal `&`. Relative paths are resolved
    /// against the app's log folder.
    pub path: String,
    #[serde(default)]
    pub mode: LogMode,
    /// Append to an existing file instead of replacing it.
    #[serde(default = "yes")]
    pub append: bool,
}

fn yes() -> bool {
    true
}

impl Default for SessionLog {
    fn default() -> Self {
        Self {
            path: "&S-&Y&M&D-&T.log".into(),
            mode: LogMode::Printable,
            append: true,
        }
    }
}

/// Values substituted into a [`SessionLog::path`] template.
pub struct LogVars<'a> {
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub session: &'a str,
    /// Local time as (year, month, day, hour, minute, second).
    pub time: (i32, u32, u32, u32, u32, u32),
}

/// Expand a log file template. Path separators and characters illegal in
/// Windows file names are removed from substituted values, so a session name
/// like `prod/web` cannot escape the log folder.
pub fn expand_log_path(template: &str, v: &LogVars<'_>) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    };
    let (y, mo, d, h, mi, s) = v.time;
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('H') => out.push_str(&clean(v.host)),
            Some('P') => out.push_str(&v.port.to_string()),
            Some('U') => out.push_str(&clean(v.user)),
            Some('S') => out.push_str(&clean(v.session)),
            Some('Y') => out.push_str(&format!("{y:04}")),
            Some('M') => out.push_str(&format!("{mo:02}")),
            Some('D') => out.push_str(&format!("{d:02}")),
            Some('T') => out.push_str(&format!("{h:02}{mi:02}{s:02}")),
            Some('&') => out.push('&'),
            Some(other) => {
                out.push('&');
                out.push(other);
            }
            None => out.push('&'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_putty_tokens_safely() {
        let v = LogVars {
            host: "10.0.0.5",
            port: 2222,
            user: "ops",
            session: "prod/web",
            time: (2026, 3, 4, 5, 6, 7),
        };
        assert_eq!(
            expand_log_path("&S_&H_&P-&Y&M&D-&T.log", &v),
            "prod_web_10.0.0.5_2222-20260304-050607.log"
        );
        assert_eq!(expand_log_path("logs/&U&&x&Z&", &v), "logs/ops&x&Z&");
    }
}
