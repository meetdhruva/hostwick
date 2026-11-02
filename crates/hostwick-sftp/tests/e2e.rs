//! End-to-end tests against a real OpenSSH server.
//!
//! Skipped unless `HOSTWICK_E2E_PORT` is set; `scripts/e2e.sh` starts a
//! throwaway sshd and sets the variables.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, Endpoint, ForwardSpec, HostProfile, SecretString};
use hostwick_sftp::{RemoteFs, SftpFs, SftpOptions};
use hostwick_ssh::{
    ConnectContext, Connection, ConnectionPool, HostKeyDecision, HostKeyInfo, Interaction,
    KnownHosts, NoCredentials, PtyOptions, ShellCommand, ShellEvent, SshError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Env {
    host: String,
    port: u16,
    user: String,
    password: String,
    key: PathBuf,
    key_enc: PathBuf,
    key_enc_pass: String,
}

fn env() -> Option<Env> {
    let port = std::env::var("HOSTWICK_E2E_PORT").ok()?.parse().ok()?;
    let v = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"));
    Some(Env {
        host: std::env::var("HOSTWICK_E2E_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        user: v("HOSTWICK_E2E_USER"),
        password: v("HOSTWICK_E2E_PASSWORD"),
        key: v("HOSTWICK_E2E_KEY").into(),
        key_enc: v("HOSTWICK_E2E_KEY_ENC").into(),
        key_enc_pass: v("HOSTWICK_E2E_KEY_ENC_PASS"),
    })
}

macro_rules! require_env {
    () => {
        match env() {
            Some(e) => e,
            None => {
                eprintln!("skipping: HOSTWICK_E2E_PORT not set");
                return;
            }
        }
    };
}

/// Scripted answers; counts how often each prompt fired.
struct Scripted {
    decision: HostKeyDecision,
    password: Option<String>,
    passphrase: Option<String>,
    host_key_prompts: AtomicUsize,
    password_prompts: AtomicUsize,
}

impl Scripted {
    fn new(decision: HostKeyDecision) -> Self {
        Self {
            decision,
            password: None,
            passphrase: None,
            host_key_prompts: AtomicUsize::new(0),
            password_prompts: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Interaction for Scripted {
    async fn host_key(&self, info: &HostKeyInfo) -> HostKeyDecision {
        assert!(info.fingerprint.starts_with("SHA256:"));
        self.host_key_prompts.fetch_add(1, Ordering::SeqCst);
        self.decision
    }
    async fn password(&self, _: &str, _: &str) -> Option<SecretString> {
        self.password_prompts.fetch_add(1, Ordering::SeqCst);
        self.password.clone().map(SecretString::from)
    }
    async fn key_passphrase(&self, _: &Path) -> Option<SecretString> {
        self.passphrase.clone().map(SecretString::from)
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

fn profile(e: &Env, auth: Vec<AuthMethod>) -> HostProfile {
    let mut p = HostProfile::new("e2e", &e.host);
    p.port = e.port;
    p.user = Some(e.user.clone());
    p.auth = auth;
    p
}

fn ctx(ix: Arc<Scripted>, kh: &Path) -> ConnectContext {
    let mut c = ConnectContext::new(ix, Arc::new(NoCredentials));
    c.known_hosts = Some(KnownHosts::new(kh));
    c.connect_timeout = Duration::from_secs(10);
    c
}

fn key_auth(e: &Env) -> Vec<AuthMethod> {
    vec![AuthMethod::KeyFile {
        path: e.key.clone(),
        passphrase_credential: None,
    }]
}

async fn key_conn(e: &Env, kh: &Path) -> Connection {
    let ix = Arc::new(Scripted::new(HostKeyDecision::AcceptAndRemember));
    Connection::connect(&profile(e, key_auth(e)), &ctx(ix, kh))
        .await
        .unwrap()
}

#[tokio::test]
async fn password_auth_and_exec() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Scripted::new(HostKeyDecision::AcceptOnce);
    s.password = Some(e.password.clone());
    let ix = Arc::new(s);
    let conn = Connection::connect(
        &profile(&e, vec![AuthMethod::Password { credential: None }]),
        &ctx(ix.clone(), &dir.path().join("kh")),
    )
    .await
    .unwrap();
    let out = conn.exec("echo out; echo err >&2; exit 3").await.unwrap();
    assert_eq!(out.stdout_lossy(), "out\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n");
    assert_eq!(out.exit_status, Some(3));
    assert_eq!(ix.password_prompts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wrong_password_fails_cleanly() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Scripted::new(HostKeyDecision::AcceptOnce);
    s.password = Some("definitely-wrong".into());
    let err = Connection::connect(
        &profile(&e, vec![AuthMethod::Password { credential: None }]),
        &ctx(Arc::new(s), &dir.path().join("kh")),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SshError::AuthFailed { .. }), "{err}");
}

#[tokio::test]
async fn encrypted_key_prompts_for_passphrase() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Scripted::new(HostKeyDecision::AcceptOnce);
    s.passphrase = Some(e.key_enc_pass.clone());
    let auth = vec![AuthMethod::KeyFile {
        path: e.key_enc.clone(),
        passphrase_credential: None,
    }];
    let conn = Connection::connect(
        &profile(&e, auth),
        &ctx(Arc::new(s), &dir.path().join("kh")),
    )
    .await
    .unwrap();
    assert_eq!(
        conn.exec("whoami").await.unwrap().stdout_lossy().trim(),
        e.user
    );
}

#[tokio::test]
async fn missing_key_falls_through_to_next_method() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let mut s = Scripted::new(HostKeyDecision::AcceptOnce);
    s.password = Some(e.password.clone());
    let auth = vec![
        AuthMethod::KeyFile {
            path: "/nonexistent/key".into(),
            passphrase_credential: None,
        },
        AuthMethod::Password { credential: None },
    ];
    Connection::connect(
        &profile(&e, auth),
        &ctx(Arc::new(s), &dir.path().join("kh")),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn host_key_tofu_and_rejection() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let kh = dir.path().join("known_hosts");

    let ix = Arc::new(Scripted::new(HostKeyDecision::Reject));
    let err = Connection::connect(&profile(&e, key_auth(&e)), &ctx(ix, &kh))
        .await
        .unwrap_err();
    assert!(matches!(err, SshError::HostKeyRejected(_)), "{err}");

    let ix = Arc::new(Scripted::new(HostKeyDecision::AcceptAndRemember));
    Connection::connect(&profile(&e, key_auth(&e)), &ctx(ix.clone(), &kh))
        .await
        .unwrap();
    assert_eq!(ix.host_key_prompts.load(Ordering::SeqCst), 1);

    // Known now: a "reject everything" verifier is never consulted.
    let ix = Arc::new(Scripted::new(HostKeyDecision::Reject));
    Connection::connect(&profile(&e, key_auth(&e)), &ctx(ix.clone(), &kh))
        .await
        .unwrap();
    assert_eq!(ix.host_key_prompts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn interactive_shell_round_trip() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let conn = key_conn(&e, &dir.path().join("kh")).await;
    let mut sh = conn
        .open_shell(PtyOptions {
            cols: 100,
            rows: 30,
            ..Default::default()
        })
        .await
        .unwrap();
    sh.send(ShellCommand::Resize {
        cols: 120,
        rows: 40,
    })
    .await
    .unwrap();
    sh.send(ShellCommand::Data(
        "stty size; echo marker-$((6*7)); exit 7\n".into(),
    ))
    .await
    .unwrap();
    let mut seen = Vec::new();
    let mut exit = None;
    let deadline = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(ev) = sh.recv().await {
            match ev {
                ShellEvent::Stdout(b) | ShellEvent::Stderr(b) => seen.extend_from_slice(&b),
                ShellEvent::Exit(c) => exit = Some(c),
                ShellEvent::ExitSignal(_) => {}
                ShellEvent::Closed => break,
            }
        }
    });
    deadline.await.unwrap();
    let text = String::from_utf8_lossy(&seen);
    assert!(text.contains("marker-42"), "{text}");
    assert!(text.contains("40 120"), "resize not applied: {text}");
    assert_eq!(exit, Some(7));
}

async fn echo_server() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

async fn assert_echo(mut s: tokio::net::TcpStream) {
    s.write_all(b"ping-through-tunnel").await.unwrap();
    let mut buf = [0u8; 19];
    tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf, b"ping-through-tunnel");
}

#[tokio::test]
async fn local_dynamic_and_remote_forwards() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let conn = key_conn(&e, &dir.path().join("kh")).await;
    let echo = echo_server().await;

    // -L
    let l = conn
        .start_forward(&ForwardSpec::Local {
            bind_addr: "127.0.0.1".into(),
            bind_port: 0,
            host: "127.0.0.1".into(),
            host_port: echo,
        })
        .await
        .unwrap();
    assert_echo(
        tokio::net::TcpStream::connect(l.local_addr().unwrap())
            .await
            .unwrap(),
    )
    .await;

    // -D (SOCKS5 CONNECT with a domain name)
    let d = conn
        .start_forward(&ForwardSpec::Dynamic {
            bind_addr: "127.0.0.1".into(),
            bind_port: 0,
        })
        .await
        .unwrap();
    let mut s = tokio::net::TcpStream::connect(d.local_addr().unwrap())
        .await
        .unwrap();
    s.write_all(&[5, 1, 0]).await.unwrap();
    let mut r = [0u8; 2];
    s.read_exact(&mut r).await.unwrap();
    assert_eq!(r, [5, 0]);
    let host = b"localhost";
    let mut req = vec![5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host);
    req.extend_from_slice(&echo.to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0, "SOCKS reply {rep:?}");
    assert_echo(s).await;

    // -R with a server-assigned port
    let r = conn
        .start_forward(&ForwardSpec::Remote {
            bind_addr: "127.0.0.1".into(),
            bind_port: 0,
            host: "127.0.0.1".into(),
            host_port: echo,
        })
        .await
        .unwrap();
    let port = r.remote_port().unwrap();
    assert!(port > 0);
    // The test server is this machine, so its listener is reachable here.
    assert_echo(
        tokio::net::TcpStream::connect(("127.0.0.1", port as u16))
            .await
            .unwrap(),
    )
    .await;

    drop(l);
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn jump_host_chain() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let mut p = profile(&e, key_auth(&e));
    // Jump through the same server twice: exercises connect-over-channel.
    p.jump = vec![
        Endpoint {
            host: e.host.clone(),
            port: e.port,
            user: Some(e.user.clone()),
        },
        Endpoint {
            host: e.host.clone(),
            port: e.port,
            user: Some(e.user.clone()),
        },
    ];
    let ix = Arc::new(Scripted::new(HostKeyDecision::AcceptAndRemember));
    let conn = Connection::connect(&p, &ctx(ix.clone(), &dir.path().join("kh")))
        .await
        .unwrap();
    assert_eq!(
        conn.exec("echo via-jump").await.unwrap().stdout_lossy(),
        "via-jump\n"
    );
    // Same host:port each hop, so only the first hop asks.
    assert_eq!(ix.host_key_prompts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn pool_shares_one_connection_and_one_prompt() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let ix = Arc::new(Scripted::new(HostKeyDecision::AcceptOnce));
    let c = ctx(ix.clone(), &dir.path().join("kh"));
    let p = profile(&e, key_auth(&e));
    let pool = ConnectionPool::new();
    let (a, b) = tokio::join!(
        pool.get_or_connect(&p, &c, None),
        pool.get_or_connect(&p, &c, None)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(ix.host_key_prompts.load(Ordering::SeqCst), 1);
    // Terminal and SFTP on the same link.
    let fs = SftpFs::open(&b, &SftpOptions::default()).await.unwrap();
    assert!(fs.canonicalize(".").await.unwrap().starts_with('/'));
    assert_eq!(
        a.exec("echo shared").await.unwrap().stdout_lossy(),
        "shared\n"
    );
    pool.disconnect_all().await;
}

#[tokio::test]
async fn sftp_file_operations_and_large_transfer() {
    let e = require_env!();
    let dir = tempfile::tempdir().unwrap();
    let conn = key_conn(&e, &dir.path().join("kh")).await;
    let fs = SftpFs::open(&conn, &SftpOptions::default()).await.unwrap();

    let home = fs.canonicalize(".").await.unwrap();
    let base = format!("{home}/hw-e2e-{}", std::process::id());
    fs.mkdir_all(&format!("{base}/a/b")).await.unwrap();
    assert!(
        fs.stat(&format!("{base}/a/b"))
            .await
            .unwrap()
            .unwrap()
            .is_dir()
    );
    assert!(fs.stat(&format!("{base}/missing")).await.unwrap().is_none());

    // 24 MiB of pseudo-random data through the pipelined writer and reader.
    let size = 24 * 1024 * 1024;
    let data: Vec<u8> = (0..size)
        .map(|i: usize| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let path = format!("{base}/big.bin");
    let started = std::time::Instant::now();
    let mut w = fs.open_write(&path, 0).await.unwrap();
    w.write_all(&data).await.unwrap();
    w.shutdown().await.unwrap();
    let up = started.elapsed();

    let started = std::time::Instant::now();
    let mut r = fs.open_read(&path, 0).await.unwrap();
    let mut back = Vec::with_capacity(size);
    r.read_to_end(&mut back).await.unwrap();
    let down = started.elapsed();
    assert_eq!(blake3::hash(&back), blake3::hash(&data));
    eprintln!(
        "24 MiB loopback: upload {:.0} MiB/s, download {:.0} MiB/s",
        24.0 / up.as_secs_f64(),
        24.0 / down.as_secs_f64()
    );

    // Ranged read (used for resume verification).
    let tail = fs.read_range(&path, (size - 10) as u64, 64).await.unwrap();
    assert_eq!(tail, &data[size - 10..]);

    // Append at offset.
    let mut w = fs.open_write(&path, size as u64).await.unwrap();
    w.write_all(b"TAIL").await.unwrap();
    w.shutdown().await.unwrap();
    assert_eq!(fs.stat(&path).await.unwrap().unwrap().size, size as u64 + 4);

    fs.set_modified(&path, 1_600_000_000).await.unwrap();
    assert_eq!(
        fs.stat(&path).await.unwrap().unwrap().modified,
        Some(1_600_000_000)
    );

    let renamed = format!("{base}/a/big2.bin");
    fs.rename(&path, &renamed).await.unwrap();
    let listing = fs.list_dir(&format!("{base}/a")).await.unwrap();
    let names: Vec<_> = listing.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["b", "big2.bin"], "dirs first, no dot entries");

    assert!(
        fs.list_dir("/definitely/not/here")
            .await
            .unwrap_err()
            .is_not_found()
    );

    fs.remove_file(&renamed).await.unwrap();
    fs.remove_dir(&format!("{base}/a/b")).await.unwrap();
    fs.remove_dir(&format!("{base}/a")).await.unwrap();
    fs.remove_dir(&base).await.unwrap();
}
