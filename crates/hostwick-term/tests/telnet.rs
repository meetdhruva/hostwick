//! Telnet against an in-test fake server (always), and against a real
//! telnetd when `HOSTWICK_TELNETD=host:port` points at one whose login
//! program is a shell (e.g. `busybox telnetd -F -p 2241 -l /bin/sh`).

use std::time::Duration;

use bytes::Bytes;
use hostwick_term::telnet::{
    self, DO, DONT, IAC, OPT_ECHO, OPT_NAWS, OPT_SGA, OPT_TTYPE, SB, SE, TelnetOptions, WILL, WONT,
};
use hostwick_term::{Control, ShellCommand, ShellEvent, TermError, TerminalSession};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const T: Duration = Duration::from_secs(5);

async fn read_exact_n(s: &mut TcpStream, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    tokio::time::timeout(T, s.read_exact(&mut buf))
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {n} bytes"))
        .unwrap_or_else(|e| panic!("{e}"));
    buf
}

async fn next_output(session: &mut TerminalSession) -> Vec<u8> {
    match tokio::time::timeout(T, session.recv()).await {
        Ok(Some(ShellEvent::Stdout(b))) => b.to_vec(),
        other => panic!("expected output, got {other:?}"),
    }
}

async fn fake_server() -> (TcpListener, TelnetOptions) {
    let l = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let port = l.local_addr().unwrap_or_else(|e| panic!("{e}")).port();
    let mut o = TelnetOptions::new("127.0.0.1", port);
    o.term = "vt220".into();
    o.cols = 100;
    o.rows = 40;
    (l, o)
}

#[tokio::test]
async fn negotiates_resizes_and_escapes_against_a_fake_server() {
    let (listener, opts) = fake_server().await;
    let mut session = telnet::connect(&opts)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(session.can_break());
    let (mut srv, _) = listener.accept().await.unwrap_or_else(|e| panic!("{e}"));

    // The client opens with its offers.
    let hello = read_exact_n(&mut srv, 15).await;
    assert_eq!(
        hello,
        vec![
            IAC, WILL, OPT_NAWS, IAC, WILL, OPT_TTYPE, IAC, WILL, OPT_SGA, IAC, DO, OPT_ECHO, IAC,
            DO, OPT_SGA
        ]
    );

    // Server accepts NAWS (→ window size), asks for the terminal type,
    // offers echo, and tries an option we don't know in both directions.
    srv.write_all(&[
        IAC, DO, OPT_NAWS, IAC, WILL, OPT_ECHO, IAC, SB, OPT_TTYPE, 1, IAC, SE, IAC, DO, 42, IAC,
        WILL, 42,
    ])
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let mut want = vec![IAC, SB, OPT_NAWS, 0, 100, 0, 40, IAC, SE];
    // TTYPE was only offered (WantYes), not yet agreed: the SEND is ignored.
    want.extend_from_slice(&[IAC, WONT, 42, IAC, DONT, 42]);
    assert_eq!(read_exact_n(&mut srv, want.len()).await, want);

    // Agree to TTYPE, then ask again: now it is answered.
    srv.write_all(&[IAC, DO, OPT_TTYPE, IAC, SB, OPT_TTYPE, 1, IAC, SE])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut want = vec![IAC, SB, OPT_TTYPE, 0];
    want.extend_from_slice(b"vt220");
    want.extend_from_slice(&[IAC, SE]);
    assert_eq!(read_exact_n(&mut srv, want.len()).await, want);

    // Resize → NAWS; 255 rows is escaped.
    session
        .send(ShellCommand::Resize {
            cols: 120,
            rows: 255,
        })
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        read_exact_n(&mut srv, 10).await,
        vec![IAC, SB, OPT_NAWS, 0, 120, 0, IAC, IAC, IAC, SE]
    );

    // Input: Enter becomes CR LF, 0xFF is doubled; the server echoes, so no
    // local echo appears.
    session
        .send(ShellCommand::Data(Bytes::from_static(b"ls\r\xff")))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read_exact_n(&mut srv, 6).await, b"ls\r\n\xff\xff");

    // Output: IAC IAC → 0xFF, CR NUL → CR, negotiation removed.
    srv.write_all(b"hi\xff\xff\r\0there\r\n\xff\xfb\x01")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut got = Vec::new();
    while got.len() < 11 {
        got.extend(next_output(&mut session).await);
    }
    assert_eq!(got, b"hi\xff\rthere\r\n");

    // BREAK.
    session
        .control(Control::Break)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read_exact_n(&mut srv, 2).await, vec![IAC, 243]);

    // Server turns echo off: client acknowledges once and echoes locally.
    srv.write_all(&[IAC, WONT, OPT_ECHO])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read_exact_n(&mut srv, 3).await, vec![IAC, DONT, OPT_ECHO]);
    session
        .send(ShellCommand::Data(Bytes::from_static(b"ab\r")))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read_exact_n(&mut srv, 4).await, b"ab\r\n");
    assert_eq!(next_output(&mut session).await, b"ab\r\n");

    // Server hangs up: Closed.
    drop(srv);
    loop {
        match tokio::time::timeout(T, session.recv()).await {
            Ok(Some(ShellEvent::Closed)) => break,
            Ok(Some(_)) => {}
            other => panic!("expected Closed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn closing_the_session_hangs_up() {
    let (listener, opts) = fake_server().await;
    let session = telnet::connect(&opts)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let (mut srv, _) = listener.accept().await.unwrap_or_else(|e| panic!("{e}"));
    read_exact_n(&mut srv, 15).await;
    drop(session);
    let mut buf = [0u8; 16];
    let n = tokio::time::timeout(T, srv.read(&mut buf))
        .await
        .unwrap_or_else(|_| panic!("no EOF"))
        .unwrap_or(0);
    assert_eq!(n, 0, "expected EOF after the session was dropped");
}

#[tokio::test]
async fn connection_errors_are_clear() {
    // Nothing listens on the port we just released.
    let (listener, opts) = fake_server().await;
    drop(listener);
    let err = telnet::connect(&opts).await.err();
    match err {
        Some(TermError::Connect { ref host, port, .. }) => {
            assert_eq!((host.as_str(), port), ("127.0.0.1", opts.port));
            let msg = err.map(|e| e.to_string()).unwrap_or_default();
            assert!(msg.starts_with("cannot connect to 127.0.0.1:"), "{msg}");
        }
        other => panic!("expected a connect error, got {other:?}"),
    }

    // A non-routable address times out within the connect timeout.
    let mut o = TelnetOptions::new("10.255.255.1", 23);
    o.connect_timeout = Duration::from_millis(300);
    let started = std::time::Instant::now();
    match telnet::connect(&o).await {
        Err(TermError::Timeout { .. }) | Err(TermError::Connect { .. }) => {}
        Ok(_) => panic!("connected to a blackhole address?"),
        Err(e) => panic!("unexpected error {e}"),
    }
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// Real server: `HOSTWICK_TELNETD=127.0.0.1:2241` with a shell as the login
/// program, e.g. `busybox telnetd -F -p 2241 -l /bin/sh`.
#[tokio::test]
async fn real_telnetd_shell() {
    let Ok(addr) = std::env::var("HOSTWICK_TELNETD") else {
        eprintln!("HOSTWICK_TELNETD not set; skipping");
        return;
    };
    let (host, port) = addr
        .rsplit_once(':')
        .and_then(|(h, p)| Some((h.to_owned(), p.parse::<u16>().ok()?)))
        .unwrap_or_else(|| panic!("HOSTWICK_TELNETD must be host:port"));
    let mut o = TelnetOptions::new(host, port);
    o.cols = 91;
    o.rows = 33;
    let mut s = telnet::connect(&o).await.unwrap_or_else(|e| panic!("{e}"));
    let mut seen = Vec::new();
    let mut wait_for = async |s: &mut TerminalSession, needle: &str| {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !String::from_utf8_lossy(&seen).contains(needle) {
            match tokio::time::timeout_at(deadline, s.recv()).await {
                Ok(Some(ShellEvent::Stdout(b) | ShellEvent::Stderr(b))) => {
                    seen.extend_from_slice(&b)
                }
                other => panic!(
                    "waiting for {needle:?}: {other:?}; output so far: {}",
                    String::from_utf8_lossy(&seen)
                ),
            }
        }
        seen.clear();
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    s.send(ShellCommand::Data(Bytes::from_static(
        b"stty size; echo marker-$((6*7))\r",
    )))
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    wait_for(&mut s, "33 91").await;
    s.send(ShellCommand::Resize {
        cols: 132,
        rows: 50,
    })
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    s.send(ShellCommand::Data(Bytes::from_static(
        b"stty size; echo marker-$((6*7))\r",
    )))
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    wait_for(&mut s, "50 132").await;
    wait_for(&mut s, "marker-42").await;
    s.send(ShellCommand::Data(Bytes::from_static(b"exit\r")))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    loop {
        match tokio::time::timeout(T, s.recv()).await {
            Ok(Some(ShellEvent::Closed)) => break,
            Ok(Some(_)) => {}
            other => panic!("expected Closed after exit, got {other:?}"),
        }
    }
}
