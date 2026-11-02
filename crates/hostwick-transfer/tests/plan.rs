//! Sync plans executed between two local folders, and the restart path for
//! protocols that cannot resume.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use hostwick_sftp::{BoxRead, BoxWrite, Entry, FsError, LocalFs, RemoteFs};
use hostwick_transfer::{
    ActionKind, EngineConfig, ExistingPolicy, Job, PART_SUFFIX, PlanOptions, SyncDirection,
    TransferEngine, TransferState, build_plan, execute_plan,
};
use tokio_util::sync::CancellationToken;

fn write(root: &Path, rel: &str, data: &[u8], mtime: u64) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, data).unwrap();
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime);
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

#[tokio::test]
async fn two_way_sync_runs_the_checked_actions() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l, r) = (a.path(), b.path());
    write(l, "same.txt", b"same", 1_000);
    write(r, "same.txt", b"same", 1_000);
    write(l, "mine.txt", b"local edit", 2_000);
    write(r, "mine.txt", b"old", 1_000);
    write(l, "theirs.txt", b"old", 1_000);
    write(r, "theirs.txt", b"server edit", 2_000);
    write(l, "only/local.txt", b"L", 1_000);
    write(r, "only/remote.txt", b"R", 1_000);
    write(l, "clash.txt", b"aaaa", 1_000);
    write(r, "clash.txt", b"bbbbbbbb", 1_001);
    write(l, "unchecked.txt", b"skip me", 1_000);
    std::fs::create_dir_all(r.join("empty/deeper")).unwrap();

    let (local, remote): (Arc<dyn RemoteFs>, Arc<dyn RemoteFs>) =
        (Arc::new(LocalFs::new(l)), Arc::new(LocalFs::new(r)));
    let plan = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        "/",
        PlanOptions {
            direction: SyncDirection::Both,
            delete: true, // ignored for Both
            excludes: vec![],
        },
    )
    .await
    .unwrap();
    let summary: Vec<_> = plan
        .items
        .iter()
        .map(|i| (i.rel_path.as_str(), i.kind))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("clash.txt", ActionKind::Conflict),
            ("empty", ActionKind::Download),
            ("empty/deeper", ActionKind::Download),
            ("mine.txt", ActionKind::Upload),
            ("only/local.txt", ActionKind::Upload),
            ("only/remote.txt", ActionKind::Download),
            ("theirs.txt", ActionKind::Download),
            ("unchecked.txt", ActionKind::Upload),
        ]
    );
    let engine = TransferEngine::new(EngineConfig::default());
    let report = execute_plan(
        &engine,
        local.clone(),
        remote.clone(),
        &plan,
        |_, item| item.rel_path != "unchecked.txt",
        &CancellationToken::new(),
    )
    .await;
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(read(r, "mine.txt"), b"local edit");
    assert_eq!(read(l, "theirs.txt"), b"server edit");
    assert_eq!(read(r, "only/local.txt"), b"L");
    assert_eq!(read(l, "only/remote.txt"), b"R");
    assert!(l.join("empty/deeper").is_dir());
    // The conflict was left alone on both sides; the unchecked file too.
    assert_eq!(read(l, "clash.txt"), b"aaaa");
    assert_eq!(read(r, "clash.txt"), b"bbbbbbbb");
    assert!(!r.join("unchecked.txt").exists());
    // mtimes travel with the copies, so a second plan is quiet.
    let again = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        "/",
        PlanOptions {
            direction: SyncDirection::Both,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let left: Vec<_> = again.items.iter().map(|i| i.rel_path.as_str()).collect();
    assert_eq!(left, ["clash.txt", "unchecked.txt"]);
    assert_eq!(report.uploaded.len() + report.downloaded.len(), 4);
    assert!(report.summary().contains("uploaded"));
}

#[tokio::test]
async fn mirror_deletes_extra_files_and_folders_but_not_excluded_ones() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (l, r) = (a.path(), b.path());
    write(l, "keep.txt", b"k", 1_000);
    write(r, "keep.txt", b"k", 1_000);
    write(r, "gone.txt", b"g", 1_000);
    write(r, "old/dir/file.txt", b"x", 1_000);
    write(r, "cache/kept.tmp", b"excluded", 1_000);
    let (local, remote): (Arc<dyn RemoteFs>, Arc<dyn RemoteFs>) =
        (Arc::new(LocalFs::new(l)), Arc::new(LocalFs::new(r)));
    let plan = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        "/",
        PlanOptions {
            direction: SyncDirection::LocalToRemote,
            delete: true,
            excludes: vec!["*.tmp".into()],
        },
    )
    .await
    .unwrap();
    let t = plan.totals(|_, i| i.default_selected());
    // gone.txt, old/dir/file.txt, old/, old/dir/, cache/ (its file is excluded)
    assert_eq!(t.delete_remote, 5, "{:#?}", plan.items);
    let engine = TransferEngine::new(EngineConfig::default());
    let report = execute_plan(
        &engine,
        local,
        remote,
        &plan,
        |_, _| true,
        &CancellationToken::new(),
    )
    .await;
    assert!(!r.join("gone.txt").exists());
    assert!(!r.join("old").exists());
    // The folder holding an excluded file stays, and says why.
    assert_eq!(read(r, "cache/kept.tmp"), b"excluded");
    assert_eq!(report.failed.len(), 1, "{:?}", report.failed);
    assert_eq!(report.failed[0].0, "cache/");
}

#[tokio::test]
async fn missing_destination_root_is_created() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(a.path(), "x/y.txt", b"y", 1_000);
    let (local, remote): (Arc<dyn RemoteFs>, Arc<dyn RemoteFs>) = (
        Arc::new(LocalFs::new(a.path())),
        Arc::new(LocalFs::new(b.path())),
    );
    let plan = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        "/new/root",
        PlanOptions::default(),
    )
    .await
    .unwrap();
    let engine = TransferEngine::new(EngineConfig::default());
    let report = execute_plan(
        &engine,
        local,
        remote,
        &plan,
        |_, _| true,
        &CancellationToken::new(),
    )
    .await;
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(read(b.path(), "new/root/x/y.txt"), b"y");
}

/// Like SCP: whole files only.
struct WholeFiles(LocalFs);

#[async_trait]
impl RemoteFs for WholeFiles {
    async fn list_dir(&self, p: &str) -> Result<Vec<Entry>, FsError> {
        self.0.list_dir(p).await
    }
    async fn stat(&self, p: &str) -> Result<Option<Entry>, FsError> {
        self.0.stat(p).await
    }
    async fn mkdir(&self, p: &str) -> Result<(), FsError> {
        self.0.mkdir(p).await
    }
    async fn remove_file(&self, p: &str) -> Result<(), FsError> {
        self.0.remove_file(p).await
    }
    async fn remove_dir(&self, p: &str) -> Result<(), FsError> {
        self.0.remove_dir(p).await
    }
    async fn rename(&self, a: &str, b: &str) -> Result<(), FsError> {
        self.0.rename(a, b).await
    }
    async fn open_read(&self, p: &str, off: u64) -> Result<BoxRead, FsError> {
        if off > 0 {
            return Err(FsError::Unsupported("resume".into()));
        }
        self.0.open_read(p, 0).await
    }
    async fn open_write(&self, p: &str, off: u64) -> Result<BoxWrite, FsError> {
        if off > 0 {
            return Err(FsError::Unsupported("resume".into()));
        }
        self.0.open_write(p, 0).await
    }
    async fn set_modified(&self, p: &str, t: u64) -> Result<(), FsError> {
        self.0.set_modified(p, t).await
    }
    async fn canonicalize(&self, p: &str) -> Result<String, FsError> {
        self.0.canonicalize(p).await
    }
}

#[tokio::test]
async fn protocols_without_resume_restart_from_zero() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let content: Vec<u8> = (0..500_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(a.path().join("f"), &content).unwrap();
    // A matching partial that a resumable protocol would continue.
    std::fs::write(
        b.path().join(format!("f{PART_SUFFIX}")),
        &content[..200_000],
    )
    .unwrap();
    for (src, dst) in [
        (
            Arc::new(LocalFs::new(a.path())) as Arc<dyn RemoteFs>,
            Arc::new(WholeFiles(LocalFs::new(b.path()))) as Arc<dyn RemoteFs>,
        ),
        (
            Arc::new(WholeFiles(LocalFs::new(a.path()))) as Arc<dyn RemoteFs>,
            Arc::new(LocalFs::new(b.path())) as Arc<dyn RemoteFs>,
        ),
    ] {
        std::fs::write(
            b.path().join(format!("f{PART_SUFFIX}")),
            &content[..200_000],
        )
        .unwrap();
        let engine = TransferEngine::new(EngineConfig::default());
        let id = engine.enqueue(Job {
            src,
            src_path: "/f".into(),
            dst,
            dst_path: "/f".into(),
            policy: ExistingPolicy::Overwrite,
        });
        let state = engine.wait(id).await;
        assert_eq!(
            state,
            Some(TransferState::Done {
                bytes: 500_000,
                resumed_from: 0
            })
        );
        assert_eq!(std::fs::read(b.path().join("f")).unwrap(), content);
    }
}

/// A server whose listing names an entry `../evil` (a malicious SFTP
/// server; on Windows a plain Linux file called `..\..\x` does the same),
/// and that records the order of changes.
struct Hostile {
    inner: LocalFs,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Hostile {
    fn new(root: &Path) -> Self {
        Self {
            inner: LocalFs::new(root),
            log: Arc::default(),
        }
    }
    fn note(&self, what: String) {
        self.log.lock().unwrap().push(what);
    }
}

#[async_trait]
impl RemoteFs for Hostile {
    async fn list_dir(&self, p: &str) -> Result<Vec<Entry>, FsError> {
        let mut v = self.inner.list_dir(p).await?;
        if p == "/"
            && let Some(mut e) = v.first().cloned()
        {
            e.name = "../evil".into();
            e.path = "/../evil".into();
            e.kind = hostwick_sftp::EntryKind::File;
            v.push(e);
        }
        Ok(v)
    }
    async fn stat(&self, p: &str) -> Result<Option<Entry>, FsError> {
        self.inner.stat(p).await
    }
    async fn mkdir(&self, p: &str) -> Result<(), FsError> {
        self.inner.mkdir(p).await
    }
    async fn remove_file(&self, p: &str) -> Result<(), FsError> {
        self.note(format!("rm {p}"));
        self.inner.remove_file(p).await
    }
    async fn remove_dir(&self, p: &str) -> Result<(), FsError> {
        self.note(format!("rmdir {p}"));
        self.inner.remove_dir(p).await
    }
    async fn rename(&self, a: &str, b: &str) -> Result<(), FsError> {
        self.inner.rename(a, b).await
    }
    async fn open_read(&self, p: &str, off: u64) -> Result<BoxRead, FsError> {
        self.inner.open_read(p, off).await
    }
    async fn open_write(&self, p: &str, off: u64) -> Result<BoxWrite, FsError> {
        self.note(format!("write {p}"));
        self.inner.open_write(p, off).await
    }
    async fn set_modified(&self, p: &str, t: u64) -> Result<(), FsError> {
        self.inner.set_modified(p, t).await
    }
    async fn canonicalize(&self, p: &str) -> Result<String, FsError> {
        self.inner.canonicalize(p).await
    }
}

#[tokio::test]
async fn unsafe_remote_names_never_leave_the_local_root() {
    let t = tempfile::tempdir().unwrap();
    let (server, local_root) = (t.path().join("server"), t.path().join("home/sync"));
    std::fs::create_dir_all(&local_root).unwrap();
    write(&server, "normal.txt", b"ok", 1_000);
    std::fs::write(t.path().join("evil"), b"PWNED").unwrap();
    let remote: Arc<dyn RemoteFs> = Arc::new(Hostile::new(&server));
    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::system());
    let lr = hostwick_sftp::path_to_slash(&local_root);
    let plan = build_plan(
        local.as_ref(),
        &lr,
        remote.as_ref(),
        "/",
        PlanOptions {
            direction: SyncDirection::RemoteToLocal,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(plan.items.iter().all(|i| !i.rel_path.contains("..")));
    assert_eq!(plan.skipped.len(), 1, "{:?}", plan.skipped);
    let engine = TransferEngine::new(EngineConfig::default());
    let report = execute_plan(
        &engine,
        local.clone(),
        remote.clone(),
        &plan,
        |_, _| true,
        &CancellationToken::new(),
    )
    .await;
    assert!(!t.path().join("home/evil").exists());
    assert_eq!(read(&local_root, "normal.txt"), b"ok");
    assert!(report.failed.iter().any(|(p, _)| p.contains("../evil")));

    // Recursive download: same.
    let dest = hostwick_sftp::path_to_slash(&t.path().join("home/dl"));
    let ids = engine
        .enqueue_tree(remote, "/", local, &dest, ExistingPolicy::Overwrite)
        .await
        .unwrap();
    for id in ids {
        engine.wait(id).await;
    }
    assert!(!t.path().join("home/evil").exists());
    assert!(t.path().join("home/dl/normal.txt").exists());
}

/// A typo in the source folder must not plan to delete the destination.
#[tokio::test]
async fn one_way_sync_refuses_a_missing_source() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(a.path(), "README", b"precious", 1_000);
    let (local, remote): (Arc<dyn RemoteFs>, Arc<dyn RemoteFs>) = (
        Arc::new(LocalFs::new(a.path())),
        Arc::new(LocalFs::new(b.path())),
    );
    for (direction, l, r) in [
        (SyncDirection::RemoteToLocal, "/", "/projetcs"),
        (SyncDirection::LocalToRemote, "/nope", "/"),
    ] {
        let err = build_plan(
            local.as_ref(),
            l,
            remote.as_ref(),
            r,
            PlanOptions {
                direction,
                delete: true,
                excludes: vec![],
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, hostwick_transfer::TransferError::SourceMissing(_)),
            "{err}"
        );
    }
    assert_eq!(read(a.path(), "README"), b"precious");
}

/// Mirror deletions run before copies, so a case-only rename (`File.txt`
/// deleted, `file.txt` copied) cannot delete the fresh copy on a
/// case-insensitive disk.
#[tokio::test]
async fn mirror_deletes_before_copying() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    write(a.path(), "File.txt", b"old name", 1_000);
    write(b.path(), "file.txt", b"new name", 2_000);
    let local = Arc::new(Hostile::new(a.path()));
    let log = local.log.clone();
    let (local, remote): (Arc<dyn RemoteFs>, Arc<dyn RemoteFs>) =
        (local, Arc::new(LocalFs::new(b.path())));
    let plan = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        "/",
        PlanOptions {
            direction: SyncDirection::RemoteToLocal,
            delete: true,
            excludes: vec![],
        },
    )
    .await
    .unwrap();
    let engine = TransferEngine::new(EngineConfig::default());
    let report = execute_plan(
        &engine,
        local,
        remote,
        &plan,
        |_, _| true,
        &CancellationToken::new(),
    )
    .await;
    // (The hostile listing's `../evil` is reported as skipped.)
    assert!(
        report.failed.iter().all(|(p, _)| p.contains("../evil")),
        "{:?}",
        report.failed
    );
    let log = log.lock().unwrap().clone();
    let rm = log.iter().position(|l| l == "rm /File.txt").unwrap();
    let wr = log.iter().position(|l| l.starts_with("write")).unwrap();
    assert!(rm < wr, "{log:?}");
}
