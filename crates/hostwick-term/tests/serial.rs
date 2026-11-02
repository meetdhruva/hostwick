//! Serial sessions over a pseudo-terminal pair made by socat (Unix only).
//! Enable with `HOSTWICK_SERIAL_E2E=1` (needs `socat` on PATH).

#![cfg(unix)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use hostwick_core::SerialSettings;
use hostwick_term::{Control, ShellCommand, ShellEvent, TermError, serial};

struct Socat {
    child: std::process::Child,
    a: PathBuf,
    b: PathBuf,
    _dir: PathBuf,
}

impl Drop for Socat {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}

fn socat_pair() -> Option<Socat> {
    if std::env::var_os("HOSTWICK_SERIAL_E2E").is_none() {
        eprintln!("HOSTWICK_SERIAL_E2E not set; skipping");
        return None;
    }
    let dir = std::env::temp_dir().join(format!("hw-serial-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let (a, b) = (dir.join("a"), dir.join("b"));
    let child = std::process::Command::new("socat")
        .arg(format!("pty,raw,echo=0,link={}", a.display()))
        .arg(format!("pty,raw,echo=0,link={}", b.display()))
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot run socat: {e}"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !(a.exists() && b.exists()) {
        assert!(
            std::time::Instant::now() < deadline,
            "socat did not create the ptys"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(Socat {
        child,
        a,
        b,
        _dir: dir,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn talks_both_ways_over_a_pty_pair() {
    let Some(pair) = socat_pair() else { return };
    let settings = SerialSettings {
        baud: 115_200,
        ..SerialSettings::default()
    };
    let port = pair.a.to_string_lossy().into_owned();
    let mut session = serial::open(&port, &settings)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(session.can_break());

    // The "device" end, opened raw.
    let mut dev = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pair.b)
        .unwrap_or_else(|e| panic!("{e}"));

    // Device → terminal.
    dev.write_all(b"login: ").unwrap_or_else(|e| panic!("{e}"));
    let mut got = Vec::new();
    while got.len() < 7 {
        match tokio::time::timeout(Duration::from_secs(5), session.recv()).await {
            Ok(Some(ShellEvent::Stdout(b))) => got.extend_from_slice(&b),
            other => panic!("expected output, got {other:?}"),
        }
    }
    assert_eq!(got, b"login: ");

    // Terminal → device; bytes go out untranslated (Enter is CR).
    session
        .send(ShellCommand::Data(Bytes::from_static(b"root\r\xff")))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut buf = [0u8; 6];
    let read = tokio::task::spawn_blocking(move || dev.read_exact(&mut buf).map(|_| (buf, dev)))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let (buf, dev) = read.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(&buf, b"root\r\xff");

    // BREAK on a pty: must not crash or end the session.
    session
        .control(Control::Break)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    tokio::time::sleep(Duration::from_millis(500)).await;
    session
        .send(ShellCommand::Data(Bytes::from_static(b"x")))
        .await
        .unwrap_or_else(|e| panic!("still open after BREAK: {e}"));

    // Opening the same port twice: busy (ports are opened exclusively).
    match serial::open(&port, &settings).await {
        Err(TermError::SerialBusy(_)) => {}
        Err(e) => panic!("expected busy, got {e}"),
        Ok(_) => panic!("opened an exclusively held port twice"),
    }

    // The other end goes away (adapter unplugged): the session ends with a
    // reason on screen.
    drop(dev);
    drop(pair);
    let mut ended = false;
    let mut text = Vec::new();
    for _ in 0..50 {
        match tokio::time::timeout(Duration::from_secs(5), session.recv()).await {
            Ok(Some(ShellEvent::Closed)) | Ok(None) => {
                ended = true;
                break;
            }
            Ok(Some(ShellEvent::Stdout(b) | ShellEvent::Stderr(b))) => text.extend_from_slice(&b),
            Ok(Some(_)) => {}
            Err(_) => break,
        }
    }
    assert!(
        ended,
        "session did not end; output {:?}",
        String::from_utf8_lossy(&text)
    );
}

#[tokio::test]
async fn missing_port_is_reported() {
    let err = serial::open("/dev/hostwick-no-such-port", &SerialSettings::default())
        .await
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        err.starts_with("serial port /dev/hostwick-no-such-port not found"),
        "{err}"
    );
}
