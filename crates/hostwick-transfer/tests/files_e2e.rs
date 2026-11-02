//! Two-way sync and SCP transfers against real servers. Skipped unless
//! `HOSTWICK_FILES_PORT` is set (see `scripts/e2e-files.sh`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, HostProfile, SecretString};
use hostwick_sftp::{LocalFs, RemoteFs, open_remote_fs};
use hostwick_ssh::{
    ConnectContext, Connection, HostKeyDecision, HostKeyInfo, Interaction, NoCredentials,
};
use hostwick_transfer::{
    ActionKind, EngineConfig, ExistingPolicy, Job, PlanOptions, SyncDirection, TransferEngine,
    TransferState, build_plan, execute_plan,
};
use tokio_util::sync::CancellationToken;

struct Accept;

#[async_trait]
impl Interaction for Accept {
    async fn host_key(&self, _: &HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::AcceptOnce
    }
    async fn password(&self, _: &str, _: &str) -> Option<SecretString> {
        None
    }
    async fn key_passphrase(&self, _: &Path) -> Option<SecretString> {
        None
    }
    async fn keyboard_interactive(
        &self,
        _: &str,
        _: &str,
        _: &[(String, bool)],
    ) -> Option<Vec<String>> {
        None
    }
}

async fn remote(port_var: &str) -> Option<(Connection, Arc<dyn RemoteFs>)> {
    let port: u16 = std::env::var(port_var).ok()?.parse().ok()?;
    let key = std::env::var("HOSTWICK_FILES_KEY").ok()?;
    let mut p = HostProfile::new("files-e2e", "127.0.0.1");
    p.port = port;
    p.user = Some("hwfiles2".into());
    p.auth = vec![AuthMethod::KeyFile {
        path: key.into(),
        passphrase_credential: None,
    }];
    let mut ctx = ConnectContext::new(Arc::new(Accept), Arc::new(NoCredentials));
    ctx.known_hosts = None;
    ctx.connect_timeout = Duration::from_secs(10);
    let conn = Connection::connect(&p, &ctx).await.unwrap();
    let fs = open_remote_fs(&conn, &p, None).await.unwrap();
    Some((conn, fs))
}

fn write(root: &Path, rel: &str, data: &[u8], mtime: u64) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, data).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(mtime))
        .unwrap();
}

async fn two_way(port_var: &str, tag: &str) {
    let Some((conn, remote)) = remote(port_var).await else {
        eprintln!("skipping: {port_var} not set");
        return;
    };
    let home = remote.canonicalize(".").await.unwrap();
    let root = format!("{home}/sync-{tag}");
    let _ = conn.exec(&format!("rm -rf '{root}'")).await;
    // Server side: one newer file, one only there.
    conn.exec(&format!(
        "mkdir -p '{root}/sub dir' && printf server > '{root}/theirs.txt' && touch -d @2000000000 '{root}/theirs.txt' \
         && printf old > '{root}/mine.txt' && touch -d @1000000000 '{root}/mine.txt' \
         && printf r > '{root}/sub dir/remote only.txt'"
    ))
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let l = dir.path();
    write(l, "mine.txt", b"local edit", 1_900_000_000);
    write(l, "theirs.txt", b"old", 1_000_000_000);
    write(l, "it's new.txt", b"new", 1_500_000_000);

    let local: Arc<dyn RemoteFs> = Arc::new(LocalFs::new(l));
    let plan = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        &root,
        PlanOptions {
            direction: SyncDirection::Both,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let got: Vec<_> = plan
        .items
        .iter()
        .map(|i| (i.rel_path.as_str(), i.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            ("it's new.txt", ActionKind::Upload),
            ("mine.txt", ActionKind::Upload),
            ("sub dir", ActionKind::Download),
            ("sub dir/remote only.txt", ActionKind::Download),
            ("theirs.txt", ActionKind::Download),
        ]
    );
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
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(std::fs::read(l.join("theirs.txt")).unwrap(), b"server");
    assert_eq!(
        std::fs::read(l.join("sub dir/remote only.txt")).unwrap(),
        b"r"
    );
    let out = conn
        .exec(&format!("cat '{root}/mine.txt' '{root}/it'\\''s new.txt'"))
        .await
        .unwrap();
    assert_eq!(out.stdout_lossy(), "local editnew");
    // Converged: a second plan has nothing to do.
    let again = build_plan(
        local.as_ref(),
        "/",
        remote.as_ref(),
        &root,
        PlanOptions {
            direction: SyncDirection::Both,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(again.is_empty(), "{:#?}", again.items);
    conn.exec(&format!("rm -rf '{root}'")).await.unwrap();
}

#[tokio::test]
async fn two_way_sync_over_sftp() {
    two_way("HOSTWICK_FILES_PORT", "sftp").await;
}

#[tokio::test]
async fn two_way_sync_over_scp() {
    two_way("HOSTWICK_FILES_NOSFTP_PORT", "scp").await;
}

/// A stale `.filepart` on an SCP server: the copy restarts from zero.
#[tokio::test]
async fn scp_upload_restarts_instead_of_resuming() {
    let Some((conn, remote)) = remote("HOSTWICK_FILES_NOSFTP_PORT").await else {
        eprintln!("skipping: HOSTWICK_FILES_NOSFTP_PORT not set");
        return;
    };
    let home = remote.canonicalize(".").await.unwrap();
    let content: Vec<u8> = (0..2_000_000u32).map(|i| (i % 249) as u8).collect();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bin"), &content).unwrap();
    conn.exec(&format!(
        "head -c 100000 /dev/zero > '{home}/big.bin.filepart'"
    ))
    .await
    .unwrap();
    let engine = TransferEngine::new(EngineConfig::default());
    let id = engine.enqueue(Job {
        src: Arc::new(LocalFs::new(dir.path())),
        src_path: "/big.bin".into(),
        dst: remote.clone(),
        dst_path: format!("{home}/big.bin"),
        policy: ExistingPolicy::Overwrite,
    });
    assert_eq!(
        engine.wait(id).await,
        Some(TransferState::Done {
            bytes: 2_000_000,
            resumed_from: 0
        })
    );
    let sum = conn
        .exec(&format!(
            "sha256sum '{home}/big.bin' && rm '{home}/big.bin'"
        ))
        .await
        .unwrap();
    let local_sum = remote_sum(&content);
    assert!(
        sum.stdout_lossy().starts_with(&local_sum),
        "{}",
        sum.stdout_lossy()
    );
}

fn remote_sum(data: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x"), data).unwrap();
    let out = std::process::Command::new("sha256sum")
        .arg(dir.path().join("x"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}
