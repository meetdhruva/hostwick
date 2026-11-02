//! Outbound proxies for the first hop: SOCKS5 (RFC 1928, with RFC 1929
//! user/password), SOCKS4/4a and HTTP `CONNECT` (with Basic auth).
//!
//! Host names are always sent to the proxy unresolved (SOCKS5 domain
//! addressing, SOCKS4a, HTTP `CONNECT host:port`), so DNS happens on the
//! proxy side, which is what corporate and Tor-style proxies expect.
//!
//! Every network step has its own timeout and every failure names the proxy
//! and what it said, e.g. `proxy 10.0.0.1:1080 refused: connection not
//! allowed by ruleset (target db.internal:22)`.

use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use base64::Engine as _;
use hostwick_core::{ExposeSecret, ProxyKind, ProxySpec, SecretString};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroize;

use crate::SshError;

/// Credentials for the proxy itself (not the SSH server).
#[derive(Default)]
pub(crate) struct ProxyCredentials {
    pub user: Option<String>,
    pub password: Option<SecretString>,
}

/// Connect to `target:port` through the proxy. `timeout` bounds each step
/// (TCP connect, each handshake round trip) and the whole exchange.
pub(crate) async fn connect(
    spec: &ProxySpec,
    creds: &ProxyCredentials,
    target: &str,
    port: u16,
    timeout: Duration,
) -> Result<TcpStream, SshError> {
    let p = Proxy {
        addr: display_addr(&spec.host, spec.port),
        timeout,
    };
    let whole = async {
        let mut tcp = match tokio::time::timeout(
            timeout,
            TcpStream::connect((spec.host.as_str(), spec.port)),
        )
        .await
        {
            Ok(Ok(t)) => t,
            // Unreachable proxy: a plain (transient) connect error.
            Ok(Err(reason)) => {
                return Err(SshError::Connect {
                    host: format!("proxy {}", spec.host),
                    port: spec.port,
                    reason,
                });
            }
            Err(_) => {
                return Err(SshError::Timeout(
                    timeout,
                    format!("proxy {} (TCP connect)", p.addr),
                ));
            }
        };
        let _ = tcp.set_nodelay(true);
        handshake(&p, spec.kind, &mut tcp, creds, target, port).await?;
        Ok(tcp)
    };
    // Generous overall cap: a proxy trickling bytes can't keep us forever.
    match tokio::time::timeout(timeout.saturating_mul(2), whole).await {
        Ok(r) => r,
        Err(_) => Err(SshError::Timeout(timeout, format!("proxy {}", p.addr))),
    }
}

/// Run the proxy handshake on an already-connected stream.
pub(crate) async fn handshake<S>(
    p: &Proxy,
    kind: ProxyKind,
    stream: &mut S,
    creds: &ProxyCredentials,
    target: &str,
    port: u16,
) -> Result<(), SshError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match kind {
        ProxyKind::Socks5 => socks5(p, stream, creds, target, port).await,
        ProxyKind::Socks4 => socks4(p, stream, creds, target, port).await,
        ProxyKind::Http => http_connect(p, stream, creds, target, port).await,
    }
}

pub(crate) struct Proxy {
    /// `host:port` (IPv6 in brackets), for messages.
    pub addr: String,
    pub timeout: Duration,
}

impl Proxy {
    fn err(&self, reason: impl Into<String>) -> SshError {
        SshError::Proxy {
            proxy: self.addr.clone(),
            reason: reason.into(),
        }
    }

    async fn step<T>(
        &self,
        what: &str,
        fut: impl Future<Output = std::io::Result<T>>,
    ) -> Result<T, SshError> {
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Err(self.err(format!("closed the connection during {what}")))
            }
            Ok(Err(e)) => Err(self.err(format!("failed during {what}: {e}"))),
            Err(_) => Err(SshError::Timeout(
                self.timeout,
                format!("proxy {} ({what})", self.addr),
            )),
        }
    }
}

fn display_addr(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn socks5_reply_text(code: u8) -> &'static str {
    match code {
        1 => "general SOCKS server failure",
        2 => "connection not allowed by ruleset",
        3 => "network unreachable",
        4 => "host unreachable",
        5 => "connection refused by the target",
        6 => "TTL expired",
        7 => "command not supported",
        8 => "address type not supported",
        _ => "unknown error",
    }
}

/// A peer that answers a SOCKS greeting with `HTTP/1.x ...` is an HTTP proxy.
fn not_socks(p: &Proxy, first: u8, version: u8) -> SshError {
    if first == b'H' {
        p.err(format!(
            "answered with HTTP, not SOCKS{version}; is it an HTTP proxy? (use http://)"
        ))
    } else {
        p.err(format!(
            "is not a SOCKS{version} proxy (unexpected reply byte 0x{first:02x})"
        ))
    }
}

async fn socks5<S>(
    p: &Proxy,
    s: &mut S,
    creds: &ProxyCredentials,
    target: &str,
    port: u16,
) -> Result<(), SshError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let user_pass = match (&creds.user, &creds.password) {
        (Some(u), Some(pw)) => Some((u.as_str(), pw)),
        _ => None,
    };
    // Greeting: offer no-auth, plus user/password when we have both.
    let greeting: &[u8] = if user_pass.is_some() {
        &[5, 2, 0x00, 0x02]
    } else {
        &[5, 1, 0x00]
    };
    let mut choice = [0u8; 2];
    p.step("SOCKS5 greeting", async {
        s.write_all(greeting).await?;
        s.flush().await?;
        s.read_exact(&mut choice).await
    })
    .await?;
    if choice[0] != 5 {
        return Err(not_socks(p, choice[0], 5));
    }
    match choice[1] {
        0x00 => {}
        0x02 => {
            let Some((user, pw)) = user_pass else {
                return Err(p.err("asked for a password we did not offer"));
            };
            let pw = pw.expose_secret().as_bytes();
            let (Ok(ulen), Ok(plen)) = (u8::try_from(user.len()), u8::try_from(pw.len())) else {
                return Err(p.err("user name or password longer than 255 bytes (SOCKS5 limit)"));
            };
            let mut msg = Vec::with_capacity(3 + user.len() + pw.len());
            msg.push(1);
            msg.push(ulen);
            msg.extend_from_slice(user.as_bytes());
            msg.push(plen);
            msg.extend_from_slice(pw);
            let mut status = [0u8; 2];
            let res = p
                .step("SOCKS5 authentication", async {
                    s.write_all(&msg).await?;
                    s.flush().await?;
                    s.read_exact(&mut status).await
                })
                .await;
            msg.zeroize();
            res?;
            if status[1] != 0 {
                return Err(p.err(format!("rejected the user name or password for `{user}`")));
            }
        }
        0xFF => {
            return Err(p.err(match (&creds.user, user_pass.is_some()) {
                (_, true) => "accepts none of our authentication methods \
                              (we offer none and user/password)"
                    .to_owned(),
                (Some(u), false) => {
                    format!("requires a password for `{u}` and none was given")
                }
                (None, false) => "requires authentication; set a proxy user name".to_owned(),
            }));
        }
        m => {
            return Err(p.err(format!(
                "chose unsupported SOCKS5 authentication method 0x{m:02x}"
            )));
        }
    }

    let mut req = vec![5, 1, 0];
    match target.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => {
            req.push(1);
            req.extend_from_slice(&a.octets());
        }
        Ok(IpAddr::V6(a)) => {
            req.push(4);
            req.extend_from_slice(&a.octets());
        }
        Err(_) => {
            let len = u8::try_from(target.len())
                .map_err(|_| p.err("target host name longer than 255 bytes"))?;
            req.push(3);
            req.push(len);
            req.extend_from_slice(target.as_bytes());
        }
    }
    req.extend_from_slice(&port.to_be_bytes());
    let mut head = [0u8; 4];
    p.step("SOCKS5 connect", async {
        s.write_all(&req).await?;
        s.flush().await?;
        s.read_exact(&mut head).await
    })
    .await?;
    let [ver, rep, _rsv, atyp] = head;
    if ver != 5 {
        return Err(not_socks(p, ver, 5));
    }
    if rep != 0 {
        return Err(p.err(format!(
            "refused: {} (target {})",
            socks5_reply_text(rep),
            display_addr(target, port)
        )));
    }
    // Consume the bound address so the SSH banner starts clean.
    let skip = match atyp {
        1 => 4 + 2,
        4 => 16 + 2,
        3 => {
            let n = p.step("SOCKS5 connect", s.read_u8()).await?;
            usize::from(n) + 2
        }
        t => return Err(p.err(format!("sent unknown address type {t} in its reply"))),
    };
    let mut rest = vec![0u8; skip];
    p.step("SOCKS5 connect", s.read_exact(&mut rest)).await?;
    Ok(())
}

async fn socks4<S>(
    p: &Proxy,
    s: &mut S,
    creds: &ProxyCredentials,
    target: &str,
    port: u16,
) -> Result<(), SshError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut req = vec![4, 1];
    req.extend_from_slice(&port.to_be_bytes());
    let domain = match target.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => {
            req.extend_from_slice(&a.octets());
            None
        }
        Ok(IpAddr::V6(_)) => {
            return Err(p.err("cannot reach IPv6 addresses over SOCKS4; use SOCKS5"));
        }
        // SOCKS4a: 0.0.0.x (x != 0) means "host name follows".
        Err(_) => {
            req.extend_from_slice(&[0, 0, 0, 1]);
            Some(target)
        }
    };
    let user = creds.user.as_deref().unwrap_or("");
    if user.contains('\0') || target.contains('\0') {
        return Err(p.err("user or host name contains a NUL byte"));
    }
    req.extend_from_slice(user.as_bytes());
    req.push(0);
    if let Some(d) = domain {
        req.extend_from_slice(d.as_bytes());
        req.push(0);
    }
    let mut reply = [0u8; 8];
    p.step("SOCKS4 connect", async {
        s.write_all(&req).await?;
        s.flush().await?;
        s.read_exact(&mut reply).await
    })
    .await?;
    // VN is 0 per the spec; some servers echo 4.
    if reply[0] != 0 && reply[0] != 4 {
        return Err(not_socks(p, reply[0], 4));
    }
    let reason = match reply[1] {
        90 => return Ok(()),
        91 => "request rejected or failed",
        92 => "request rejected: the proxy cannot reach our identd",
        93 => "request rejected: identd reported a different user id",
        _ => "unknown reply code",
    };
    Err(p.err(format!(
        "refused: {reason} (code {}, target {})",
        reply[1],
        display_addr(target, port)
    )))
}

/// Longest proxy response header we accept before giving up.
const MAX_HTTP_HEADER: usize = 16 * 1024;

async fn http_connect<S>(
    p: &Proxy,
    s: &mut S,
    creds: &ProxyCredentials,
    target: &str,
    port: u16,
) -> Result<(), SshError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if target.contains(['\r', '\n', ' ']) {
        return Err(p.err("target host name contains whitespace"));
    }
    let authority = display_addr(target, port);
    let mut req = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nUser-Agent: Hostwick/{}\r\n",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(user) = &creds.user {
        let pw = creds
            .password
            .as_ref()
            .map(|p| p.expose_secret())
            .unwrap_or("");
        let mut raw = format!("{user}:{pw}");
        let token = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        raw.zeroize();
        req.push_str("Proxy-Authorization: Basic ");
        req.push_str(&token);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    let res = p
        .step("HTTP CONNECT", async {
            s.write_all(req.as_bytes()).await?;
            s.flush().await
        })
        .await;
    req.zeroize();
    res?;

    // Read the response one byte at a time: the SSH server's banner may
    // follow the header immediately and must stay in the stream.
    let mut head = Vec::with_capacity(256);
    p.step("HTTP CONNECT response", async {
        loop {
            let b = s.read_u8().await?;
            head.push(b);
            if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
                return Ok(());
            }
            if head.len() > MAX_HTTP_HEADER {
                return Err(std::io::Error::other("response header too long"));
            }
        }
    })
    .await?;
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.lines();
    let status_line = lines.next().unwrap_or("").trim();
    let mut parts = status_line.splitn(3, ' ');
    let (proto, code, reason) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("").trim(),
    );
    if !proto.starts_with("HTTP/") {
        return Err(p.err(format!(
            "is not an HTTP proxy (it answered `{}`)",
            status_line.chars().take(60).collect::<String>()
        )));
    }
    let code: u16 = code
        .parse()
        .map_err(|_| p.err(format!("sent a malformed status line `{status_line}`")))?;
    if (200..300).contains(&code) {
        return Ok(());
    }
    // 407 is the standard answer; some proxies (tinyproxy) send 401.
    if code == 407 || code == 401 {
        let schemes: Vec<String> = lines
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                let k = k.trim();
                (k.eq_ignore_ascii_case("proxy-authenticate")
                    || k.eq_ignore_ascii_case("www-authenticate"))
                .then(|| v.split_whitespace().next().unwrap_or("").to_owned())
            })
            .collect();
        let basic_ok =
            schemes.is_empty() || schemes.iter().any(|s| s.eq_ignore_ascii_case("basic"));
        return Err(p.err(match (&creds.user, basic_ok) {
            (_, false) => format!(
                "requires {} authentication (HTTP {code}); only Basic is supported",
                schemes.join("/")
            ),
            (Some(u), true) if creds.password.is_some() => {
                format!("rejected the credentials for `{u}` (HTTP {code})")
            }
            (Some(u), true) => {
                format!("requires a password for `{u}` (HTTP {code}) and none was given")
            }
            (None, true) => {
                format!("requires authentication (HTTP {code} {reason}); set a proxy user name")
            }
        }));
    }
    Err(p.err(format!(
        "refused: HTTP {code} {reason} (target {authority})"
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use tokio::net::TcpListener;

    const T: Duration = Duration::from_secs(5);

    /// Fake proxy: runs `script` on the accepted connection, then echoes a
    /// banner so the test can check the stream is positioned right after
    /// the handshake.
    async fn fake<F, Fut>(script: F) -> (ProxySpec, tokio::task::JoinHandle<Vec<u8>>)
    where
        F: FnOnce(TcpStream) -> Fut + Send + 'static,
        Fut: Future<Output = Vec<u8>> + Send + 'static,
    {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let h = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            script(s).await
        });
        let spec = ProxySpec {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port,
            user: None,
            password_credential: None,
        };
        (spec, h)
    }

    fn creds(user: Option<&str>, pw: Option<&str>) -> ProxyCredentials {
        ProxyCredentials {
            user: user.map(str::to_owned),
            password: pw.map(|p| SecretString::from(p.to_owned())),
        }
    }

    async fn read_banner(mut s: TcpStream) -> String {
        let mut b = [0u8; 8];
        s.read_exact(&mut b).await.unwrap();
        String::from_utf8_lossy(&b).into_owned()
    }

    #[tokio::test]
    async fn socks5_no_auth_domain_name() {
        let (spec, h) = fake(|mut s| async move {
            let mut g = [0u8; 3];
            s.read_exact(&mut g).await.unwrap();
            assert_eq!(g, [5, 1, 0]);
            s.write_all(&[5, 0]).await.unwrap();
            let mut req = [0u8; 5];
            s.read_exact(&mut req).await.unwrap();
            assert_eq!(&req[..4], &[5, 1, 0, 3]);
            let mut name = vec![0u8; usize::from(req[4]) + 2];
            s.read_exact(&mut name).await.unwrap();
            // Reply with a domain-name bound address, then the "SSH banner".
            s.write_all(&[5, 0, 0, 3, 4, b'a', b'b', b'c', b'd', 0, 22])
                .await
                .unwrap();
            s.write_all(b"SSH-2.0-").await.unwrap();
            name
        })
        .await;
        let tcp = connect(&spec, &creds(None, None), "db.internal", 2222, T)
            .await
            .unwrap();
        assert_eq!(read_banner(tcp).await, "SSH-2.0-");
        let name = h.await.unwrap();
        assert_eq!(&name[..name.len() - 2], b"db.internal");
        assert_eq!(&name[name.len() - 2..], &2222u16.to_be_bytes());
    }

    #[tokio::test]
    async fn socks5_user_password() {
        let (spec, h) = fake(|mut s| async move {
            let mut g = [0u8; 4];
            s.read_exact(&mut g).await.unwrap();
            assert_eq!(g, [5, 2, 0, 2]);
            s.write_all(&[5, 2]).await.unwrap();
            let mut v = [0u8; 2];
            s.read_exact(&mut v).await.unwrap();
            let mut user = vec![0u8; usize::from(v[1])];
            s.read_exact(&mut user).await.unwrap();
            let plen = s.read_u8().await.unwrap();
            let mut pw = vec![0u8; usize::from(plen)];
            s.read_exact(&mut pw).await.unwrap();
            s.write_all(&[1, 0]).await.unwrap();
            let mut req = [0u8; 10];
            s.read_exact(&mut req).await.unwrap();
            assert_eq!(&req[..4], &[5, 1, 0, 1]);
            assert_eq!(&req[4..8], &[10, 1, 2, 3]);
            s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
            s.write_all(b"SSH-2.0-").await.unwrap();
            [user, b":".to_vec(), pw].concat()
        })
        .await;
        let tcp = connect(&spec, &creds(Some("alice"), Some("pw")), "10.1.2.3", 22, T)
            .await
            .unwrap();
        assert_eq!(read_banner(tcp).await, "SSH-2.0-");
        assert_eq!(h.await.unwrap(), b"alice:pw");
    }

    #[tokio::test]
    async fn socks5_bad_password_and_ruleset() {
        let (spec, _h) = fake(|mut s| async move {
            let mut g = [0u8; 4];
            s.read_exact(&mut g).await.unwrap();
            s.write_all(&[5, 2]).await.unwrap();
            let mut buf = [0u8; 64];
            let _ = s.read(&mut buf).await;
            s.write_all(&[1, 1]).await.unwrap();
            Vec::new()
        })
        .await;
        let e = connect(&spec, &creds(Some("bob"), Some("x")), "h", 22, T)
            .await
            .unwrap_err();
        assert!(
            e.to_string()
                .contains("rejected the user name or password for `bob`"),
            "{e}"
        );

        let (spec, _h) = fake(|mut s| async move {
            let mut g = [0u8; 3];
            s.read_exact(&mut g).await.unwrap();
            s.write_all(&[5, 0]).await.unwrap();
            let mut buf = [0u8; 64];
            let _ = s.read(&mut buf).await;
            s.write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
            Vec::new()
        })
        .await;
        let e = connect(&spec, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            format!(
                "proxy 127.0.0.1:{} refused: connection not allowed by ruleset (target h:22)",
                spec.port
            )
        );
        assert!(!e.is_transient());
    }

    #[tokio::test]
    async fn socks5_requires_auth_and_http_mixup() {
        let (spec, _h) = fake(|mut s| async move {
            let mut g = [0u8; 3];
            s.read_exact(&mut g).await.unwrap();
            s.write_all(&[5, 0xFF]).await.unwrap();
            Vec::new()
        })
        .await;
        let e = connect(&spec, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("requires authentication"), "{e}");

        let (spec, _h) = fake(|mut s| async move {
            let mut g = [0u8; 3];
            s.read_exact(&mut g).await.unwrap();
            s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
                .await
                .unwrap();
            Vec::new()
        })
        .await;
        let e = connect(&spec, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("is it an HTTP proxy"), "{e}");
    }

    #[tokio::test]
    async fn socks4a_and_socks4_rejection() {
        let (mut spec, h) = fake(|mut s| async move {
            let mut buf = vec![0u8; 8];
            s.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf[..4], &[4, 1, 0, 22]);
            assert_eq!(&buf[4..], &[0, 0, 0, 1]);
            // user id "u\0" then host "example.org\0"
            let mut rest = Vec::new();
            let mut zeros = 0;
            while zeros < 2 {
                let b = s.read_u8().await.unwrap();
                if b == 0 {
                    zeros += 1;
                }
                rest.push(b);
            }
            s.write_all(&[0, 90, 0, 0, 0, 0, 0, 0]).await.unwrap();
            s.write_all(b"SSH-2.0-").await.unwrap();
            rest
        })
        .await;
        spec.kind = ProxyKind::Socks4;
        let tcp = connect(&spec, &creds(Some("u"), None), "example.org", 22, T)
            .await
            .unwrap();
        assert_eq!(read_banner(tcp).await, "SSH-2.0-");
        assert_eq!(h.await.unwrap(), b"u\0example.org\0");

        let (mut spec, _h) = fake(|mut s| async move {
            let mut buf = [0u8; 9];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&[0, 91, 0, 0, 0, 0, 0, 0]).await.unwrap();
            Vec::new()
        })
        .await;
        spec.kind = ProxyKind::Socks4;
        let e = connect(&spec, &creds(None, None), "10.0.0.9", 22, T)
            .await
            .unwrap_err();
        assert!(
            e.to_string()
                .contains("refused: request rejected or failed"),
            "{e}"
        );
        let (mut a, _b) = tokio::io::duplex(64);
        let p = Proxy {
            addr: "x:1".into(),
            timeout: T,
        };
        let e = handshake(&p, ProxyKind::Socks4, &mut a, &creds(None, None), "::1", 22)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("IPv6"), "{e}");
    }

    async fn read_http_request(s: &mut TcpStream) -> String {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(s.read_u8().await.unwrap());
        }
        String::from_utf8(head).unwrap()
    }

    #[tokio::test]
    async fn http_connect_basic_auth_keeps_banner() {
        let (mut spec, h) = fake(|mut s| async move {
            let req = read_http_request(&mut s).await;
            // Response and banner in one write, as real proxies often do.
            s.write_all(b"HTTP/1.1 200 Connection established\r\nVia: x\r\n\r\nSSH-2.0-")
                .await
                .unwrap();
            req.into_bytes()
        })
        .await;
        spec.kind = ProxyKind::Http;
        let tcp = connect(&spec, &creds(Some("alice"), Some("s3cret")), "::1", 22, T)
            .await
            .unwrap();
        assert_eq!(read_banner(tcp).await, "SSH-2.0-");
        let req = String::from_utf8(h.await.unwrap()).unwrap();
        assert!(req.starts_with("CONNECT [::1]:22 HTTP/1.1\r\n"), "{req}");
        // base64("alice:s3cret")
        assert!(
            req.contains("Proxy-Authorization: Basic YWxpY2U6czNjcmV0\r\n"),
            "{req}"
        );
    }

    #[tokio::test]
    async fn http_407_and_403() {
        let (mut spec, _h) = fake(|mut s| async move {
            read_http_request(&mut s).await;
            s.write_all(
                b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"x\"\r\n\r\n",
            )
            .await
            .unwrap();
            Vec::new()
        })
        .await;
        spec.kind = ProxyKind::Http;
        let e = connect(&spec, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert!(
            e.to_string().contains("requires authentication (HTTP 407"),
            "{e}"
        );

        let (mut spec, _h) = fake(|mut s| async move {
            read_http_request(&mut s).await;
            s.write_all(
                b"HTTP/1.1 407 Nope\r\nProxy-Authenticate: NTLM\r\nProxy-Authenticate: Negotiate\r\n\r\n",
            )
            .await
            .unwrap();
            Vec::new()
        })
        .await;
        spec.kind = ProxyKind::Http;
        let e = connect(&spec, &creds(Some("u"), Some("p")), "h", 22, T)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("NTLM/Negotiate"), "{e}");

        let (mut spec, _h) = fake(|mut s| async move {
            read_http_request(&mut s).await;
            s.write_all(b"HTTP/1.0 403 Forbidden\r\n\r\n")
                .await
                .unwrap();
            Vec::new()
        })
        .await;
        spec.kind = ProxyKind::Http;
        let e = connect(&spec, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert!(
            e.to_string()
                .ends_with("refused: HTTP 403 Forbidden (target h:22)"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn silent_proxy_times_out_and_dead_proxy_is_transient() {
        let (spec, _h) = fake(|s| async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(s);
            Vec::new()
        })
        .await;
        let e = connect(
            &spec,
            &creds(None, None),
            "h",
            22,
            Duration::from_millis(200),
        )
        .await
        .unwrap_err();
        assert!(matches!(e, SshError::Timeout(..)), "{e}");
        assert!(e.to_string().contains("SOCKS5 greeting"), "{e}");

        // Nothing listening: a connect error naming the proxy.
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let dead = ProxySpec { port, ..spec };
        let e = connect(&dead, &creds(None, None), "h", 22, T)
            .await
            .unwrap_err();
        assert!(e.is_transient(), "{e}");
        assert!(
            e.to_string()
                .starts_with(&format!("could not reach proxy 127.0.0.1:{port}")),
            "{e}"
        );
    }
}
