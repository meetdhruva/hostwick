//! Telnet client (RFC 854/855) with the options a terminal needs: ECHO (857),
//! SUPPRESS-GO-AHEAD (858), BINARY (856), TERMINAL-TYPE (1091) and NAWS
//! (1073, window size, re-sent on every resize).
//!
//! Option negotiation follows RFC 1143's "Q method", so a request is only
//! ever answered when it changes an option's state and the two ends can
//! never loop. Unknown options are refused (`DONT`/`WONT`) once.
//!
//! [`TelnetEngine`] is the protocol, with no I/O, so it is unit-tested byte
//! by byte; [`connect`]/[`start`] run it over a TCP stream (or any stream,
//! e.g. one already set up through a proxy).

use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::{
    Control, QUEUE, ShellCommand, ShellEvent, TermError, TerminalSession, TransportKind,
    failure_text,
};

// Commands (RFC 854).
pub const IAC: u8 = 255;
pub const DONT: u8 = 254;
pub const DO: u8 = 253;
pub const WONT: u8 = 252;
pub const WILL: u8 = 251;
pub const SB: u8 = 250;
pub const GA: u8 = 249;
pub const BRK: u8 = 243;
pub const NOP: u8 = 241;
pub const SE: u8 = 240;

// Options.
pub const OPT_BINARY: u8 = 0;
pub const OPT_ECHO: u8 = 1;
pub const OPT_SGA: u8 = 3;
pub const OPT_TTYPE: u8 = 24;
pub const OPT_NAWS: u8 = 31;

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// Longest subnegotiation we buffer; anything longer is a broken or hostile
/// peer and is dropped.
const MAX_SB: usize = 4096;

/// What the Enter key (a CR from the terminal) is sent as when the
/// connection is not in BINARY mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Newline {
    /// The Telnet end-of-line, `CR LF` (PuTTY's default).
    #[default]
    CrLf,
    /// A bare carriage return, `CR NUL`.
    CrNul,
}

/// RFC 1143 option state, one per side per option.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Q {
    #[default]
    No,
    Yes,
    /// We asked to enable it and wait for the answer. (We never ask to
    /// disable an option, so RFC 1143's WANTNO state is not needed.)
    WantYes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rx {
    Data,
    Iac,
    /// After `IAC WILL|WONT|DO|DONT`, waiting for the option byte.
    Verb(u8),
    Sb,
    SbIac,
}

/// The telnet protocol state machine.
#[derive(Debug, Clone)]
pub struct TelnetEngine {
    /// Options we perform (answered with DO/DONT by the peer).
    us: [Q; 256],
    /// Options the peer performs (answered with WILL/WONT by the peer).
    him: [Q; 256],
    rx: Rx,
    sb: Vec<u8>,
    /// The previous data byte was CR (in NVT mode a following NUL is dropped).
    rx_cr: bool,
    /// We just turned an input CR into a newline; swallow a following LF.
    tx_cr: bool,
    term: String,
    cols: u16,
    rows: u16,
    newline: Newline,
}

impl TelnetEngine {
    pub fn new(term: &str, cols: u16, rows: u16, newline: Newline) -> Self {
        Self {
            us: [Q::No; 256],
            him: [Q::No; 256],
            rx: Rx::Data,
            sb: Vec::new(),
            rx_cr: false,
            tx_cr: false,
            term: if term.trim().is_empty() {
                "xterm-256color".to_owned()
            } else {
                term.trim().to_owned()
            },
            cols: cols.max(1),
            rows: rows.max(1),
            newline,
        }
    }

    /// Options we offer or ask for up front (like PuTTY in active mode):
    /// we will send our window size and terminal type and suppress go-ahead,
    /// and we'd like the server to echo and suppress go-ahead.
    pub fn start(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for opt in [OPT_NAWS, OPT_TTYPE, OPT_SGA] {
            self.us[opt as usize] = Q::WantYes;
            out.extend_from_slice(&[IAC, WILL, opt]);
        }
        for opt in [OPT_ECHO, OPT_SGA] {
            self.him[opt as usize] = Q::WantYes;
            out.extend_from_slice(&[IAC, DO, opt]);
        }
        out
    }

    /// Whether the server echoes what we type. When it does not (plain NVT,
    /// or a line-mode service), the client echoes locally.
    pub fn remote_echo(&self) -> bool {
        self.him[OPT_ECHO as usize] == Q::Yes
    }

    /// Our side transmits 8-bit binary (no CR/NUL translation).
    pub fn binary_out(&self) -> bool {
        self.us[OPT_BINARY as usize] == Q::Yes
    }

    pub fn binary_in(&self) -> bool {
        self.him[OPT_BINARY as usize] == Q::Yes
    }

    pub fn naws_enabled(&self) -> bool {
        self.us[OPT_NAWS as usize] == Q::Yes
    }

    fn accepts_us(opt: u8) -> bool {
        matches!(opt, OPT_BINARY | OPT_SGA | OPT_TTYPE | OPT_NAWS)
    }

    fn accepts_him(opt: u8) -> bool {
        matches!(opt, OPT_BINARY | OPT_ECHO | OPT_SGA)
    }

    /// Process bytes from the server: terminal data goes to `data`, protocol
    /// answers to `reply`. State carries over between calls, so sequences
    /// may be split anywhere.
    pub fn receive(&mut self, input: &[u8], data: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &b in input {
            match self.rx {
                Rx::Data => {
                    if b == IAC {
                        self.rx = Rx::Iac;
                    } else {
                        self.data_byte(b, data);
                    }
                }
                Rx::Iac => {
                    self.rx = Rx::Data;
                    match b {
                        IAC => self.data_byte(IAC, data),
                        WILL | WONT | DO | DONT => self.rx = Rx::Verb(b),
                        SB => {
                            self.sb.clear();
                            self.rx = Rx::Sb;
                        }
                        // NOP, GA, DM, AYT, ...: nothing to do for a client.
                        _ => {}
                    }
                }
                Rx::Verb(verb) => {
                    self.rx = Rx::Data;
                    self.negotiate(verb, b, reply);
                }
                Rx::Sb => {
                    if b == IAC {
                        self.rx = Rx::SbIac;
                    } else if self.sb.len() < MAX_SB {
                        self.sb.push(b);
                    }
                }
                Rx::SbIac => match b {
                    SE => {
                        self.rx = Rx::Data;
                        let sb = std::mem::take(&mut self.sb);
                        self.subnegotiation(&sb, reply);
                    }
                    IAC => {
                        if self.sb.len() < MAX_SB {
                            self.sb.push(IAC);
                        }
                        self.rx = Rx::Sb;
                    }
                    // Malformed: an IAC command inside SB. Abandon the SB.
                    _ => {
                        self.sb.clear();
                        self.rx = Rx::Data;
                    }
                },
            }
        }
    }

    fn data_byte(&mut self, b: u8, data: &mut Vec<u8>) {
        if !self.binary_in() {
            // NVT: CR NUL means a bare CR; CR LF is a newline (kept as is).
            if self.rx_cr && b == 0 {
                self.rx_cr = false;
                return;
            }
            self.rx_cr = b == b'\r';
        }
        data.push(b);
    }

    fn negotiate(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        let i = opt as usize;
        match verb {
            WILL => match self.him[i] {
                Q::No => {
                    if Self::accepts_him(opt) {
                        self.him[i] = Q::Yes;
                        reply.extend_from_slice(&[IAC, DO, opt]);
                    } else {
                        reply.extend_from_slice(&[IAC, DONT, opt]);
                    }
                }
                Q::Yes => {}
                Q::WantYes => self.him[i] = Q::Yes,
            },
            WONT => match self.him[i] {
                Q::No => {}
                Q::Yes => {
                    self.him[i] = Q::No;
                    reply.extend_from_slice(&[IAC, DONT, opt]);
                }
                Q::WantYes => self.him[i] = Q::No,
            },
            DO => match self.us[i] {
                Q::No => {
                    if Self::accepts_us(opt) {
                        self.us[i] = Q::Yes;
                        reply.extend_from_slice(&[IAC, WILL, opt]);
                        self.enabled_us(opt, reply);
                    } else {
                        reply.extend_from_slice(&[IAC, WONT, opt]);
                    }
                }
                Q::Yes => {}
                Q::WantYes => {
                    self.us[i] = Q::Yes;
                    self.enabled_us(opt, reply);
                }
            },
            DONT => match self.us[i] {
                Q::No => {}
                Q::Yes => {
                    self.us[i] = Q::No;
                    reply.extend_from_slice(&[IAC, WONT, opt]);
                }
                Q::WantYes => self.us[i] = Q::No,
            },
            _ => {}
        }
    }

    /// Something to send as soon as one of our options becomes active.
    fn enabled_us(&mut self, opt: u8, reply: &mut Vec<u8>) {
        if opt == OPT_NAWS {
            self.push_naws(reply);
        }
    }

    fn subnegotiation(&mut self, sb: &[u8], reply: &mut Vec<u8>) {
        if let [OPT_TTYPE, TTYPE_SEND, ..] = sb
            && self.us[OPT_TTYPE as usize] == Q::Yes
        {
            reply.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_IS]);
            escape_into(self.term.as_bytes(), reply);
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    fn push_naws(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[IAC, SB, OPT_NAWS]);
        let mut size = [0u8; 4];
        size[..2].copy_from_slice(&self.cols.to_be_bytes());
        size[2..].copy_from_slice(&self.rows.to_be_bytes());
        escape_into(&size, out);
        out.extend_from_slice(&[IAC, SE]);
    }

    /// The window size changed. Returns the NAWS message to send, if the
    /// server asked for window sizes.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if (cols, rows) == (self.cols, self.rows) {
            return None;
        }
        self.cols = cols;
        self.rows = rows;
        self.naws_enabled().then(|| {
            let mut out = Vec::new();
            self.push_naws(&mut out);
            out
        })
    }

    /// Encode keyboard input for the wire: IAC doubled, and outside BINARY
    /// mode Enter (CR) becomes the telnet newline.
    pub fn encode_input(&mut self, input: &[u8], out: &mut Vec<u8>) {
        let binary = self.binary_out();
        for &b in input {
            if b == IAC {
                self.tx_cr = false;
                out.extend_from_slice(&[IAC, IAC]);
                continue;
            }
            if binary {
                out.push(b);
                continue;
            }
            if self.tx_cr && b == b'\n' {
                // Already sent as part of CR LF.
                self.tx_cr = false;
                continue;
            }
            self.tx_cr = false;
            match b {
                b'\r' => {
                    out.push(b'\r');
                    out.push(match self.newline {
                        Newline::CrLf => b'\n',
                        Newline::CrNul => 0,
                    });
                    self.tx_cr = true;
                }
                _ => out.push(b),
            }
        }
    }
}

/// Copy `bytes`, doubling any IAC.
fn escape_into(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        if b == IAC {
            out.push(IAC);
        }
        out.push(b);
    }
}

/// What the client shows for typed input when the server does not echo:
/// printable text as is, Enter as a new line, Backspace erases. Escape
/// sequences (arrow and function keys) and other control keys are not shown.
pub fn local_echo(input: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        i += 1;
        match b {
            b'\r' => out.extend_from_slice(b"\r\n"),
            0x7f | 0x08 => out.extend_from_slice(b"\x08 \x08"),
            b'\t' => out.push(b'\t'),
            0x1b => match input.get(i) {
                Some(b'[') => {
                    i += 1;
                    while i < input.len() && !(0x40..=0x7e).contains(&input[i]) {
                        i += 1;
                    }
                    i += 1;
                }
                Some(b'O') => i += 2,
                Some(_) => i += 1,
                None => {}
            },
            0x00..=0x1f => {}
            _ => out.push(b),
        }
    }
}

/// How to reach a telnet server.
#[derive(Debug, Clone)]
pub struct TelnetOptions {
    pub host: String,
    pub port: u16,
    /// Terminal type announced with TERMINAL-TYPE, e.g. `xterm-256color`.
    pub term: String,
    pub cols: u16,
    pub rows: u16,
    pub connect_timeout: Duration,
    /// TCP keepalive interval; `None` disables.
    pub keepalive: Option<Duration>,
    pub newline: Newline,
}

impl TelnetOptions {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            term: "xterm-256color".into(),
            cols: 80,
            rows: 24,
            connect_timeout: Duration::from_secs(15),
            keepalive: Some(Duration::from_secs(30)),
            newline: Newline::CrLf,
        }
    }

    /// From a saved host: its address, terminal type and keepalive. A
    /// profile still on the SSH default port 22 is taken to mean the telnet
    /// default, 23.
    pub fn from_profile(profile: &hostwick_core::HostProfile, cols: u32, rows: u32) -> Self {
        let port = if profile.port == hostwick_core::DEFAULT_PORT {
            23
        } else {
            profile.port
        };
        let mut o = Self::new(profile.host.trim(), port);
        o.term = profile.term.clone();
        o.cols = u16::try_from(cols).unwrap_or(u16::MAX);
        o.rows = u16::try_from(rows).unwrap_or(u16::MAX);
        o.keepalive = (profile.keepalive_secs > 0)
            .then(|| Duration::from_secs(profile.keepalive_secs.into()));
        o
    }
}

/// Connect over TCP and start the session.
pub async fn connect(opts: &TelnetOptions) -> Result<TerminalSession, TermError> {
    let tcp = connect_tcp(opts).await?;
    Ok(start(tcp, opts))
}

/// Open the TCP connection (with timeout, `TCP_NODELAY` and keepalive)
/// without starting telnet on it.
pub async fn connect_tcp(opts: &TelnetOptions) -> Result<TcpStream, TermError> {
    if opts.host.is_empty() {
        return Err(TermError::InvalidSettings("no host name".into()));
    }
    let tcp = match tokio::time::timeout(
        opts.connect_timeout,
        TcpStream::connect((opts.host.as_str(), opts.port)),
    )
    .await
    {
        Ok(Ok(t)) => t,
        Ok(Err(reason)) => {
            return Err(TermError::Connect {
                host: opts.host.clone(),
                port: opts.port,
                reason,
            });
        }
        Err(_) => {
            return Err(TermError::Timeout {
                host: opts.host.clone(),
                port: opts.port,
                after: opts.connect_timeout,
            });
        }
    };
    tune_socket(&tcp, opts.keepalive);
    Ok(tcp)
}

/// `TCP_NODELAY` (keystrokes go out at once) and TCP keepalive, so a dead
/// link is noticed even when nobody types.
pub fn tune_socket(tcp: &TcpStream, keepalive: Option<Duration>) {
    let _ = tcp.set_nodelay(true);
    if let Some(every) = keepalive {
        let ka = socket2::TcpKeepalive::new()
            .with_time(every)
            .with_interval(every);
        #[cfg(not(any(target_os = "windows", target_os = "openbsd", target_os = "redox")))]
        let ka = ka.with_retries(4);
        if let Err(e) = socket2::SockRef::from(tcp).set_tcp_keepalive(&ka) {
            tracing::debug!(error = %e, "cannot enable TCP keepalive");
        }
    }
}

/// Run telnet over an established stream (TCP, or a proxied connection).
pub fn start<S>(stream: S, opts: &TelnetOptions) -> TerminalSession
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (in_tx, in_rx) = mpsc::channel::<ShellCommand>(QUEUE);
    let (out_tx, out_rx) = mpsc::channel::<ShellEvent>(QUEUE);
    let (ctl_tx, ctl_rx) = mpsc::channel::<Control>(8);
    let engine = TelnetEngine::new(&opts.term, opts.cols, opts.rows, opts.newline);
    let peer = format!("{}:{}", opts.host, opts.port);
    tokio::spawn(pump(stream, engine, in_rx, out_tx, ctl_rx, peer));
    TerminalSession::new(in_tx, out_rx, TransportKind::Telnet).with_control(ctl_tx)
}

async fn pump<S>(
    stream: S,
    mut engine: TelnetEngine,
    mut in_rx: mpsc::Receiver<ShellCommand>,
    out_tx: mpsc::Sender<ShellEvent>,
    mut ctl_rx: mpsc::Receiver<Control>,
    peer: String,
) where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let mut buf = vec![0u8; 16 * 1024];
    let mut data = Vec::with_capacity(16 * 1024);
    let mut reply = Vec::new();
    let mut ctl_open = true;

    let hello = engine.start();
    let failed = |e: std::io::Error| format!("connection to {peer} failed: {e}");
    let mut failure: Option<String> = wr.write_all(&hello).await.err().map(failed);

    while failure.is_none() {
        tokio::select! {
            n = rd.read(&mut buf) => {
                let n = match n {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => { failure = Some(failed(e)); break; }
                };
                data.clear();
                reply.clear();
                engine.receive(&buf[..n], &mut data, &mut reply);
                if !reply.is_empty() && let Err(e) = wr.write_all(&reply).await {
                    failure = Some(failed(e));
                    break;
                }
                if !data.is_empty()
                    && out_tx.send(ShellEvent::Stdout(Bytes::copy_from_slice(&data))).await.is_err()
                {
                    // Consumer gone.
                    let _ = wr.shutdown().await;
                    return;
                }
            }
            cmd = in_rx.recv() => {
                let mut wire = Vec::new();
                match cmd {
                    Some(ShellCommand::Data(b)) => {
                        engine.encode_input(&b, &mut wire);
                        if !engine.remote_echo() {
                            let mut echo = Vec::new();
                            local_echo(&b, &mut echo);
                            if !echo.is_empty() {
                                let _ = out_tx.send(ShellEvent::Stdout(Bytes::from(echo))).await;
                            }
                        }
                    }
                    Some(ShellCommand::Resize { cols, rows }) => {
                        if let Some(naws) = engine.resize(
                            u16::try_from(cols).unwrap_or(u16::MAX),
                            u16::try_from(rows).unwrap_or(u16::MAX),
                        ) {
                            wire = naws;
                        }
                    }
                    Some(ShellCommand::Eof) => {}
                    Some(ShellCommand::Close) | None => {
                        let _ = wr.shutdown().await;
                        break;
                    }
                }
                if !wire.is_empty() && let Err(e) = wr.write_all(&wire).await {
                    failure = Some(failed(e));
                }
            }
            c = ctl_rx.recv(), if ctl_open => match c {
                Some(Control::Break) => {
                    if let Err(e) = wr.write_all(&[IAC, BRK]).await {
                        failure = Some(failed(e));
                    }
                }
                None => ctl_open = false,
            },
        }
    }
    if let Some(msg) = failure {
        tracing::info!(%peer, "{msg}");
        let _ = out_tx.send(ShellEvent::Stderr(failure_text(&msg))).await;
    }
    let _ = out_tx.send(ShellEvent::Closed).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eng() -> TelnetEngine {
        TelnetEngine::new("xterm-256color", 80, 24, Newline::CrLf)
    }

    fn rx(e: &mut TelnetEngine, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut d, mut r) = (Vec::new(), Vec::new());
        e.receive(bytes, &mut d, &mut r);
        (d, r)
    }

    #[test]
    fn accepts_known_options_and_refuses_unknown_ones_once() {
        let mut e = eng();
        let (d, r) = rx(
            &mut e,
            &[
                IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SGA, IAC, DO, OPT_TTYPE, IAC, DO, 39, IAC,
                WILL, 200,
            ],
        );
        assert!(d.is_empty());
        assert_eq!(
            r,
            vec![
                IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, WILL, OPT_TTYPE, IAC, WONT, 39, IAC,
                DONT, 200
            ]
        );
        assert!(e.remote_echo());
        // Repeats change nothing and are not answered (no loops).
        let (_, r) = rx(&mut e, &[IAC, WILL, OPT_ECHO, IAC, DO, OPT_TTYPE]);
        assert!(r.is_empty());
        // Refused options stay refused without a reply to a repeated refusal.
        let (_, r) = rx(&mut e, &[IAC, WONT, 200, IAC, DONT, 39]);
        assert!(r.is_empty());
        // Turning an option off is acknowledged once.
        let (_, r) = rx(&mut e, &[IAC, WONT, OPT_ECHO, IAC, WONT, OPT_ECHO]);
        assert_eq!(r, vec![IAC, DONT, OPT_ECHO]);
        assert!(!e.remote_echo());
    }

    #[test]
    fn our_own_requests_are_not_answered_again() {
        let mut e = eng();
        let hello = e.start();
        assert_eq!(
            hello,
            vec![
                IAC, WILL, OPT_NAWS, IAC, WILL, OPT_TTYPE, IAC, WILL, OPT_SGA, IAC, DO, OPT_ECHO,
                IAC, DO, OPT_SGA
            ]
        );
        // Server agrees to some, refuses others: only NAWS data goes out.
        let (_, r) = rx(
            &mut e,
            &[
                IAC, DO, OPT_NAWS, IAC, DONT, OPT_TTYPE, IAC, WILL, OPT_ECHO, IAC, WONT, OPT_SGA,
                IAC, DO, OPT_SGA,
            ],
        );
        assert_eq!(r, vec![IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE]);
        assert!(e.remote_echo() && e.naws_enabled());
        // A later DO TTYPE (server changed its mind) is accepted normally.
        let (_, r) = rx(&mut e, &[IAC, DO, OPT_TTYPE]);
        assert_eq!(r, vec![IAC, WILL, OPT_TTYPE]);
    }

    #[test]
    fn naws_on_enable_and_on_resize_with_escaping() {
        let mut e = eng();
        assert_eq!(e.resize(100, 30), None, "NAWS not negotiated yet");
        let (_, r) = rx(&mut e, &[IAC, DO, OPT_NAWS]);
        assert_eq!(
            r,
            vec![
                IAC, WILL, OPT_NAWS, IAC, SB, OPT_NAWS, 0, 100, 0, 30, IAC, SE
            ]
        );
        assert_eq!(e.resize(100, 30), None, "unchanged size");
        // 255 columns must be sent as IAC IAC.
        assert_eq!(
            e.resize(255, 50),
            Some(vec![IAC, SB, OPT_NAWS, 0, IAC, IAC, 0, 50, IAC, SE])
        );
        assert_eq!(
            e.resize(300, 1000),
            Some(vec![IAC, SB, OPT_NAWS, 1, 44, 3, 232, IAC, SE])
        );
    }

    #[test]
    fn terminal_type_is_sent_when_asked() {
        let mut e = TelnetEngine::new("vt100", 80, 24, Newline::CrLf);
        // Not agreed yet: ignored.
        let (_, r) = rx(&mut e, &[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
        assert!(r.is_empty());
        rx(&mut e, &[IAC, DO, OPT_TTYPE]);
        // Split across reads.
        let (_, r1) = rx(&mut e, &[IAC, SB, OPT_TTYPE]);
        let (_, r2) = rx(&mut e, &[TTYPE_SEND, IAC, SE, b'x']);
        assert!(r1.is_empty());
        let mut want = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
        want.extend_from_slice(b"vt100");
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(r2, want);
    }

    #[test]
    fn data_unescaping_and_nvt_newlines() {
        let mut e = eng();
        let (d, _) = rx(&mut e, b"a\xff\xffb\r\0c\r\nd\r");
        assert_eq!(d, b"a\xffb\rc\r\nd\r");
        // CR NUL split across reads.
        let (d, _) = rx(&mut e, b"\0e");
        assert_eq!(d, b"e");
        // IAC split from its command; NOP/GA dropped.
        let (d, _) = rx(&mut e, &[b'x', IAC]);
        let (d2, _) = rx(&mut e, &[NOP, b'y', IAC, GA]);
        assert_eq!((d, d2), (b"x".to_vec(), b"y".to_vec()));
        // In BINARY mode NUL after CR is data.
        rx(&mut e, &[IAC, WILL, OPT_BINARY]);
        assert!(e.binary_in());
        let (d, _) = rx(&mut e, b"\r\0");
        assert_eq!(d, b"\r\0");
    }

    #[test]
    fn input_encoding() {
        let mut e = eng();
        let mut out = Vec::new();
        e.encode_input(b"ls\r", &mut out);
        assert_eq!(out, b"ls\r\n");
        out.clear();
        e.encode_input(b"\xff", &mut out);
        assert_eq!(out, vec![IAC, IAC]);
        out.clear();
        // CR LF from a paste is not doubled, even across calls.
        e.encode_input(b"a\r", &mut out);
        e.encode_input(b"\nb", &mut out);
        assert_eq!(out, b"a\r\nb");
        let mut e = TelnetEngine::new("x", 80, 24, Newline::CrNul);
        out.clear();
        e.encode_input(b"\r", &mut out);
        assert_eq!(out, b"\r\0");
        // BINARY: no translation, IAC still doubled.
        rx(&mut e, &[IAC, DO, OPT_BINARY]);
        assert!(e.binary_out());
        out.clear();
        e.encode_input(b"\r\xff", &mut out);
        assert_eq!(out, vec![b'\r', IAC, IAC]);
    }

    #[test]
    fn subnegotiation_overflow_and_malformed_are_dropped() {
        let mut e = eng();
        let mut flood = vec![IAC, SB, 99];
        flood.extend(std::iter::repeat_n(b'z', MAX_SB * 2));
        flood.extend_from_slice(&[IAC, SE, b'k']);
        let (d, r) = rx(&mut e, &flood);
        assert_eq!((d, r), (b"k".to_vec(), vec![]));
        let (d, _) = rx(&mut e, &[IAC, SB, 24, IAC, WILL, b'q']);
        assert_eq!(d, b"q");
    }

    #[test]
    fn local_echo_is_readable() {
        let mut out = Vec::new();
        local_echo(b"hi\x7f\r\x1b[A\x1b[1;5Cx\x1bOPy\x1bz\x03", &mut out);
        assert_eq!(out, b"hi\x08 \x08\r\nxy");
    }
}
