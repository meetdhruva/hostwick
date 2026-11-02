//! X11 forwarding, the way OpenSSH's untrusted `ssh -X` does it.
//!
//! The server gets a random *fake* MIT-MAGIC-COOKIE-1. Each X11 channel the
//! server opens back to us starts with the X client's connection-setup
//! packet carrying that fake cookie; we check it, swap in the local X
//! server's *real* cookie (from `xauth list`), and from then on just pump
//! bytes both ways. The real cookie never leaves this machine.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{ChannelOpenHandle, Msg};
use russh::{Channel, ChannelOpenFailure};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::OnceCell;

use crate::interaction::Interaction;

const MIT_COOKIE: &str = "MIT-MAGIC-COOKIE-1";
const COOKIE_LEN: usize = 16;
/// Connecting to the local display.
const LOCAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The X client must send its setup packet promptly.
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
const XAUTH_TIMEOUT: Duration = Duration::from_secs(5);

/// Where a local X display lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11Target {
    /// Unix-domain socket, e.g. `/tmp/.X11-unix/X0`.
    Unix(PathBuf),
    /// TCP, port 6000 + display number.
    Tcp { host: String, port: u16 },
}

/// A parsed X display name (`:0`, `unix:0.1`, `localhost:10.0`,
/// `host:0`, or macOS launchd's `/private/tmp/.../org.xquartz:0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Display {
    /// The display name as given (for messages).
    pub name: String,
    pub target: X11Target,
    pub number: u32,
    pub screen: u32,
}

impl X11Display {
    /// Parses a display name with this platform's rules: on Windows there
    /// are no X sockets, so `:0` means TCP `localhost:6000`.
    pub fn parse(name: &str) -> Result<Self, String> {
        parse_display(name, cfg!(unix))
    }

    /// The display to use: the profile's, else `$DISPLAY`, else (Windows
    /// only, for VcXsrv/Xming) `localhost:0`.
    pub fn resolve(configured: Option<&str>) -> Result<Self, String> {
        if let Some(d) = configured.map(str::trim).filter(|d| !d.is_empty()) {
            return Self::parse(d);
        }
        match std::env::var("DISPLAY") {
            Ok(d) if !d.trim().is_empty() => Self::parse(d.trim()),
            _ if cfg!(windows) => Self::parse("localhost:0"),
            _ => Err("no local X display ($DISPLAY is not set)".into()),
        }
    }

    /// Display name for `xauth list`. Like OpenSSH, `localhost:N` is looked
    /// up as `unix:N`, which is how xauth records local displays.
    fn xauth_name(&self) -> String {
        match &self.target {
            X11Target::Tcp { host, .. } if host == "localhost" => format!("unix:{}", self.number),
            _ => self.name.clone(),
        }
    }

    async fn connect(&self) -> std::io::Result<Box<dyn Duplex>> {
        match &self.target {
            #[cfg(unix)]
            X11Target::Unix(path) => {
                let s = tokio::net::UnixStream::connect(path).await?;
                Ok(Box::new(s))
            }
            #[cfg(not(unix))]
            X11Target::Unix(path) => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("Unix socket {} not supported here", path.display()),
            )),
            X11Target::Tcp { host, port } => {
                let s = tokio::net::TcpStream::connect((host.as_str(), *port)).await?;
                let _ = s.set_nodelay(true);
                Ok(Box::new(s))
            }
        }
    }
}

fn parse_display(name: &str, unix_sockets: bool) -> Result<X11Display, String> {
    let bad = |why: &str| format!("bad X display `{name}`: {why}");
    // macOS launchd: "/private/tmp/com.apple.launchd.XXX/org.xquartz:0".
    let (host, rest) = name
        .rsplit_once(':')
        .ok_or_else(|| bad("expected [host]:display[.screen]"))?;
    if host.ends_with(':') && !host.starts_with('[') && !host.contains("::") {
        return Err(bad("DECnet displays are not supported"));
    }
    let (num, screen) = match rest.split_once('.') {
        Some((n, s)) => (n, s),
        None => (rest, "0"),
    };
    let number: u32 = num.parse().map_err(|_| bad("display number"))?;
    let screen: u32 = screen.parse().map_err(|_| bad("screen number"))?;
    let port = u16::try_from(number)
        .ok()
        .and_then(|n| 6000u16.checked_add(n))
        .ok_or_else(|| bad("display number too large"))?;

    let host = host.trim_start_matches('[').trim_end_matches(']');
    // xauth-style "host/unix:0" means the local socket.
    let host = host.strip_suffix("/unix").map_or(host, |_| "unix");
    let target = if host.starts_with('/') {
        if !unix_sockets {
            return Err(bad("socket paths only work on macOS/Linux"));
        }
        X11Target::Unix(PathBuf::from(host))
    } else if host.is_empty() || host == "unix" {
        if unix_sockets {
            X11Target::Unix(PathBuf::from(format!("/tmp/.X11-unix/X{number}")))
        } else {
            X11Target::Tcp {
                host: "localhost".into(),
                port,
            }
        }
    } else {
        X11Target::Tcp {
            host: host.to_owned(),
            port,
        }
    };
    Ok(X11Display {
        name: name.to_owned(),
        target,
        number,
        screen,
    })
}

pub(crate) trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}

/// Real authorization for the local display, from `xauth`.
#[derive(Clone, PartialEq, Eq)]
struct XAuth {
    proto: String,
    data: Vec<u8>,
}

/// Per-connection X11 forwarding state, shared by the shell (which asks the
/// server for forwarding) and the handler (which receives X11 channels).
pub(crate) struct X11Forwarding {
    display: Result<X11Display, String>,
    fake_cookie: [u8; COOKIE_LEN],
    real_auth: OnceCell<Option<XAuth>>,
    interaction: Arc<dyn Interaction>,
    warned: AtomicBool,
    refused_warned: AtomicBool,
}

impl X11Forwarding {
    pub(crate) fn new(configured: Option<&str>, interaction: Arc<dyn Interaction>) -> Arc<Self> {
        let mut fake_cookie = [0u8; COOKIE_LEN];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut fake_cookie);
        Arc::new(Self {
            display: X11Display::resolve(configured),
            fake_cookie,
            real_auth: OnceCell::new(),
            interaction,
            warned: AtomicBool::new(false),
            refused_warned: AtomicBool::new(false),
        })
    }

    /// Shown at most once per connection, so a GUI app retrying doesn't
    /// flood the terminal.
    fn notice_once(&self, text: &str) {
        if !self.warned.swap(true, Ordering::SeqCst) {
            self.interaction.notice(text);
        }
    }

    /// The server said no to `x11-req` (once per connection).
    pub(crate) fn report_refused(&self) {
        if !self.refused_warned.swap(true, Ordering::SeqCst) {
            self.interaction.notice(
                "X11 forwarding request refused by the server \
                 (it needs `X11Forwarding yes` in sshd_config and `xauth` installed)",
            );
        }
    }

    fn no_server_hint(display: &X11Display, e: &std::io::Error) -> String {
        let hint = if cfg!(windows) {
            "start an X server such as VcXsrv or Xming"
        } else if cfg!(target_os = "macos") {
            "install and start XQuartz"
        } else {
            "is an X server running?"
        };
        format!(
            "X11 forwarding: cannot open local display {} ({e}); {hint}. The session works without it.",
            display.name
        )
    }

    async fn real_auth(&self, display: &X11Display) -> Option<XAuth> {
        self.real_auth
            .get_or_init(|| async { xauth_lookup(&display.xauth_name()).await })
            .await
            .clone()
    }

    /// Parameters for the `x11-req` channel request, or `None` (after a
    /// notice) when there is no display to forward to. Also checks that the
    /// local X server answers, so a missing one is reported up front.
    pub(crate) async fn request_params(&self) -> Option<(String, String, u32)> {
        let display = match &self.display {
            Ok(d) => d,
            Err(e) => {
                self.notice_once(&format!(
                    "X11 forwarding: {e}; remote GUI programs won't be shown."
                ));
                return None;
            }
        };
        match tokio::time::timeout(LOCAL_CONNECT_TIMEOUT, display.connect()).await {
            Ok(Ok(_probe)) => {}
            Ok(Err(e)) => self.notice_once(&Self::no_server_hint(display, &e)),
            Err(_) => self.notice_once(&Self::no_server_hint(
                display,
                &std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"),
            )),
        }
        // Fetch now so the first X11 channel doesn't wait on xauth.
        if self.real_auth(display).await.is_none() {
            let name = &display.name;
            tracing::info!(display = %name, "no xauth cookie; X connections will carry no authorization");
        }
        Some((
            MIT_COOKIE.to_owned(),
            hex::encode(self.fake_cookie),
            display.screen,
        ))
    }

    /// Handle one X11 channel opened by the server. Runs in its own task.
    pub(crate) async fn serve(self: Arc<Self>, channel: Channel<Msg>, reply: ChannelOpenHandle) {
        let display = match &self.display {
            Ok(d) => d.clone(),
            Err(_) => {
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
                return;
            }
        };
        let mut local = match tokio::time::timeout(LOCAL_CONNECT_TIMEOUT, display.connect()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                self.notice_once(&Self::no_server_hint(&display, &e));
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
                return;
            }
            Err(_) => {
                self.notice_once(&Self::no_server_hint(
                    &display,
                    &std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"),
                ));
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
                return;
            }
        };
        reply.accept().await;
        let mut remote = channel.into_stream();

        let setup = match tokio::time::timeout(SETUP_TIMEOUT, read_setup(&mut remote)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "X11 channel closed before setup");
                return;
            }
            Err(_) => {
                tracing::warn!("X11 client sent no setup packet; closing");
                return;
            }
        };
        let real = self.real_auth(&display).await;
        let packet = match setup.substitute(&self.fake_cookie, real.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "rejecting X11 connection");
                return;
            }
        };
        if let Err(e) = local.write_all(&packet).await {
            tracing::debug!(error = %e, "local X server write failed");
            return;
        }
        // copy_bidirectional only reads when the other side can take the
        // data, and channel writes wait for SSH window: backpressure both ways.
        match tokio::io::copy_bidirectional(&mut remote, &mut local).await {
            Ok((up, down)) => tracing::debug!(up, down, "X11 connection closed"),
            Err(e) => tracing::debug!(error = %e, "X11 connection ended"),
        }
    }
}

/// `xauth list <display>` → the MIT-MAGIC-COOKIE-1 for it, if any.
async fn xauth_lookup(display: &str) -> Option<XAuth> {
    let mut candidates = vec![PathBuf::from("xauth")];
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/opt/X11/bin/xauth"));
    }
    for exe in candidates {
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("list")
            .arg(display)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let out = match tokio::time::timeout(XAUTH_TIMEOUT, cmd.output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                tracing::debug!(exe = %exe.display(), error = %e, "xauth not runnable");
                continue;
            }
            Err(_) => {
                tracing::warn!("xauth timed out");
                return None;
            }
        };
        return parse_xauth_list(&String::from_utf8_lossy(&out.stdout));
    }
    None
}

fn parse_xauth_list(out: &str) -> Option<XAuth> {
    out.lines().find_map(|line| {
        let mut f = line.split_whitespace();
        let (_name, proto, hexdata) = (f.next()?, f.next()?, f.next()?);
        (proto == MIT_COOKIE).then_some(())?;
        let data = hex::decode(hexdata).ok()?;
        Some(XAuth {
            proto: proto.to_owned(),
            data,
        })
    })
}

/// The X11 connection-setup request (X protocol, "Connection Setup").
#[derive(Debug)]
struct Setup {
    big_endian: bool,
    /// Bytes 0..6: byte order, pad, protocol major/minor.
    prefix: [u8; 6],
    proto: Vec<u8>,
    data: Vec<u8>,
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

async fn read_setup<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Setup> {
    let mut head = [0u8; 12];
    r.read_exact(&mut head).await?;
    let big_endian = match head[0] {
        b'B' => true,
        b'l' => false,
        b => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad X11 byte-order byte 0x{b:02x}"),
            ));
        }
    };
    let u16_at = |i: usize| {
        let b = [head[i], head[i + 1]];
        usize::from(if big_endian {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let (n, d) = (u16_at(6), u16_at(8));
    let mut body = vec![0u8; n + pad4(n) + d + pad4(d)];
    r.read_exact(&mut body).await?;
    let mut prefix = [0u8; 6];
    prefix.copy_from_slice(&head[..6]);
    Ok(Setup {
        big_endian,
        prefix,
        proto: body[..n].to_vec(),
        data: body[n + pad4(n)..n + pad4(n) + d].to_vec(),
    })
}

impl Setup {
    /// Check the fake cookie and build the packet for the local server with
    /// the real authorization (or none, if we have no cookie).
    fn substitute(&self, fake: &[u8], real: Option<&XAuth>) -> Result<Vec<u8>, String> {
        if self.proto != MIT_COOKIE.as_bytes() {
            return Err(format!(
                "X11 client used authorization `{}`, not the forwarded cookie",
                String::from_utf8_lossy(&self.proto)
            ));
        }
        if !constant_time_eq(&self.data, fake) {
            return Err("X11 client presented the wrong cookie".into());
        }
        let (proto, data): (&[u8], &[u8]) = match real {
            Some(a) => (a.proto.as_bytes(), &a.data),
            None => (&[], &[]),
        };
        let len16 =
            |n: usize| u16::try_from(n).map_err(|_| "X11 authorization too long".to_owned());
        let enc = |v: u16| {
            if self.big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut out = Vec::with_capacity(12 + proto.len() + data.len() + 6);
        out.extend_from_slice(&self.prefix);
        out.extend_from_slice(&enc(len16(proto.len())?));
        out.extend_from_slice(&enc(len16(data.len())?));
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(proto);
        out.resize(out.len() + pad4(proto.len()), 0);
        out.extend_from_slice(data);
        out.resize(out.len() + pad4(data.len()), 0);
        Ok(out)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn unix(n: u32) -> X11Target {
        X11Target::Unix(PathBuf::from(format!("/tmp/.X11-unix/X{n}")))
    }

    #[test]
    fn display_names_parse() {
        let p = |s: &str| parse_display(s, true).unwrap();
        assert_eq!(p(":0").target, unix(0));
        assert_eq!((p(":1.2").number, p(":1.2").screen), (1, 2));
        assert_eq!(p("unix:3").target, unix(3));
        assert_eq!(p("myhost/unix:4").target, unix(4));
        assert_eq!(
            p("localhost:10.0").target,
            X11Target::Tcp {
                host: "localhost".into(),
                port: 6010
            }
        );
        assert_eq!(
            p("10.0.0.5:0").target,
            X11Target::Tcp {
                host: "10.0.0.5".into(),
                port: 6000
            }
        );
        assert_eq!(
            p("[::1]:1").target,
            X11Target::Tcp {
                host: "::1".into(),
                port: 6001
            }
        );
        assert_eq!(
            p("/private/tmp/com.apple.launchd.abc/org.xquartz:0").target,
            X11Target::Unix(PathBuf::from(
                "/private/tmp/com.apple.launchd.abc/org.xquartz"
            ))
        );
        for bad in ["", "0", ":x", ":0.y", "host::0", ":70000"] {
            assert!(parse_display(bad, true).is_err(), "{bad}");
        }
        // Windows: no sockets, ":0" is TCP localhost:6000.
        assert_eq!(
            parse_display(":0", false).unwrap().target,
            X11Target::Tcp {
                host: "localhost".into(),
                port: 6000
            }
        );
        assert!(parse_display("/tmp/x:0", false).is_err());
    }

    #[test]
    fn xauth_name_uses_unix_for_localhost() {
        let d = parse_display("localhost:10.0", true).unwrap();
        assert_eq!(d.xauth_name(), "unix:10");
        let d = parse_display(":2", true).unwrap();
        assert_eq!(d.xauth_name(), ":2");
    }

    #[test]
    fn xauth_output_parses() {
        let out = "box/unix:0  XDM-AUTHORIZATION-1  00\nbox/unix:0  MIT-MAGIC-COOKIE-1  0011223344556677\n";
        let a = parse_xauth_list(out).unwrap();
        assert_eq!(a.proto, MIT_COOKIE);
        assert_eq!(a.data, vec![0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
        assert!(parse_xauth_list("").is_none());
        assert!(parse_xauth_list("x MIT-MAGIC-COOKIE-1 zz").is_none());
    }

    fn setup_packet(big: bool, proto: &[u8], data: &[u8]) -> Vec<u8> {
        let enc = |v: u16| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut p = vec![if big { b'B' } else { b'l' }, 0];
        p.extend_from_slice(&enc(11));
        p.extend_from_slice(&enc(0));
        p.extend_from_slice(&enc(u16::try_from(proto.len()).unwrap()));
        p.extend_from_slice(&enc(u16::try_from(data.len()).unwrap()));
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(proto);
        p.resize(p.len() + pad4(proto.len()), 0);
        p.extend_from_slice(data);
        p.resize(p.len() + pad4(data.len()), 0);
        p
    }

    #[tokio::test]
    async fn cookie_is_substituted_both_byte_orders() {
        let fake = [7u8; 16];
        let real = XAuth {
            proto: MIT_COOKIE.into(),
            data: vec![9u8; 16],
        };
        for big in [true, false] {
            let mut input = setup_packet(big, MIT_COOKIE.as_bytes(), &fake);
            input.extend_from_slice(b"NEXT"); // must not be consumed
            let mut r = &input[..];
            let s = read_setup(&mut r).await.unwrap();
            assert_eq!(r, b"NEXT");
            let out = s.substitute(&fake, Some(&real)).unwrap();
            assert_eq!(out, setup_packet(big, MIT_COOKIE.as_bytes(), &real.data));

            // No local cookie: forward with no authorization at all.
            let out = s.substitute(&fake, None).unwrap();
            assert_eq!(out, setup_packet(big, b"", b""));
        }
    }

    #[tokio::test]
    async fn wrong_cookie_or_protocol_is_rejected() {
        let fake = [7u8; 16];
        let input = setup_packet(false, MIT_COOKIE.as_bytes(), &[8u8; 16]);
        let s = read_setup(&mut &input[..]).await.unwrap();
        assert!(
            s.substitute(&fake, None)
                .unwrap_err()
                .contains("wrong cookie")
        );

        let input = setup_packet(true, b"XDM-AUTHORIZATION-1", &fake);
        let s = read_setup(&mut &input[..]).await.unwrap();
        assert!(s.substitute(&fake, None).is_err());

        let input = setup_packet(true, MIT_COOKIE.as_bytes(), &fake[..15]);
        let s = read_setup(&mut &input[..]).await.unwrap();
        assert!(s.substitute(&fake, None).is_err());

        let mut bad = setup_packet(true, b"", b"");
        bad[0] = b'X';
        assert!(read_setup(&mut &bad[..]).await.is_err());
        // Truncated packet: an error, not a hang.
        let short = setup_packet(true, MIT_COOKIE.as_bytes(), &fake);
        assert!(read_setup(&mut &short[..20]).await.is_err());
    }
}
