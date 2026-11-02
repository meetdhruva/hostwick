//! UI-independent pieces of the file browser: sorting, selection, path
//! helpers, recursive delete and the external editor command line.

use std::collections::HashSet;
use std::path::Path;

use hostwick_sftp::{Entry, FsError, RemoteFs, natural_cmp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    Permissions,
}

/// Folders first (like every file manager), then by `key`.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, ascending: bool) {
    entries.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Permissions => a.permissions.cmp(&b.permissions),
        }
        .then_with(|| natural_cmp(&a.name, &b.name));
        b.is_dir()
            .cmp(&a.is_dir())
            .then(if ascending { ord } else { ord.reverse() })
    });
}

/// Multi-selection over a list of names, with a cursor (the focused row)
/// and an anchor for Shift ranges. Kept by name so it survives a refresh.
#[derive(Debug, Default, Clone)]
pub struct Selection {
    pub names: HashSet<String>,
    pub cursor: Option<usize>,
    pub anchor: Option<usize>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.names.clear();
        self.cursor = None;
        self.anchor = None;
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Mouse click: plain = select only this; Ctrl/Cmd = toggle; Shift =
    /// range from the anchor (added to the selection with Ctrl too).
    pub fn click(&mut self, ix: usize, names: &[&str], toggle: bool, range: bool) {
        let Some(name) = names.get(ix) else {
            return;
        };
        if range {
            let anchor = self.anchor.unwrap_or(ix).min(names.len().saturating_sub(1));
            if !toggle {
                self.names.clear();
            }
            let (a, b) = if anchor <= ix {
                (anchor, ix)
            } else {
                (ix, anchor)
            };
            for n in &names[a..=b] {
                self.names.insert((*n).to_owned());
            }
        } else if toggle {
            if !self.names.remove(*name) {
                self.names.insert((*name).to_owned());
            }
            self.anchor = Some(ix);
        } else {
            self.names.clear();
            self.names.insert((*name).to_owned());
            self.anchor = Some(ix);
        }
        self.cursor = Some(ix);
    }

    /// Right-click: keep a selection that includes the row, else select it.
    pub fn context(&mut self, ix: usize, names: &[&str]) {
        if names.get(ix).is_some_and(|n| !self.names.contains(*n)) {
            self.click(ix, names, false, false);
        }
        self.cursor = Some(ix);
    }

    pub fn select_all(&mut self, names: &[&str]) {
        self.names = names.iter().map(|n| (*n).to_owned()).collect();
    }

    /// Arrow keys: move the cursor; with Shift, extend from the anchor.
    pub fn step(&mut self, delta: isize, names: &[&str], extend: bool) {
        if names.is_empty() {
            return;
        }
        let last = names.len() - 1;
        let ix = match self.cursor {
            // No cursor yet: as if it sat just above the first row.
            None => usize::try_from(delta - 1).unwrap_or(0).min(last),
            Some(c) => c.saturating_add_signed(delta).min(last),
        };
        if extend {
            if self.anchor.is_none() {
                self.anchor = self.cursor.or(Some(ix));
            }
            self.click(ix, names, false, true);
        } else {
            self.click(ix, names, false, false);
        }
    }

    /// After a reload: forget names that are gone, clamp the cursor.
    pub fn retain(&mut self, names: &[&str]) {
        let present: HashSet<&str> = names.iter().copied().collect();
        self.names.retain(|n| present.contains(n.as_str()));
        if let Some(c) = self.cursor
            && c >= names.len()
        {
            self.cursor = names.len().checked_sub(1);
        }
        self.anchor = self.anchor.filter(|a| *a < names.len());
    }
}

/// Parent folder, also for Windows drive paths (`C:/Users` → `C:/`).
/// `None` at the top.
pub fn parent_dir(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    // "C:" or "C:/" is a drive root.
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return None;
    }
    let p = hostwick_sftp::parent(trimmed);
    if p == "." {
        return None;
    }
    if p.len() == 2 && p.ends_with(':') {
        return Some(format!("{p}/"));
    }
    Some(p)
}

/// Name for "Save as copy": `a.txt` → `a (my copy).txt`, then `(my copy 2)`.
pub fn copy_name(name: &str, n: u32) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    if n <= 1 {
        format!("{stem} (my copy){ext}")
    } else {
        format!("{stem} (my copy {n}){ext}")
    }
}

/// Delete a file, or a folder and everything in it (files first, then
/// folders deepest first).
pub async fn remove_recursive(fs: &dyn RemoteFs, entry: &Entry) -> Result<(), FsError> {
    if !entry.is_dir() {
        return fs.remove_file(&entry.path).await;
    }
    let mut dirs = vec![entry.path.clone()];
    let mut stack = vec![entry.path.clone()];
    while let Some(d) = stack.pop() {
        for e in fs.list_dir(&d).await? {
            if e.is_dir() {
                dirs.push(e.path.clone());
                stack.push(e.path);
            } else {
                fs.remove_file(&e.path).await?;
            }
        }
    }
    for d in dirs.iter().rev() {
        fs.remove_dir(d).await?;
    }
    Ok(())
}

/// Program and arguments for the external editor setting: `{file}` is
/// replaced by the path, otherwise the path is appended. Double or single
/// quotes group words (`"C:\Program Files\Notepad++\notepad++.exe" -multiInst`).
pub fn editor_command(template: &str, file: &Path) -> Option<(String, Vec<String>)> {
    let file = file.to_string_lossy();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in template.trim().chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    if words.is_empty() {
        return None;
    }
    let mut placed = false;
    for w in &mut words {
        if w.contains("{file}") {
            *w = w.replace("{file}", &file);
            placed = true;
        }
    }
    if !placed {
        words.push(file.into_owned());
    }
    let program = words.remove(0);
    Some((program, words))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hostwick_sftp::EntryKind;

    fn e(name: &str, kind: EntryKind, size: u64, m: u64) -> Entry {
        Entry {
            name: name.into(),
            path: format!("/{name}"),
            kind,
            size,
            modified: Some(m),
            modified_nanos: None,
            permissions: Some(0o644),
            owner: None,
            group: None,
            uid: None,
            gid: None,
        }
    }

    #[test]
    fn sorting_keeps_folders_first() {
        let mut v = vec![
            e("b10", EntryKind::File, 5, 3),
            e("dir", EntryKind::Dir, 0, 1),
            e("b2", EntryKind::File, 50, 1),
            e("a", EntryKind::File, 7, 2),
        ];
        sort_entries(&mut v, SortKey::Name, true);
        let n: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(n, ["dir", "a", "b2", "b10"]);
        sort_entries(&mut v, SortKey::Size, false);
        let n: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(n, ["dir", "b2", "a", "b10"]);
        sort_entries(&mut v, SortKey::Modified, true);
        let n: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(n, ["dir", "b2", "a", "b10"]);
    }

    #[test]
    fn selection() {
        let names = ["a", "b", "c", "d", "e"];
        let mut s = Selection::default();
        s.click(1, &names, false, false);
        s.click(3, &names, false, true);
        assert_eq!(s.names.len(), 3);
        assert!(s.contains("b") && s.contains("c") && s.contains("d"));
        s.click(0, &names, true, false);
        assert_eq!(s.names.len(), 4);
        s.click(2, &names, true, false);
        assert!(!s.contains("c"));
        // Right-click inside the selection keeps it; outside replaces it.
        s.context(1, &names);
        assert_eq!(s.names.len(), 3);
        s.context(4, &names);
        assert_eq!(s.names.len(), 1);
        // Keyboard.
        s.step(-1, &names, false);
        assert_eq!((s.cursor, s.names.len()), (Some(3), 1));
        s.step(-2, &names, true);
        assert_eq!(s.names.len(), 3);
        s.step(-10, &names, false);
        assert_eq!(s.cursor, Some(0));
        s.select_all(&names);
        assert_eq!(s.names.len(), 5);
        // From no cursor: Down → first, End → last, Home → first.
        let mut t = Selection::default();
        t.step(1, &names, false);
        assert_eq!(t.cursor, Some(0));
        let mut t = Selection::default();
        t.step(5, &names, false);
        assert_eq!(t.cursor, Some(4));
        let mut t = Selection::default();
        t.step(-5, &names, false);
        assert_eq!(t.cursor, Some(0));
        s.retain(&["a", "x"]);
        assert_eq!((s.names.len(), s.cursor), (1, Some(0)));
    }

    #[test]
    fn parents() {
        assert_eq!(parent_dir("/a/b").as_deref(), Some("/a"));
        assert_eq!(parent_dir("/a").as_deref(), Some("/"));
        assert_eq!(parent_dir("/"), None);
        assert_eq!(parent_dir("C:/Users/me").as_deref(), Some("C:/Users"));
        assert_eq!(parent_dir("C:/Users").as_deref(), Some("C:/"));
        assert_eq!(parent_dir("C:/"), None);
    }

    #[test]
    fn copies() {
        assert_eq!(copy_name("a.txt", 1), "a (my copy).txt");
        assert_eq!(copy_name("a.tar.gz", 2), "a.tar (my copy 2).gz");
        assert_eq!(copy_name(".bashrc", 1), ".bashrc (my copy)");
        assert_eq!(copy_name("Makefile", 1), "Makefile (my copy)");
    }

    #[test]
    fn editor_commands() {
        let f = Path::new("/tmp/x y/a.txt");
        assert_eq!(
            editor_command("code --wait", f),
            Some((
                "code".into(),
                vec!["--wait".into(), "/tmp/x y/a.txt".into()]
            ))
        );
        assert_eq!(
            editor_command(
                r#""C:\Program Files\Notepad++\notepad++.exe" -multiInst "{file}""#,
                f
            ),
            Some((
                r"C:\Program Files\Notepad++\notepad++.exe".into(),
                vec!["-multiInst".into(), "/tmp/x y/a.txt".into()]
            ))
        );
        assert_eq!(
            editor_command("vim +{file}", f),
            Some(("vim".into(), vec!["+/tmp/x y/a.txt".into()]))
        );
        assert_eq!(editor_command("   ", f), None);
    }
}
