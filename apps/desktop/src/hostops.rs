//! Saved-host management without the UI: favourites, the folder tree the
//! sidebar shows, moving/renaming/deleting folders, and bulk edits.
//!
//! Pure logic, independent of GPUI, so it is unit tested.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use hostwick_core::{AuthMethod, Endpoint, FileProtocol, HostProfile, Protocol, ProxySpec};

/// Favourites are a tag, so they travel with the host through settings sync.
pub const FAVOURITE_TAG: &str = "favourite";

pub fn is_favourite(p: &HostProfile) -> bool {
    p.tags.iter().any(|t| t == FAVOURITE_TAG)
}

/// Mark or unmark; true if the profile changed.
pub fn set_favourite(p: &mut HostProfile, on: bool) -> bool {
    if is_favourite(p) == on {
        return false;
    }
    if on {
        p.tags.push(FAVOURITE_TAG.to_owned());
    } else {
        p.tags.retain(|t| t != FAVOURITE_TAG);
    }
    true
}

// ---- recent connections -----------------------------------------------------

/// A recent connection: a saved host, or quick-connect text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Recent {
    Host { id: hostwick_core::ProfileId },
    Quick { text: String },
}

/// Recent connections kept (the sidebar shows fewer).
pub const RECENTS_KEEP: usize = 20;

/// Put `r` first, without duplicates, keeping at most [`RECENTS_KEEP`].
pub fn push_recent(list: &mut Vec<Recent>, r: Recent) {
    list.retain(|x| *x != r);
    list.insert(0, r);
    list.truncate(RECENTS_KEEP);
}

pub fn recents_from_json(json: &str) -> Vec<Recent> {
    serde_json::from_str(json).unwrap_or_default()
}

pub fn recents_to_json(list: &[Recent]) -> String {
    serde_json::to_string(list).unwrap_or_default()
}

// ---- folders ----------------------------------------------------------------

/// `prod/eu` → `prod`; `prod` → ``.
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// `prod/eu` → `eu`.
pub fn leaf(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, l)| l)
}

/// `a` + `b` → `a/b` (either may be empty).
pub fn join(parent: &str, name: &str) -> String {
    match (parent.is_empty(), name.is_empty()) {
        (true, _) => name.to_owned(),
        (_, true) => parent.to_owned(),
        _ => format!("{parent}/{name}"),
    }
}

/// A folder path as stored: no empty segments or surrounding spaces.
pub fn normalize(path: &str) -> String {
    path.split(['/', '\\'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Is `name` usable as one folder level?
pub fn valid_folder_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && !n.contains(['/', '\\']) && n != "." && n != ".."
}

/// Is `folder` the folder `dir` or inside it?
pub fn within(folder: &str, dir: &str) -> bool {
    dir.is_empty()
        || folder == dir
        || (folder.len() > dir.len()
            && folder.starts_with(dir)
            && folder.as_bytes()[dir.len()] == b'/')
}

/// The folder `folder` becomes when `from` is renamed or moved to `to`
/// (its subfolders follow); None if it is not affected.
pub fn remap(folder: &str, from: &str, to: &str) -> Option<String> {
    if from.is_empty() || !within(folder, from) {
        return None;
    }
    Some(join(to, folder[from.len()..].trim_start_matches('/')))
}

/// Every folder: those hosts are in, their parents, and the empty folders
/// kept in the settings.
pub fn all_folders<'a>(
    host_folders: impl IntoIterator<Item = &'a str>,
    explicit: &[String],
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in host_folders
        .into_iter()
        .map(normalize)
        .chain(explicit.iter().map(|f| normalize(f)))
    {
        let mut f = f.as_str();
        while !f.is_empty() {
            if !out.insert(f.to_owned()) {
                break;
            }
            f = parent(f);
        }
    }
    out
}

/// A name for a new folder in `parent` that is not taken: `base`, `base 2`…
pub fn unique_folder(parent: &str, base: &str, existing: &BTreeSet<String>) -> String {
    let mut n = 1;
    loop {
        let name = if n == 1 {
            base.to_owned()
        } else {
            format!("{base} {n}")
        };
        let path = join(parent, &name);
        if !existing.iter().any(|e| e.eq_ignore_ascii_case(&path)) {
            return path;
        }
        n += 1;
    }
}

/// What renaming, moving or deleting a folder changes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FolderChange {
    /// Host index → its new folder.
    pub hosts: Vec<(usize, String)>,
    /// The new list of empty folders to keep in the settings.
    pub explicit: Vec<String>,
}

/// Move folder `from` (and everything in it) to `to`. Renaming is a move
/// within the same parent. Folders named the same are merged.
pub fn move_folder(
    host_folders: &[&str],
    explicit: &[String],
    from: &str,
    to: &str,
) -> FolderChange {
    let (from, to) = (normalize(from), normalize(to));
    let hosts = host_folders
        .iter()
        .enumerate()
        .filter_map(|(i, f)| remap(&normalize(f), &from, &to).map(|n| (i, n)))
        .collect();
    let mut seen = BTreeSet::new();
    let explicit = explicit
        .iter()
        .map(|f| {
            let f = normalize(f);
            remap(&f, &from, &to).unwrap_or(f)
        })
        .filter(|f| !f.is_empty() && seen.insert(f.clone()))
        .collect();
    FolderChange { hosts, explicit }
}

/// Delete folder `dir`: its hosts and subfolders move up one level (into
/// its parent); nothing else is lost.
pub fn delete_folder(host_folders: &[&str], explicit: &[String], dir: &str) -> FolderChange {
    let dir = normalize(dir);
    let up = parent(&dir).to_owned();
    let mut change = move_folder(host_folders, explicit, &dir, &up);
    change
        .explicit
        .retain(|f| *f != up || explicit.iter().any(|e| normalize(e) == up));
    change
}

/// Number of hosts directly or indirectly in each folder.
pub fn host_counts<'a>(host_folders: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in host_folders {
        let mut f = normalize(f);
        while !f.is_empty() {
            *out.entry(f.clone()).or_default() += 1;
            f = parent(&f).to_owned();
        }
    }
    out
}

/// One row of the folder tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Folder {
        path: String,
        depth: usize,
        /// Hosts inside, recursively.
        count: usize,
        collapsed: bool,
    },
    /// Index into the host list.
    Host { index: usize, depth: usize },
}

/// The folder tree: at each level folders first (then hosts), each sorted
/// by name ignoring case; nothing inside a collapsed folder.
pub fn tree_rows(
    hosts: &[(&str, &str)],
    explicit: &[String],
    collapsed: &HashSet<String>,
) -> Vec<TreeRow> {
    let folders = all_folders(hosts.iter().map(|(f, _)| *f), explicit);
    let counts = host_counts(hosts.iter().map(|(f, _)| *f));
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in &folders {
        children
            .entry(parent(f).to_owned())
            .or_default()
            .push(f.clone());
    }
    let mut by_folder: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (f, _)) in hosts.iter().enumerate() {
        by_folder.entry(normalize(f)).or_default().push(i);
    }
    let walker = Walker {
        children: &children,
        by_folder: &by_folder,
        counts: &counts,
        hosts,
        collapsed,
    };
    let mut out = Vec::new();
    walker.walk("", 0, &mut out);
    out
}

struct Walker<'a> {
    children: &'a BTreeMap<String, Vec<String>>,
    by_folder: &'a BTreeMap<String, Vec<usize>>,
    counts: &'a BTreeMap<String, usize>,
    hosts: &'a [(&'a str, &'a str)],
    collapsed: &'a HashSet<String>,
}

impl Walker<'_> {
    fn walk(&self, dir: &str, depth: usize, out: &mut Vec<TreeRow>) {
        let mut subs = self.children.get(dir).cloned().unwrap_or_default();
        subs.sort_by_key(|s| leaf(s).to_lowercase());
        for s in subs {
            let is_collapsed = self.collapsed.contains(&s);
            out.push(TreeRow::Folder {
                path: s.clone(),
                depth,
                count: self.counts.get(&s).copied().unwrap_or(0),
                collapsed: is_collapsed,
            });
            if !is_collapsed {
                self.walk(&s, depth + 1, out);
            }
        }
        let mut own = self.by_folder.get(dir).cloned().unwrap_or_default();
        own.sort_by_key(|i| self.hosts[*i].1.to_lowercase());
        out.extend(own.into_iter().map(|index| TreeRow::Host { index, depth }));
    }
}

// ---- bulk edit --------------------------------------------------------------

/// Changes to apply to many hosts at once. `None` = leave as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BulkEdit {
    /// `Some(None)` clears the user (log in as the local user).
    pub user: Option<Option<String>>,
    pub port: Option<u16>,
    /// Replaces the key files (other ways of logging in stay).
    pub keys: Option<Vec<PathBuf>>,
    pub jump: Option<Vec<Endpoint>>,
    /// `Some(None)` removes the proxy.
    pub proxy: Option<Option<ProxySpec>>,
    pub folder: Option<String>,
    pub keepalive: Option<u32>,
    pub file_protocol: Option<FileProtocol>,
    pub term: Option<String>,
}

impl BulkEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Apply to one host; true if it changed. Settings that do not exist
    /// for the host's protocol are skipped (no port or user for a serial
    /// port, no keys, jump hosts or file protocol without SSH).
    pub fn apply(&self, p: &mut HostProfile) -> bool {
        let before = p.clone();
        let ssh = p.protocol == Protocol::Ssh;
        let serial = matches!(p.protocol, Protocol::Serial(_));
        if let Some(f) = &self.folder {
            p.folder = normalize(f);
        }
        if !serial {
            if let Some(u) = &self.user {
                p.user = u.clone().filter(|u| !u.trim().is_empty());
            }
            if let Some(port) = self.port {
                p.port = port;
            }
            if let Some(k) = self.keepalive {
                p.keepalive_secs = k;
            }
            if let Some(t) = &self.term {
                p.term = t.clone();
            }
            if let Some(proxy) = &self.proxy {
                let keep_secret = |new: &ProxySpec| {
                    p.proxy
                        .as_ref()
                        .filter(|old| old.host == new.host && old.user == new.user)
                        .and_then(|old| old.password_credential.clone())
                };
                p.proxy = proxy.clone().map(|mut new| {
                    new.password_credential = keep_secret(&new);
                    new
                });
            }
        }
        if ssh {
            if let Some(keys) = &self.keys {
                p.auth = replace_keys(&p.auth, keys);
            }
            if let Some(j) = &self.jump {
                p.jump = j.clone();
            }
            if let Some(fp) = self.file_protocol {
                p.file_protocol = fp;
            }
        }
        *p != before
    }
}

/// The auth list with its key files replaced by `keys`, at the same place
/// (after the agent and Kerberos, before passwords). Saved passphrases of
/// keys that stay are kept.
pub fn replace_keys(auth: &[AuthMethod], keys: &[PathBuf]) -> Vec<AuthMethod> {
    let saved = |path: &PathBuf| {
        auth.iter().find_map(|a| match a {
            AuthMethod::KeyFile {
                path: p,
                passphrase_credential,
            } if p == path => passphrase_credential.clone(),
            _ => None,
        })
    };
    let new_keys: Vec<AuthMethod> = keys
        .iter()
        .map(|k| AuthMethod::KeyFile {
            path: k.clone(),
            passphrase_credential: saved(k),
        })
        .collect();
    let mut out = Vec::new();
    let mut placed = false;
    for a in auth {
        match a {
            AuthMethod::KeyFile { .. } => {
                if !placed {
                    out.extend(new_keys.iter().cloned());
                    placed = true;
                }
            }
            AuthMethod::Password { .. } | AuthMethod::KeyboardInteractive if !placed => {
                out.extend(new_keys.iter().cloned());
                placed = true;
                out.push(a.clone());
            }
            _ => out.push(a.clone()),
        }
    }
    if !placed {
        out.extend(new_keys);
    }
    out
}

/// The value every item shares, if they all share one.
pub fn common<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

/// A host's key files, comma-separated (as the editors show them).
pub fn key_text(p: &HostProfile) -> String {
    p.auth
        .iter()
        .filter_map(|a| match a {
            AuthMethod::KeyFile { path, .. } => Some(path.display().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Key files from comma-separated text.
pub fn parse_keys(text: &str) -> Vec<PathBuf> {
    text.split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hostwick_core::{ProxyKind, SerialSettings};

    #[test]
    fn favourites_are_a_tag() {
        let mut p = HostProfile::new("a", "h");
        assert!(!is_favourite(&p));
        assert!(set_favourite(&mut p, true));
        assert!(!set_favourite(&mut p, true));
        assert!(is_favourite(&p));
        p.tags.push("other".into());
        assert!(set_favourite(&mut p, false));
        assert_eq!(p.tags, vec!["other".to_owned()]);
    }

    #[test]
    fn folder_paths() {
        assert_eq!(parent("a/b/c"), "a/b");
        assert_eq!(parent("a"), "");
        assert_eq!(leaf("a/b/c"), "c");
        assert_eq!(join("", "x"), "x");
        assert_eq!(join("a", ""), "a");
        assert_eq!(normalize(" /a// b \\c/ "), "a/b/c");
        assert!(within("prod/eu", "prod"));
        assert!(!within("production", "prod"));
        assert!(within("prod", "prod"));
        assert!(within("anything", ""));
        assert_eq!(remap("prod/eu", "prod", "live").as_deref(), Some("live/eu"));
        assert_eq!(remap("prod", "prod", "").as_deref(), Some(""));
        assert_eq!(remap("production", "prod", "x"), None);
        assert!(valid_folder_name("db servers"));
        assert!(!valid_folder_name("a/b"));
        assert!(!valid_folder_name(" "));
        assert!(!valid_folder_name(".."));
        let all = all_folders(["a/b/c", "", "d"], &["e/f".to_owned()]);
        assert_eq!(
            all.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["a", "a/b", "a/b/c", "d", "e", "e/f"]
        );
        assert_eq!(unique_folder("a", "New folder", &all), "a/New folder");
        let taken: BTreeSet<String> = ["New folder".into(), "new folder 2".into()].into();
        assert_eq!(unique_folder("", "New folder", &taken), "New folder 3");
    }

    #[test]
    fn tree_lists_folders_first_and_hides_collapsed() {
        let hosts = [
            ("prod/eu", "web2"),
            ("prod/eu", "Web1"),
            ("prod", "db"),
            ("", "zeta"),
            ("", "alpha"),
            ("lab", "pi"),
        ];
        let rows = tree_rows(&hosts, &["empty".to_owned()], &HashSet::new());
        let show = |rows: &[TreeRow]| {
            rows.iter()
                .map(|r| match r {
                    TreeRow::Folder {
                        path, depth, count, ..
                    } => format!("{depth}:[{path} {count}]"),
                    TreeRow::Host { index, depth } => format!("{depth}:{}", hosts[*index].1),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            show(&rows),
            vec![
                "0:[empty 0]",
                "0:[lab 1]",
                "1:pi",
                "0:[prod 3]",
                "1:[prod/eu 2]",
                "2:Web1",
                "2:web2",
                "1:db",
                "0:alpha",
                "0:zeta",
            ]
        );
        let collapsed: HashSet<String> = ["prod".to_owned()].into();
        let rows = tree_rows(&hosts, &[], &collapsed);
        assert_eq!(
            show(&rows),
            vec!["0:[lab 1]", "1:pi", "0:[prod 3]", "0:alpha", "0:zeta"]
        );
        assert!(matches!(
            rows[2],
            TreeRow::Folder {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn rename_move_and_delete_folders() {
        let hosts = ["prod", "prod/eu", "production", "", "lab"];
        let explicit = vec!["prod/empty".to_owned(), "other".to_owned()];
        // Rename prod → live: subfolders follow, look-alikes do not.
        let c = move_folder(&hosts, &explicit, "prod", "live");
        assert_eq!(c.hosts, vec![(0, "live".into()), (1, "live/eu".into())]);
        assert_eq!(
            c.explicit,
            vec!["live/empty".to_owned(), "other".to_owned()]
        );
        // Move prod into lab.
        let c = move_folder(&hosts, &explicit, "prod", "lab/prod");
        assert_eq!(
            c.hosts,
            vec![(0, "lab/prod".into()), (1, "lab/prod/eu".into())]
        );
        // Delete prod: its hosts and subfolders move up to the root.
        let c = delete_folder(&hosts, &explicit, "prod");
        assert_eq!(c.hosts, vec![(0, String::new()), (1, "eu".into())]);
        assert_eq!(c.explicit, vec!["empty".to_owned(), "other".to_owned()]);
        // Delete a nested folder: into its parent; the parent is not kept
        // as an empty folder just because of that.
        let c = delete_folder(&hosts, &["prod/eu".to_owned()], "prod/eu");
        assert_eq!(c.hosts, vec![(1, "prod".into())]);
        assert!(c.explicit.is_empty());
        // An empty folder disappears.
        let c = delete_folder(&hosts, &explicit, "other");
        assert!(c.hosts.is_empty());
        assert_eq!(c.explicit, vec!["prod/empty".to_owned()]);
    }

    fn ssh_host() -> HostProfile {
        let mut p = HostProfile::new("web", "10.0.0.1");
        p.user = Some("deploy".into());
        p.auth = vec![
            AuthMethod::Agent,
            AuthMethod::KeyFile {
                path: "/k/old".into(),
                passphrase_credential: Some("cred-old".into()),
            },
            AuthMethod::KeyFile {
                path: "/k/keep".into(),
                passphrase_credential: Some("cred-keep".into()),
            },
            AuthMethod::Password { credential: None },
        ];
        p.proxy = Some(ProxySpec {
            kind: ProxyKind::Socks5,
            host: "px".into(),
            port: 1080,
            user: Some("u".into()),
            password_credential: Some("px-secret".into()),
        });
        p
    }

    #[test]
    fn bulk_edit_applies_only_changed_fields() {
        let mut p = ssh_host();
        let orig = p.clone();
        assert!(BulkEdit::default().is_empty());
        assert!(!BulkEdit::default().apply(&mut p));
        assert_eq!(p, orig);
        let edit = BulkEdit {
            port: Some(2222),
            keepalive: Some(15),
            ..Default::default()
        };
        assert!(edit.apply(&mut p));
        assert_eq!((p.port, p.keepalive_secs), (2222, 15));
        // Everything else untouched.
        assert_eq!(p.user, orig.user);
        assert_eq!(p.auth, orig.auth);
        assert_eq!(p.proxy, orig.proxy);
        // Same edit again: no change.
        assert!(!edit.apply(&mut p));
        // Clearing the user; folder normalized.
        let e = BulkEdit {
            user: Some(None),
            folder: Some("/prod//eu/".into()),
            ..Default::default()
        };
        assert!(e.apply(&mut p));
        assert_eq!(p.user, None);
        assert_eq!(p.folder, "prod/eu");
    }

    #[test]
    fn bulk_edit_keys_keep_order_and_saved_passphrases() {
        let mut p = ssh_host();
        let e = BulkEdit {
            keys: Some(vec!["/k/keep".into(), "/k/new".into()]),
            ..Default::default()
        };
        assert!(e.apply(&mut p));
        assert_eq!(
            p.auth,
            vec![
                AuthMethod::Agent,
                AuthMethod::KeyFile {
                    path: "/k/keep".into(),
                    passphrase_credential: Some("cred-keep".into())
                },
                AuthMethod::KeyFile {
                    path: "/k/new".into(),
                    passphrase_credential: None
                },
                AuthMethod::Password { credential: None },
            ]
        );
        // A host without keys gets them before the password.
        let auth = replace_keys(
            &[AuthMethod::Agent, AuthMethod::Password { credential: None }],
            &["/k/a".into()],
        );
        assert!(matches!(auth[1], AuthMethod::KeyFile { .. }));
        // Removing every key file.
        let auth = replace_keys(&ssh_host().auth, &[]);
        assert_eq!(
            auth,
            vec![AuthMethod::Agent, AuthMethod::Password { credential: None }]
        );
        assert_eq!(key_text(&ssh_host()), "/k/old, /k/keep");
        assert_eq!(
            parse_keys(" /a , ,/b"),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn bulk_edit_proxy_keeps_secret_only_for_same_proxy() {
        let mut p = ssh_host();
        let same = ProxySpec::parse_url("socks5://u@px:1081").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            BulkEdit {
                proxy: Some(Some(same)),
                ..Default::default()
            }
            .apply(&mut p)
        );
        assert_eq!(
            p.proxy
                .as_ref()
                .and_then(|x| x.password_credential.as_deref()),
            Some("px-secret")
        );
        let other = ProxySpec::parse_url("http://other:3128").unwrap_or_else(|e| panic!("{e}"));
        assert!(
            BulkEdit {
                proxy: Some(Some(other)),
                ..Default::default()
            }
            .apply(&mut p)
        );
        assert_eq!(
            p.proxy
                .as_ref()
                .and_then(|x| x.password_credential.as_deref()),
            None
        );
        assert!(
            BulkEdit {
                proxy: Some(None),
                ..Default::default()
            }
            .apply(&mut p)
        );
        assert_eq!(p.proxy, None);
    }

    #[test]
    fn bulk_edit_skips_what_a_protocol_does_not_have() {
        let edit = BulkEdit {
            user: Some(Some("root".into())),
            port: Some(2200),
            keys: Some(vec!["/k".into()]),
            jump: Some(vec![Endpoint {
                host: "bastion".into(),
                port: 22,
                user: None,
            }]),
            file_protocol: Some(FileProtocol::Scp),
            folder: Some("net".into()),
            ..Default::default()
        };
        let mut serial = HostProfile::new("console", "/dev/ttyUSB0");
        serial.protocol = Protocol::Serial(SerialSettings::default());
        let before = serial.clone();
        assert!(edit.apply(&mut serial));
        assert_eq!(serial.folder, "net");
        assert_eq!(
            (serial.port, &serial.user, &serial.auth),
            (before.port, &before.user, &before.auth)
        );
        let mut telnet = HostProfile::new("sw", "10.0.0.2");
        telnet.protocol = Protocol::Telnet;
        let before = telnet.clone();
        assert!(edit.apply(&mut telnet));
        assert_eq!((telnet.port, telnet.user.as_deref()), (2200, Some("root")));
        assert_eq!(
            (&telnet.auth, &telnet.jump, telnet.file_protocol),
            (&before.auth, &before.jump, before.file_protocol)
        );
        let mut ssh = ssh_host();
        assert!(edit.apply(&mut ssh));
        assert_eq!(ssh.jump.len(), 1);
        assert_eq!(ssh.file_protocol, FileProtocol::Scp);
    }

    #[test]
    fn recents_are_unique_newest_first_and_bounded() {
        let a = hostwick_core::ProfileId::new();
        let mut l = Vec::new();
        push_recent(&mut l, Recent::Host { id: a });
        push_recent(
            &mut l,
            Recent::Quick {
                text: "me@box".into(),
            },
        );
        push_recent(&mut l, Recent::Host { id: a });
        assert_eq!(
            l,
            vec![
                Recent::Host { id: a },
                Recent::Quick {
                    text: "me@box".into()
                }
            ]
        );
        for i in 0..50 {
            push_recent(
                &mut l,
                Recent::Quick {
                    text: format!("h{i}"),
                },
            );
        }
        assert_eq!(l.len(), RECENTS_KEEP);
        assert_eq!(l[0], Recent::Quick { text: "h49".into() });
        assert_eq!(recents_from_json(&recents_to_json(&l)), l);
        assert!(recents_from_json("junk").is_empty());
    }

    #[test]
    fn common_values() {
        assert_eq!(common([1, 1, 1].into_iter()), Some(1));
        assert_eq!(common([1, 2].into_iter()), None);
        assert_eq!(common(std::iter::empty::<u8>()), None);
    }
}
