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
