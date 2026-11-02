use std::cmp::Ordering;

use hostwick_sftp::{EntryKind, RemoteFs};
use serde::{Deserialize, Serialize};

use crate::TransferError;

/// A file in a tree, relative to the tree root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub rel_path: String,
    pub size: u64,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "path", rename_all = "snake_case")]
pub enum SyncAction {
    /// Missing or different at the destination.
    Copy(String),
    /// Only at the destination (mirror mode).
    Delete(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Copy new and changed files; never delete.
    Update,
    /// Make the destination identical, deleting extra files.
    Mirror,
}

const MTIME_SLACK: u64 = 2;

/// Plan a one-way sync by merge-joining two path-sorted listings: O(n + m)
/// after sorting, with no hash maps of the whole tree.
///
/// A file is "changed" if sizes differ, or the source is newer than the
/// destination by more than the filesystem timestamp granularity.
pub fn plan_sync(src: &[FileInfo], dst: &[FileInfo], mode: SyncMode) -> Vec<SyncAction> {
    let mut s: Vec<&FileInfo> = src.iter().collect();
    let mut d: Vec<&FileInfo> = dst.iter().collect();
    s.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    d.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < s.len() || j < d.len() {
        let ord = match (s.get(i), d.get(j)) {
            (Some(a), Some(b)) => a.rel_path.cmp(&b.rel_path),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => break,
        };
        match ord {
            Ordering::Less => {
                out.push(SyncAction::Copy(s[i].rel_path.clone()));
                i += 1;
            }
            Ordering::Greater => {
                if mode == SyncMode::Mirror {
                    out.push(SyncAction::Delete(d[j].rel_path.clone()));
                }
                j += 1;
            }
            Ordering::Equal => {
                let (a, b) = (s[i], d[j]);
                let newer = match (a.modified, b.modified) {
                    (Some(x), Some(y)) => x > y + MTIME_SLACK,
                    _ => false,
                };
                if a.size != b.size || newer {
                    out.push(SyncAction::Copy(a.rel_path.clone()));
                }
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Recursively list regular files under `root` (iterative, no recursion
/// depth limit). Skips our own `.filepart` files and symlinks.
pub async fn walk(fs: &dyn RemoteFs, root: &str) -> Result<Vec<FileInfo>, TransferError> {
    let mut out = Vec::new();
    let mut stack = vec![String::new()];
    while let Some(rel_dir) = stack.pop() {
        let abs = if rel_dir.is_empty() {
            root.to_owned()
        } else {
            hostwick_sftp::join(root, &rel_dir)
        };
        for e in fs.list_dir(&abs).await? {
            let rel = if rel_dir.is_empty() {
                e.name.clone()
            } else {
                format!("{rel_dir}/{}", e.name)
            };
            match e.kind {
                EntryKind::Dir => stack.push(rel),
                EntryKind::File if !e.name.ends_with(crate::PART_SUFFIX) => {
                    out.push(FileInfo {
                        rel_path: rel,
                        size: e.size,
                        modified: e.modified,
                    });
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str, size: u64, m: u64) -> FileInfo {
        FileInfo {
            rel_path: p.into(),
            size,
            modified: Some(m),
        }
    }

    #[test]
    fn update_and_mirror() {
        let src = vec![
            f("a", 1, 100),
            f("b/c", 2, 200),
            f("d", 3, 300),
            f("e", 4, 400),
        ];
        let dst = vec![
            f("a", 1, 100),
            f("b/c", 2, 150),
            f("d", 9, 300),
            f("zzz", 1, 1),
            f("e", 4, 401),
        ];
        assert_eq!(
            plan_sync(&src, &dst, SyncMode::Update),
            vec![SyncAction::Copy("b/c".into()), SyncAction::Copy("d".into())]
        );
        assert_eq!(
            plan_sync(&src, &dst, SyncMode::Mirror),
            vec![
                SyncAction::Copy("b/c".into()),
                SyncAction::Copy("d".into()),
                SyncAction::Delete("zzz".into())
            ]
        );
    }

    #[test]
    fn mtime_within_granularity_is_same() {
        let src = vec![f("a", 1, 102)];
        let dst = vec![f("a", 1, 100)];
        assert!(plan_sync(&src, &dst, SyncMode::Update).is_empty());
    }

    #[test]
    fn empty_sides() {
        assert_eq!(
            plan_sync(&[f("x", 1, 1)], &[], SyncMode::Mirror),
            vec![SyncAction::Copy("x".into())]
        );
        assert_eq!(
            plan_sync(&[], &[f("x", 1, 1)], SyncMode::Mirror),
            vec![SyncAction::Delete("x".into())]
        );
        assert!(plan_sync(&[], &[f("x", 1, 1)], SyncMode::Update).is_empty());
    }
}
