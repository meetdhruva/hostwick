#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use hostwick_sftp::{BoxRead, BoxWrite, Entry, FsError, LocalFs, RemoteFs};
use hostwick_transfer::{
    EngineConfig, ExistingPolicy, Job, PART_SUFFIX, SyncMode, TransferEngine, TransferState,
    plan_sync, walk,
};
use tokio::io::{AsyncRead, ReadBuf};

fn data(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn fs(dir: &std::path::Path) -> Arc<dyn RemoteFs> {
    Arc::new(LocalFs::new(dir))
}

fn job(
    src: &Arc<dyn RemoteFs>,
    sp: &str,
    dst: &Arc<dyn RemoteFs>,
    dp: &str,
    policy: ExistingPolicy,
) -> Job {
    Job {
        src: src.clone(),
        src_path: sp.into(),
        dst: dst.clone(),
        dst_path: dp.into(),
        policy,
    }
}

#[tokio::test]
async fn copies_a_tree_and_preserves_mtime() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::create_dir_all(a.path().join("x/y")).unwrap();
    std::fs::write(a.path().join("top.txt"), b"top").unwrap();
    std::fs::write(a.path().join("x/y/deep.bin"), data(3_000_000, 1)).unwrap();
    std::fs::write(a.path().join("x/empty"), b"").unwrap();
    let (src, dst) = (fs(a.path()), fs(b.path()));
    src.set_modified("/top.txt", 1_500_000_000).await.unwrap();

    let engine = TransferEngine::new(EngineConfig::default());
    let ids = engine
        .enqueue_tree(
            src.clone(),
            "/",
            dst.clone(),
            "/copy",
            ExistingPolicy::Overwrite,
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 3);
    for id in ids {
        assert!(matches!(
            engine.wait(id).await,
            Some(TransferState::Done { .. })
        ));
    }
    assert_eq!(
        std::fs::read(b.path().join("copy/x/y/deep.bin")).unwrap(),
        data(3_000_000, 1)
    );
    assert_eq!(std::fs::read(b.path().join("copy/x/empty")).unwrap(), b"");
    assert_eq!(
        dst.stat("/copy/top.txt").await.unwrap().unwrap().modified,
        Some(1_500_000_000)
    );
    // No leftovers.
    assert!(
        walk(dst.as_ref(), "/copy")
            .await
            .unwrap()
            .iter()
            .all(|f| !f.rel_path.ends_with(PART_SUFFIX))
    );
    // Source and destination are now in sync.
    let (s, d) = (
        walk(src.as_ref(), "/").await.unwrap(),
        walk(dst.as_ref(), "/copy").await.unwrap(),
    );
    assert!(plan_sync(&s, &d, SyncMode::Mirror).is_empty());
}

#[tokio::test]
async fn resumes_a_matching_partial_and_restarts_a_stale_one() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let content = data(1_000_000, 7);
    std::fs::write(a.path().join("f"), &content).unwrap();
    let (src, dst) = (fs(a.path()), fs(b.path()));
    let engine = TransferEngine::new(EngineConfig::default());

    // Matching first 400 kB.
    std::fs::write(
        b.path().join(format!("f{PART_SUFFIX}")),
        &content[..400_000],
    )
    .unwrap();
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::Overwrite));
    assert_eq!(
        engine.wait(id).await,
        Some(TransferState::Done {
            bytes: 600_000,
            resumed_from: 400_000
        })
    );
    assert_eq!(std::fs::read(b.path().join("f")).unwrap(), content);

    // A partial that does not match the source must not be spliced in.
    std::fs::write(b.path().join(format!("g{PART_SUFFIX}")), data(400_000, 99)).unwrap();
    std::fs::write(a.path().join("g"), &content).unwrap();
    let id = engine.enqueue(job(&src, "/g", &dst, "/g", ExistingPolicy::Overwrite));
    assert_eq!(
        engine.wait(id).await,
        Some(TransferState::Done {
            bytes: 1_000_000,
            resumed_from: 0
        })
    );
    assert_eq!(std::fs::read(b.path().join("g")).unwrap(), content);
}

#[tokio::test]
async fn skip_policies() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(a.path().join("f"), b"new").unwrap();
    std::fs::write(b.path().join("f"), b"old").unwrap();
    let (src, dst) = (fs(a.path()), fs(b.path()));
    let engine = TransferEngine::new(EngineConfig::default());

    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::Skip));
    assert_eq!(engine.wait(id).await, Some(TransferState::Skipped));
    assert_eq!(std::fs::read(b.path().join("f")).unwrap(), b"old");

    // Same size but different mtime -> copied.
    src.set_modified("/f", 2_000_000_000).await.unwrap();
    dst.set_modified("/f", 1_000_000_000).await.unwrap();
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::SkipIfSame));
    assert!(matches!(
        engine.wait(id).await,
        Some(TransferState::Done { .. })
    ));
    // Now identical -> skipped.
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::SkipIfSame));
    assert_eq!(engine.wait(id).await, Some(TransferState::Skipped));
}

#[tokio::test]
async fn cancel_leaves_a_resumable_partial() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(a.path().join("f"), data(4_000_000, 3)).unwrap();
    let (src, dst) = (fs(a.path()), fs(b.path()));
    // 2 MB/s: the copy takes ~2 s, so we can cancel midway.
    let engine = TransferEngine::new(EngineConfig {
        rate_limit: Some(2_000_000),
        ..Default::default()
    });
    let mut events = engine.subscribe();
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::Overwrite));
    // Wait for real progress, then cancel.
    loop {
        if let Ok(hostwick_transfer::TransferEvent::Progress { done, .. }) = events.recv().await
            && done > 500_000
        {
            break;
        }
    }
    engine.cancel(id);
    assert_eq!(engine.wait(id).await, Some(TransferState::Cancelled));
    assert!(!b.path().join("f").exists(), "incomplete file published");
    let part = std::fs::metadata(b.path().join(format!("f{PART_SUFFIX}")))
        .unwrap()
        .len();
    assert!(part > 0 && part < 4_000_000);

    let engine = TransferEngine::new(EngineConfig::default());
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::Overwrite));
    let Some(TransferState::Done { resumed_from, .. }) = engine.wait(id).await else {
        panic!()
    };
    assert_eq!(resumed_from, part);
    assert_eq!(
        std::fs::read(b.path().join("f")).unwrap(),
        data(4_000_000, 3)
    );
}

/// Fails the first read stream mid-file, like a dropped connection.
struct Flaky {
    inner: LocalFs,
    tripped: AtomicBool,
}

struct FailAfter {
    inner: BoxRead,
    left: usize,
}

impl AsyncRead for FailAfter {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.left == 0 {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "simulated drop",
            )));
        }
        let max = self.left.min(buf.remaining());
        let mut tmp = ReadBuf::new(&mut buf.initialize_unfilled()[..max]);
        let r = self.inner.as_mut().poll_read(cx, &mut tmp);
        let n = tmp.filled().len();
        buf.advance(n);
        self.left -= n;
        r
    }
}

#[async_trait]
impl RemoteFs for Flaky {
    async fn list_dir(&self, p: &str) -> Result<Vec<Entry>, FsError> {
        self.inner.list_dir(p).await
    }
    async fn stat(&self, p: &str) -> Result<Option<Entry>, FsError> {
        self.inner.stat(p).await
    }
    async fn mkdir(&self, p: &str) -> Result<(), FsError> {
        self.inner.mkdir(p).await
    }
    async fn remove_file(&self, p: &str) -> Result<(), FsError> {
        self.inner.remove_file(p).await
    }
    async fn remove_dir(&self, p: &str) -> Result<(), FsError> {
        self.inner.remove_dir(p).await
    }
    async fn rename(&self, a: &str, b: &str) -> Result<(), FsError> {
        self.inner.rename(a, b).await
    }
    async fn open_read(&self, p: &str, off: u64) -> Result<BoxRead, FsError> {
        let r = self.inner.open_read(p, off).await?;
        // Only the full-stream read trips; small verification reads pass.
        if off == 0 && !self.tripped.swap(true, Ordering::SeqCst) {
            Ok(Box::pin(FailAfter {
                inner: r,
                left: 700_000,
            }))
        } else {
            Ok(r)
        }
    }
    async fn open_write(&self, p: &str, off: u64) -> Result<BoxWrite, FsError> {
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
async fn transient_failure_retries_and_resumes() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let content = data(2_000_000, 5);
    std::fs::write(a.path().join("f"), &content).unwrap();
    let src: Arc<dyn RemoteFs> = Arc::new(Flaky {
        inner: LocalFs::new(a.path()),
        tripped: AtomicBool::new(false),
    });
    let dst = fs(b.path());
    let engine = TransferEngine::new(EngineConfig::default());
    let id = engine.enqueue(job(&src, "/f", &dst, "/f", ExistingPolicy::Overwrite));
    let state = tokio::time::timeout(Duration::from_secs(20), engine.wait(id))
        .await
        .unwrap();
    let Some(TransferState::Done { resumed_from, .. }) = state else {
        panic!("{state:?}")
    };
    assert!(resumed_from > 0, "retry should resume, not restart");
    assert_eq!(std::fs::read(b.path().join("f")).unwrap(), content);
}

#[tokio::test]
async fn missing_source_fails_without_retry() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = TransferEngine::new(EngineConfig::default());
    let id = engine.enqueue(job(
        &fs(a.path()),
        "/nope",
        &fs(b.path()),
        "/nope",
        ExistingPolicy::Overwrite,
    ));
    let state = tokio::time::timeout(Duration::from_secs(2), engine.wait(id))
        .await
        .unwrap();
    assert!(
        matches!(state, Some(TransferState::Failed { .. })),
        "{state:?}"
    );
}
