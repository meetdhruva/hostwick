//! End-to-end tests for proxies, X11 and agent forwarding, certificates and
//! Kerberos against a real OpenSSH server.
//!
//! Skipped unless `HOSTWICK_NET_PORT` is set; `scripts/e2e-net.sh` starts
//! the server (and the KDC, X server, agent and proxies) and sets the
//! variables. Tests whose piece is missing skip themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hostwick_core::{AuthMethod, HostProfile, ProxySpec, SecretString};
use hostwick_ssh::{
    ConnectContext, Connection, Credentials, HostKeyDecision, HostKeyInfo, Interaction,
    NoCredentials, SshError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Env {
    port: u16,
    user: String,
    dir: PathBuf,
}

fn env() -> Option<Env> {
    let port = std::env::var("HOSTWICK_NET_PORT").ok()?.parse().ok()?;
    Some(Env {
        port,
        user: std::env::var("HOSTWICK_NET_USER").unwrap_or_else(|_| "hwnet".into()),
        dir: std::env::var("HOSTWICK_NET_DIR")
            .expect("HOSTWICK_NET_DIR")
            .into(),
    })
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

macro_rules! require {
    () => {
        match env() {
            Some(e) => e,
            None => {
                eprintln!("skipping: HOSTWICK_NET_PORT not set");
                return;
            }
        }
    };
    ($var:literal) => {{
        let e = require!();
        if var($var).is_none() {
            eprintln!("skipping: {} not set", $var);
            return;
        }
        e
    }};
}

/// Accepts host keys, answers password prompts from a table, records
/// notices.
#[derive(Default)]
struct Recorder {
    passwords: Vec<(String, String)>,
    password_prompts: Mutex<Vec<(String, String)>>,
    notices: Mutex<Vec<String>>,
}

impl Recorder {
    fn notices(&self) -> Vec<String> {
        self.notices.lock().unwrap().clone()
    }
}

#[async_trait]
impl Interaction for Recorder {
    async fn host_key(&self, _: &HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::AcceptOnce
    }
    async fn password(&self, user: &str, host: &str) -> Option<SecretString> {
        self.password_prompts
            .lock()
            .unwrap()
            .push((user.to_owned(), host.to_owned()));
        self.passwords
            .iter()
            .find(|(h, _)| host.contains(h.as_str()))
            .map(|(_, p)| SecretString::from(p.clone()))
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
    fn notice(&self, text: &str) {
        eprintln!("notice: {text}");
        self.notices.lock().unwrap().push(text.to_owned());
    }
}

struct OneSecret(String, String);
impl Credentials for OneSecret {
    fn get(&self, id: &str) -> Option<SecretString> {
        (id == self.0).then(|| SecretString::from(self.1.clone()))
    }
}

fn key(e: &Env, name: &str) -> AuthMethod {
    AuthMethod::KeyFile {
        path: e.dir.join(name),
        passphrase_credential: None,
    }
}

fn profile(e: &Env, host: &str, auth: Vec<AuthMethod>) -> HostProfile {
    let mut p = HostProfile::new("net-e2e", host);
    p.port = e.port;
    p.user = Some(e.user.clone());
    p.auth = auth;
    p.keepalive_secs = 0;
    p
}

fn ctx(ix: Arc<Recorder>) -> ConnectContext {
    ctx_with(ix, Arc::new(NoCredentials))
}

fn ctx_with(ix: Arc<Recorder>, creds: Arc<dyn Credentials>) -> ConnectContext {
    let mut c = ConnectContext::new(ix, creds);
    c.known_hosts = None;
    c.connect_timeout = Duration::from_secs(15);
    c
}

async fn run(conn: &Connection, cmd: &str) -> (String, Option<u32>) {
    let out = tokio::time::timeout(Duration::from_secs(30), async {
        let s = conn.open_exec(cmd, None).await.unwrap();
        let (_tx, mut rx) = s.split();
        let mut text = Vec::new();
        let mut code = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                hostwick_ssh::ShellEvent::Stdout(b) | hostwick_ssh::ShellEvent::Stderr(b) => {
                    text.extend_from_slice(&b)
                }
                hostwick_ssh::ShellEvent::Exit(c) => code = Some(c),
                hostwick_ssh::ShellEvent::ExitSignal(_) => {}
                hostwick_ssh::ShellEvent::Closed => break,
            }
        }
        (String::from_utf8_lossy(&text).into_owned(), code)
    })
    .await;
    out.expect("command timed out")
}

// ---------------------------------------------------------------- proxies --

/// Minimal SOCKS5 proxy (no auth, CONNECT) that records the requested
/// address type and host, then relays.
async fn socks5_proxy() -> (u16, Arc<Mutex<Vec<(u8, String)>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else {
                return;
            };
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut g = [0u8; 2];
                c.read_exact(&mut g).await.unwrap();
                let mut m = vec![0u8; usize::from(g[1])];
                c.read_exact(&mut m).await.unwrap();
                c.write_all(&[5, 0]).await.unwrap();
                let mut h = [0u8; 4];
                c.read_exact(&mut h).await.unwrap();
                let host = match h[3] {
                    3 => {
                        let n = c.read_u8().await.unwrap();
                        let mut b = vec![0u8; usize::from(n)];
                        c.read_exact(&mut b).await.unwrap();
                        String::from_utf8(b).unwrap()
                    }
                    1 => {
                        let mut b = [0u8; 4];
                        c.read_exact(&mut b).await.unwrap();
                        std::net::Ipv4Addr::from(b).to_string()
                    }
                    _ => panic!("unexpected atyp"),
                };
                let port = c.read_u16().await.unwrap();
                seen.lock().unwrap().push((h[3], host.clone()));
                let mut up = TcpStream::connect((host.as_str(), port)).await.unwrap();
                c.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut c, &mut up).await;
            });
        }
    });
    (port, seen)
}

/// Minimal HTTP CONNECT proxy requiring Basic `u:p`.
async fn http_proxy() -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let denied = Arc::new(AtomicUsize::new(0));
    let denied2 = denied.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else {
                return;
            };
            let denied = denied2.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(c.read_u8().await.unwrap());
                }
                let head = String::from_utf8(head).unwrap();
                // base64("u:p") = dTpw
                if !head.contains("Proxy-Authorization: Basic dTpw\r\n") {
                    denied.fetch_add(1, Ordering::SeqCst);
                    let _ = c
                        .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"t\"\r\n\r\n")
                        .await;
                    return;
                }
                let target = head.split_whitespace().nth(1).unwrap().to_owned();
                let mut up = TcpStream::connect(target).await.unwrap();
                c.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut c, &mut up).await;
            });
        }
    });
    (port, denied)
}

#[tokio::test]
async fn socks5_proxy_resolves_names_at_the_proxy() {
    let e = require!();
    let (pport, seen) = socks5_proxy().await;
    let mut p = profile(&e, "localhost", vec![key(&e, "client_key")]);
    p.proxy = Some(ProxySpec::parse_url(&format!("socks5://127.0.0.1:{pport}")).unwrap());
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    assert_eq!(run(&conn, "echo via-socks").await.0.trim(), "via-socks");
    // Domain-name addressing: the proxy, not us, resolved "localhost".
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[(3, "localhost".to_owned())]
    );
}

#[tokio::test]
async fn http_proxy_with_prompted_password_and_jump_host() {
    let e = require!();
    let (pport, denied) = http_proxy().await;
    let ix = Arc::new(Recorder {
        passwords: vec![("proxy 127.0.0.1".into(), "p".into())],
        ..Default::default()
    });
    // The proxy is used for the first hop (the jump host) only.
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.jump = vec![hostwick_core::Endpoint {
        host: "localhost".into(),
        port: e.port,
        user: None,
    }];
    p.proxy = Some(ProxySpec::parse_url(&format!("http://u@127.0.0.1:{pport}")).unwrap());
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    assert_eq!(run(&conn, "echo via-http").await.0.trim(), "via-http");
    assert_eq!(denied.load(Ordering::SeqCst), 0);
    assert_eq!(
        ix.password_prompts.lock().unwrap().as_slice(),
        &[("u".to_owned(), "proxy 127.0.0.1".to_owned())]
    );

    // Wrong password: a clear 407 error, no SSH attempt.
    let ix = Arc::new(Recorder {
        passwords: vec![("proxy".into(), "wrong".into())],
        ..Default::default()
    });
    let err = Connection::connect(&p, &ctx(ix)).await.unwrap_err();
    assert!(matches!(err, SshError::Proxy { .. }), "{err}");
    assert!(
        err.to_string().contains("rejected the credentials for `u`"),
        "{err}"
    );
}

#[tokio::test]
async fn real_http_proxy_with_vault_password() {
    let e = require!("HOSTWICK_NET_HTTP_PROXY");
    let mut spec = ProxySpec::parse_url(&var("HOSTWICK_NET_HTTP_PROXY").unwrap()).unwrap();
    spec.password_credential = Some("proxy-pw".into());
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.proxy = Some(spec);
    let ix = Arc::new(Recorder::default());
    let creds = Arc::new(OneSecret(
        "proxy-pw".into(),
        var("HOSTWICK_NET_PROXY_PASSWORD").unwrap(),
    ));
    let conn = Connection::connect(&p, &ctx_with(ix.clone(), creds))
        .await
        .unwrap();
    assert_eq!(
        run(&conn, "echo via-tinyproxy").await.0.trim(),
        "via-tinyproxy"
    );
    assert!(ix.password_prompts.lock().unwrap().is_empty());

    // No password at all (prompt cancelled): tinyproxy refuses (with 401).
    let err = Connection::connect(&p, &ctx(Arc::default()))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires a password for `hwproxy`"),
        "{err}"
    );
}

#[tokio::test]
async fn real_socks5_proxy_user_password() {
    let e = require!("HOSTWICK_NET_SOCKS_PROXY");
    let spec = ProxySpec::parse_url(&var("HOSTWICK_NET_SOCKS_PROXY").unwrap()).unwrap();
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.proxy = Some(spec);
    let ix = Arc::new(Recorder {
        passwords: vec![("proxy".into(), var("HOSTWICK_NET_PROXY_PASSWORD").unwrap())],
        ..Default::default()
    });
    let conn = Connection::connect(&p, &ctx(ix)).await.unwrap();
    assert_eq!(run(&conn, "echo via-dante").await.0.trim(), "via-dante");

    let ix = Arc::new(Recorder {
        passwords: vec![("proxy".into(), "nope".into())],
        ..Default::default()
    });
    let err = Connection::connect(&p, &ctx(ix)).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("rejected the user name or password"),
        "{err}"
    );

    // dante's ruleset only allows the SSH port.
    let ix = Arc::new(Recorder {
        passwords: vec![("proxy".into(), var("HOSTWICK_NET_PROXY_PASSWORD").unwrap())],
        ..Default::default()
    });
    p.port = 1;
    let err = Connection::connect(&p, &ctx(ix)).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("refused: connection not allowed by ruleset"),
        "{err}"
    );
}

// ----------------------------------------------------------- certificates --

#[tokio::test]
async fn openssh_certificates_ed25519_and_rsa() {
    let e = require!();
    for name in ["cert_key", "cert_rsa"] {
        let ix = Arc::new(Recorder::default());
        let p = profile(&e, "127.0.0.1", vec![key(&e, name)]);
        let conn = Connection::connect(&p, &ctx(ix.clone()))
            .await
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(run(&conn, "echo cert-ok").await.0.trim(), "cert-ok");
        assert!(ix.notices().is_empty(), "{:?}", ix.notices());
    }
}

#[tokio::test]
async fn certificate_falls_back_to_plain_key() {
    let e = require!();
    // Expired certificate: skipped up front with a notice.
    let ix = Arc::new(Recorder::default());
    let p = profile(&e, "127.0.0.1", vec![key(&e, "cert_fallback")]);
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    assert_eq!(run(&conn, "echo plain").await.0.trim(), "plain");
    assert!(
        ix.notices().iter().any(|n| n.contains("has expired")),
        "{:?}",
        ix.notices()
    );

    // Certificate for another principal: the server rejects it, the plain
    // key (authorized) still works.
    let ix = Arc::new(Recorder::default());
    let p = profile(&e, "127.0.0.1", vec![key(&e, "cert_wrongprinc")]);
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    assert_eq!(run(&conn, "echo plain2").await.0.trim(), "plain2");
}

#[tokio::test]
async fn agent_held_certificate() {
    let e = require!("SSH_AUTH_SOCK");
    // The agent only holds a key whose plain form is not authorized.
    let p = profile(&e, "127.0.0.1", vec![AuthMethod::Agent]);
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    assert_eq!(run(&conn, "echo agent-cert").await.0.trim(), "agent-cert");
}

#[tokio::test]
async fn security_key_file_gives_clear_error() {
    let e = require!();
    use russh::keys::ssh_key::{private, public};
    let pk = private::Ed25519Keypair::from_seed(&[3u8; 32]).public;
    let sk = private::SkEd25519::new(public::SkEd25519::new(pk, "ssh:"), 1, vec![9; 32]).unwrap();
    let skkey = russh::keys::PrivateKey::new(private::KeypairData::SkEd25519(sk), "sk").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519_sk");
    std::fs::write(
        &path,
        skkey
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();

    let ix = Arc::new(Recorder::default());
    let p = profile(
        &e,
        "127.0.0.1",
        vec![
            AuthMethod::KeyFile {
                path,
                passphrase_credential: None,
            },
            key_auth_fallback(&e),
        ],
    );
    Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    assert!(
        ix.notices()
            .iter()
            .any(|n| n.contains("add it to ssh-agent / Pageant instead")),
        "{:?}",
        ix.notices()
    );
}

fn key_auth_fallback(e: &Env) -> AuthMethod {
    key(e, "client_key")
}

// ------------------------------------------------------- agent forwarding --

#[tokio::test]
async fn agent_forwarding() {
    let e = require!("SSH_AUTH_SOCK");
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.agent_forwarding = true;
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    let (out, code) = run(&conn, "ssh-add -l").await;
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("agent_cert_key"), "{out}");
    // Two at once: each channel gets its own agent connection.
    let (a, b) = tokio::join!(run(&conn, "ssh-add -L"), run(&conn, "ssh-add -l"));
    assert!(a.0.contains("ssh-ed25519-cert-v01@openssh.com") && b.0.contains("ED25519"));

    p.agent_forwarding = false;
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    let (out, code) = run(&conn, "ssh-add -l").await;
    assert_ne!(code, Some(0), "agent must not be forwarded: {out}");
}

// ---------------------------------------------------------------------- X11 --

#[tokio::test]
async fn x11_forwarding_with_cookie_substitution() {
    let e = require!("HOSTWICK_NET_X11_DISPLAY");
    let ix = Arc::new(Recorder::default());
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.x11_forwarding = true;
    p.x11_display = var("HOSTWICK_NET_X11_DISPLAY");
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    // The local X server requires its cookie; the server side only has
    // our fake one, so this proves the substitution.
    let (out, code) = run(
        &conn,
        "echo D=$DISPLAY; xdpyinfo | head -2; xdpyinfo >/dev/null && echo second-ok",
    )
    .await;
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("D=localhost:"), "{out}");
    assert!(out.contains("name of display:"), "{out}");
    assert!(out.contains("second-ok"), "{out}");
    assert!(ix.notices().is_empty(), "{:?}", ix.notices());

    // A client presenting a different cookie is turned away.
    let (out, code) = run(
        &conn,
        "export XAUTHORITY=$(mktemp); xauth add $DISPLAY MIT-MAGIC-COOKIE-1 00112233445566778899aabbccddeeff; xdpyinfo",
    )
    .await;
    assert_ne!(code, Some(0), "{out}");
}

#[tokio::test]
async fn x11_without_local_server_keeps_shell_working() {
    let e = require!();
    let ix = Arc::new(Recorder::default());
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.x11_forwarding = true;
    p.x11_display = Some(":77".into()); // nothing listens there
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    let (out, _) = run(
        &conn,
        "echo shell-ok; command -v xdpyinfo >/dev/null && { xdpyinfo; xdpyinfo; } ; true",
    )
    .await;
    assert!(out.contains("shell-ok"), "{out}");
    let n = ix.notices();
    assert_eq!(n.len(), 1, "one notice, not one per X connection: {n:?}");
    assert!(n[0].contains("cannot open local display :77"), "{n:?}");
}

#[tokio::test]
async fn forwarding_refused_by_server_is_reported_once() {
    let mut e = require!("HOSTWICK_NET_RESTRICTED_PORT");
    e.port = var("HOSTWICK_NET_RESTRICTED_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let ix = Arc::new(Recorder::default());
    let mut p = profile(&e, "127.0.0.1", vec![key(&e, "client_key")]);
    p.x11_forwarding = true;
    p.agent_forwarding = true;
    // Any display that exists locally (or none): the server refuses first.
    p.x11_display = var("HOSTWICK_NET_X11_DISPLAY").or(Some(":77".into()));
    let conn = Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    for _ in 0..2 {
        let (out, _) = run(
            &conn,
            "echo D=${DISPLAY:-none}; ssh-add -l >/dev/null 2>&1; echo agent=$?",
        )
        .await;
        assert!(out.contains("D=none"), "{out}");
        assert!(
            out.contains("agent=2"),
            "agent must not be reachable: {out}"
        );
    }
    let refused: Vec<_> = ix
        .notices()
        .into_iter()
        .filter(|n| n.contains("X11 forwarding request refused"))
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", ix.notices());
}

// ----------------------------------------------------------------- Kerberos --

#[tokio::test]
async fn kerberos_login_and_delegation() {
    let e = require!("HOSTWICK_NET_KRB5");
    let p = profile(
        &e,
        "localhost",
        vec![AuthMethod::Gssapi { delegate: false }],
    );
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    assert_eq!(run(&conn, "whoami").await.0.trim(), e.user);

    let trace = var("HOSTWICK_NET_KRB5_TRACE").map(PathBuf::from);
    let before = trace
        .as_ref()
        .and_then(|t| std::fs::read_to_string(t).ok())
        .map_or(0, |s| s.len());
    let p = profile(&e, "localhost", vec![AuthMethod::Gssapi { delegate: true }]);
    let conn = Connection::connect(&p, &ctx(Arc::default())).await.unwrap();
    assert_eq!(run(&conn, "whoami").await.0.trim(), e.user);
    if let Some(t) = trace {
        // The server received a forwarded TGT.
        let s = std::fs::read_to_string(t).unwrap();
        assert!(
            s[before..].contains(&format!("Storing {}@", e.user))
                && s[before..].contains("-> krbtgt/"),
            "no delegated ticket in the server trace"
        );
    }
}

#[tokio::test]
async fn kerberos_unknown_principal_falls_through() {
    let e = require!("HOSTWICK_NET_KRB5");
    // No host/127.0.0.1 principal: Kerberos is skipped with a clear reason
    // and the key is used.
    let ix = Arc::new(Recorder::default());
    let p = profile(
        &e,
        "127.0.0.1",
        vec![
            AuthMethod::Gssapi { delegate: false },
            key(&e, "client_key"),
        ],
    );
    Connection::connect(&p, &ctx(ix.clone())).await.unwrap();
    assert!(
        ix.notices()
            .iter()
            .any(|n| n.contains("no Kerberos principal for host/127.0.0.1")),
        "{:?}",
        ix.notices()
    );

    // With nothing else configured, the error lists why Kerberos was skipped.
    let p = profile(
        &e,
        "127.0.0.1",
        vec![AuthMethod::Gssapi { delegate: false }],
    );
    let err = Connection::connect(&p, &ctx(Arc::default()))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("kerberos: no Kerberos principal"),
        "{err}"
    );
}
