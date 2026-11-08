//! Breadcrumbs: a folder path split into clickable segments.
//!
//! Handles POSIX paths (`/var/www`), Windows drive paths in either slash
//! style (`C:/Users/x`, `C:\Users\x`) and UNC shares (`\\server\share\dir`,
//! `//server/share/dir`). Every crumb carries the path to navigate to, in
//! the form the file browser uses internally (forward slashes).

/// One clickable segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    /// What the button says.
    pub label: String,
    /// Where it goes.
    pub path: String,
}

fn crumb(label: impl Into<String>, path: impl Into<String>) -> Crumb {
    Crumb {
        label: label.into(),
        path: path.into(),
    }
}

/// The crumbs of `path`, root first. Empty for an empty path.
pub fn crumbs(path: &str) -> Vec<Crumb> {
    let norm = path.trim().replace('\\', "/");
    if norm.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let rest: &str;
    let mut prefix: String;

    let bytes = norm.as_bytes();
    if norm.starts_with("//") && !norm.starts_with("///") && norm.len() > 2 {
        // UNC: //server/share/dir… — the share is the root.
        let tail = &norm[2..];
        let mut parts = tail.splitn(3, '/');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        rest = parts.next().unwrap_or_default();
        if server.is_empty() {
            return vec![crumb("/", "/")];
        }
        if share.is_empty() {
            return vec![crumb(format!(r"\\{server}"), format!("//{server}"))];
        }
        prefix = format!("//{server}/{share}");
        out.push(crumb(format!(r"\\{server}\{share}"), format!("{prefix}/")));
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        // Drive: C:/Users/x
        let drive = &norm[..2];
        prefix = drive.to_owned();
        rest = norm[2..].trim_start_matches('/');
        out.push(crumb(drive, format!("{drive}/")));
    } else if norm.starts_with('/') {
        prefix = String::new();
        rest = norm.trim_start_matches('/');
        out.push(crumb("/", "/"));
    } else {
        // Relative ("." before the first listing, "~"): one crumb per part.
        prefix = String::new();
        // No root crumb: every part is a plain segment.
        for part in norm.split('/').filter(|p| !p.is_empty()) {
            if prefix.is_empty() {
                prefix = part.to_owned();
            } else {
                prefix = format!("{prefix}/{part}");
            }
            out.push(crumb(part, prefix.clone()));
        }
        return out;
    }

    for part in rest.split('/').filter(|p| !p.is_empty()) {
        prefix = format!("{prefix}/{part}");
        out.push(crumb(part, prefix.clone()));
    }
    out
}

/// Keep the tail of `crumbs` that fits a budget of `max_chars` characters
/// (labels plus a separator each); the crumbs that do not fit are returned
/// first so the UI can offer them in a menu. The last crumb is always
/// kept, and so is at least one.
pub fn split_overflow(crumbs: &[Crumb], max_chars: usize) -> (&[Crumb], &[Crumb]) {
    let mut used = 0usize;
    let mut keep_from = crumbs.len();
    for (i, c) in crumbs.iter().enumerate().rev() {
        let w = c.label.chars().count() + 3;
        if used + w > max_chars && keep_from < crumbs.len() {
            break;
        }
        used += w;
        keep_from = i;
    }
    crumbs.split_at(keep_from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(p: &str) -> Vec<(String, String)> {
        crumbs(p).into_iter().map(|c| (c.label, c.path)).collect()
    }

    fn lp(l: &str, p: &str) -> (String, String) {
        (l.to_owned(), p.to_owned())
    }

    #[test]
    fn posix_paths() {
        assert_eq!(pairs("/"), vec![lp("/", "/")]);
        assert_eq!(
            pairs("/var/www/html"),
            vec![
                lp("/", "/"),
                lp("var", "/var"),
                lp("www", "/var/www"),
                lp("html", "/var/www/html")
            ]
        );
        // Trailing and doubled slashes do not make empty crumbs.
        assert_eq!(
            pairs("/a//b/"),
            vec![lp("/", "/"), lp("a", "/a"), lp("b", "/a/b")]
        );
        assert_eq!(crumbs("/home/user/my docs")[3].label, "my docs");
        assert!(crumbs("").is_empty());
        assert!(crumbs("   ").is_empty());
    }

    #[test]
    fn windows_drive_paths_in_both_slash_styles() {
        let want = vec![
            lp("C:", "C:/"),
            lp("Users", "C:/Users"),
            lp("x", "C:/Users/x"),
        ];
        assert_eq!(pairs("C:/Users/x"), want);
        assert_eq!(pairs(r"C:\Users\x"), want);
        assert_eq!(pairs(r"C:\Users\x\"), want);
        assert_eq!(pairs("C:/"), vec![lp("C:", "C:/")]);
        assert_eq!(pairs("c:"), vec![lp("c:", "c:/")]);
        // A drive-less relative name is not mistaken for a drive.
        assert_eq!(pairs("cd/x"), vec![lp("cd", "cd"), lp("x", "cd/x")]);
    }

    #[test]
    fn unc_shares() {
        let want = vec![
            lp(r"\\server\share", "//server/share/"),
            lp("dir", "//server/share/dir"),
            lp("sub", "//server/share/dir/sub"),
        ];
        assert_eq!(pairs(r"\\server\share\dir\sub"), want);
        assert_eq!(pairs("//server/share/dir/sub"), want);
        assert_eq!(
            pairs(r"\\server\share"),
            vec![lp(r"\\server\share", "//server/share/")]
        );
        assert_eq!(pairs(r"\\server"), vec![lp(r"\\server", "//server")]);
        // Three leading slashes are just a POSIX root.
        assert_eq!(pairs("///a"), vec![lp("/", "/"), lp("a", "/a")]);
    }

    #[test]
    fn relative_and_home_paths() {
        assert_eq!(pairs("."), vec![lp(".", ".")]);
        assert_eq!(pairs("~/src"), vec![lp("~", "~"), lp("src", "~/src")]);
    }

    #[test]
    fn long_paths_keep_the_tail() {
        let c = crumbs("/srv/very/long/path/to/some/folder");
        let (hidden, shown) = split_overflow(&c, 24);
        assert!(!hidden.is_empty());
        assert_eq!(shown.last().map(|c| c.label.as_str()), Some("folder"));
        assert_eq!(hidden.len() + shown.len(), c.len());
        // Always at least the last crumb, however tight the budget.
        let (_, shown) = split_overflow(&c, 1);
        assert_eq!(shown.len(), 1);
        // Everything fits: nothing hidden.
        let (hidden, shown) = split_overflow(&c, 500);
        assert!(hidden.is_empty());
        assert_eq!(shown.len(), c.len());
        let (h, s) = split_overflow(&[], 10);
        assert!(h.is_empty() && s.is_empty());
    }
}
