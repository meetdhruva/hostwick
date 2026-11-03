//! Shared plumbing for in-terminal file transfers (ZMODEM, trzsz): the
//! byte link to the remote program, progress reports and errors.
//!
//! While a transfer runs, the terminal view hands every byte the remote
//! program prints to the transfer (instead of the screen) and the transfer
//! writes its answers through the session's bounded input queue, so a slow
//! network slows the file reader instead of filling memory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use hostwick_ssh::ShellCommand;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The remote program's output (in) and its input (out).
pub struct Link {
    pub rx: mpsc::UnboundedReceiver<Bytes>,
    pub tx: mpsc::Sender<ShellCommand>,
    /// Bytes read from `rx` that the protocol has not consumed yet.
    pub(crate) spare: Vec<u8>,
}

impl Link {
    pub fn new(rx: mpsc::UnboundedReceiver<Bytes>, tx: mpsc::Sender<ShellCommand>) -> Self {
        Self {
            rx,
            tx,
            spare: Vec::new(),
        }
    }

    pub async fn send(&self, data: Vec<u8>) -> Result<(), XferError> {
        if data.is_empty() {
            return Ok(());
        }
        self.tx
            .send(ShellCommand::Data(Bytes::from(data)))
            .await
            .map_err(|_| XferError::Closed)
    }

    /// Everything received but not consumed (to show in the terminal after
    /// the transfer), including what is still queued.
    pub fn leftover(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.spare);
        while let Ok(b) = self.rx.try_recv() {
            out.extend_from_slice(&b);
        }
        out
    }
}

/// How far a transfer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// The file being transferred.
    pub name: String,
    pub done: u64,
    pub total: u64,
    /// 1-based index of the file, and how many there are (0 = unknown).
    pub index: usize,
    pub count: usize,
}

/// A transfer ended without finishing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XferError {
    #[error("cancelled")]
    Cancelled,
    #[error("the other side cancelled")]
    RemoteCancelled,
    #[error("the connection closed")]
    Closed,
    #[error("no answer from the other side")]
    Timeout,
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Io(String),
    /// The remote program reported a failure (trzsz).
    #[error("{0}")]
    Remote(String),
}

impl From<std::io::Error> for XferError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// What a finished transfer produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Files written (download) or sent (upload).
    pub files: Vec<PathBuf>,
    pub bytes: u64,
}

pub type ProgressFn<'a> = &'a mut (dyn FnMut(&Progress) + Send);

/// A file to send.
#[derive(Debug, Clone)]
pub struct SendFile {
    pub path: PathBuf,
    /// Name the other side sees (relative, `/`-separated for folders).
    pub name: String,
}

impl SendFile {
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(Self {
            path: path.to_owned(),
            name,
        })
    }
}

/// Wait for the next chunk from the remote side, or the end of the
/// transfer: cancel, timeout or closed link.
pub(crate) async fn next_chunk(
    link: &mut Link,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<Bytes, XferError> {
    if cancel.is_cancelled() {
        return Err(XferError::Cancelled);
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(XferError::Cancelled),
        r = tokio::time::timeout(timeout, link.rx.recv()) => match r {
            Ok(Some(b)) => Ok(b),
            Ok(None) => Err(XferError::Closed),
            Err(_) => Err(XferError::Timeout),
        },
    }
}

/// A file name that is safe to create in a download folder: the last path
/// component only, no control characters, never empty, `.` or `..`.
pub fn safe_file_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_end_matches(['.', ' '])
        .to_owned();
    // Windows drive prefixes and reserved characters.
    let cleaned: String = cleaned
        .chars()
        .map(|c| if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `dir/name`, or `dir/name (1).ext`, `(2)`… when that exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_file_name("a.txt").as_deref(), Some("a.txt"));
        assert_eq!(safe_file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_file_name("C:\\Windows\\x.exe").as_deref(), Some("x.exe"));
        assert_eq!(safe_file_name("..").as_deref(), None);
        assert_eq!(safe_file_name("").as_deref(), None);
        assert_eq!(safe_file_name("dir/").as_deref(), None);
        assert_eq!(safe_file_name("a\u{7}b").as_deref(), Some("ab"));
        assert_eq!(safe_file_name("a:b").as_deref(), Some("a_b"));
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let d = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f.txt"));
        std::fs::write(d.path().join("f.txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f (1).txt"));
        std::fs::write(d.path().join("f (1).txt"), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(unique_path(d.path(), "f.txt"), d.path().join("f (2).txt"));
        assert_eq!(unique_path(d.path(), ".bashrc"), d.path().join(".bashrc"));
    }
}

// ---- spotting the start of a transfer in terminal output ---------------

/// What started: a ZMODEM peer (`sz`/`rz`) or a trzsz server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    Zmodem(crate::zmodem::Start),
    Trzsz(crate::trzsz::Magic),
}

/// The result of looking at a chunk of output.
#[derive(Debug, PartialEq, Eq)]
pub enum Scan {
    /// Nothing special: show the chunk as it is.
    Pass,
    /// Show `pass`; if `start` is set a transfer began and `rest` is what
    /// belongs to it (ZMODEM: from the first header byte; trzsz: after the
    /// magic line).
    Split {
        pass: Vec<u8>,
        start: Option<(Detected, Vec<u8>)>,
    },
}

/// Finds transfer starts in output, even when split over several chunks.
/// A few bytes that might be the beginning of a start sequence are held
/// back until the next chunk shows what they are.
#[derive(Debug, Default)]
pub struct Detector {
    held: Vec<u8>,
}

impl Detector {
    pub fn scan(&mut self, chunk: &[u8]) -> Scan {
        use crate::{trzsz, zmodem};
        let joined;
        let buf: &[u8] = if self.held.is_empty() {
            chunk
        } else {
            joined = [self.held.as_slice(), chunk].concat();
            self.held.clear();
            &joined
        };
        let z = zmodem::find_start(buf);
        let t = trzsz::find_magic(buf);
        // Whichever comes first.
        match (z, t) {
            (Some((zi, kind)), t) if t.as_ref().is_none_or(|(ti, _, _)| zi < *ti) => {
                return Scan::Split {
                    pass: buf[..zi].to_vec(),
                    start: Some((Detected::Zmodem(kind), buf[zi..].to_vec())),
                };
            }
            (_, Some((ti, end, magic))) => {
                return Scan::Split {
                    pass: buf[..ti].to_vec(),
                    start: Some((Detected::Trzsz(magic), buf[end..].to_vec())),
                };
            }
            _ => {}
        }
        let hold = zmodem::partial_start_len(buf).max(trzsz::partial_magic_len(buf));
        if hold == 0 {
            return if std::ptr::eq(buf.as_ptr(), chunk.as_ptr()) {
                Scan::Pass
            } else {
                Scan::Split {
                    pass: buf.to_vec(),
                    start: None,
                }
            };
        }
        let cut = buf.len() - hold;
        self.held = buf[cut..].to_vec();
        Scan::Split {
            pass: buf[..cut].to_vec(),
            start: None,
        }
    }

    /// Bytes are being held back.
    pub fn is_holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// Output ended, paused or a transfer is not wanted: give back what was
    /// held.
    pub fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod detector_tests {
    use super::*;
    use crate::zmodem::{Start, ZRINIT, ZRQINIT, hex_header};

    #[test]
    fn plain_output_passes_untouched() {
        let mut d = Detector::default();
        assert_eq!(d.scan(b"hello\r\n$ "), Scan::Pass);
        assert_eq!(d.scan(b"a * b ** c"), Scan::Pass);
    }

    #[test]
    fn zmodem_start_splits_the_chunk() {
        let mut d = Detector::default();
        let mut wire = b"rz\r".to_vec();
        wire.extend(hex_header(ZRQINIT, [0; 4]));
        match d.scan(&wire) {
            Scan::Split { pass, start: Some((Detected::Zmodem(Start::Download), rest)) } => {
                assert_eq!(pass, b"rz\r");
                assert!(rest.starts_with(b"**\x18B00"));
            }
            other => panic!("{other:?}"),
        }
        let up = hex_header(ZRINIT, [0, 0, 0, 0x23]);
        assert!(matches!(
            d.scan(&up),
            Scan::Split { start: Some((Detected::Zmodem(Start::Upload), _)), .. }
        ));
    }

    #[test]
    fn a_start_split_over_chunks_is_still_found() {
        let wire = [b"text".to_vec(), hex_header(ZRQINIT, [0; 4])].concat();
        for cut in 1..12 {
            let mut d = Detector::default();
            let mut shown = Vec::new();
            let mut found = None;
            for part in [&wire[..cut], &wire[cut..]] {
                if found.is_some() {
                    break;
                }
                match d.scan(part) {
                    Scan::Pass => shown.extend_from_slice(part),
                    Scan::Split { pass, start } => {
                        shown.extend(pass);
                        if start.is_some() {
                            found = start;
                        }
                    }
                }
            }
            assert_eq!(shown, b"text", "cut {cut}");
            assert!(found.is_some(), "cut {cut}");
        }
    }

    #[test]
    fn trzsz_magic_line_is_found_and_hidden() {
        let mut d = Detector::default();
        let wire = b"\x1b7::TRZSZ:TRANSFER:R:1.1.7:1234567890123:9\r\nmore";
        match d.scan(wire) {
            Scan::Split { pass, start: Some((Detected::Trzsz(m), rest)) } => {
                assert_eq!(pass, b"\x1b7");
                assert_eq!(rest, b"more");
                assert_eq!(m.mode, crate::trzsz::Mode::Upload);
            }
            other => panic!("{other:?}"),
        }
        // Split mid-line.
        let mut d = Detector::default();
        assert!(matches!(
            d.scan(b"x::TRZSZ:TRANSFER:S:1.1"),
            Scan::Split { start: None, .. }
        ));
        assert!(matches!(
            d.scan(b".7:123\r\n"),
            Scan::Split { start: Some((Detected::Trzsz(_), _)), .. }
        ));
    }

    #[test]
    fn held_bytes_are_released_when_they_turn_out_harmless() {
        let mut d = Detector::default();
        let first = d.scan(b"ab**");
        assert!(matches!(&first, Scan::Split { pass, start: None } if pass == b"ab"));
        match d.scan(b" cd") {
            Scan::Split { pass, start: None } => assert_eq!(pass, b"** cd"),
            other => panic!("{other:?}"),
        }
        assert!(d.flush().is_empty());
    }
}

/// Helpers for tests: in-memory links and a program on the other end of a
/// pipe.
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// Two links wired to each other.
    pub fn loopback() -> (Link, Link) {
        let (a_out, mut a_out_rx) = mpsc::channel::<ShellCommand>(256);
        let (b_out, mut b_out_rx) = mpsc::channel::<ShellCommand>(256);
        let (to_a, a_in) = mpsc::unbounded_channel::<Bytes>();
        let (to_b, b_in) = mpsc::unbounded_channel::<Bytes>();
        tokio::spawn(async move {
            while let Some(c) = a_out_rx.recv().await {
                if let ShellCommand::Data(d) = c {
                    let _ = to_b.send(d);
                }
            }
        });
        tokio::spawn(async move {
            while let Some(c) = b_out_rx.recv().await {
                if let ShellCommand::Data(d) = c {
                    let _ = to_a.send(d);
                }
            }
        });
        (Link::new(a_in, a_out), Link::new(b_in, b_out))
    }

    /// Run `args` (stdin/stdout piped, no terminal) in `cwd` and link to it.
    pub async fn spawn_link(args: &[&str], cwd: &Path) -> (Link, tokio::process::Child) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let mut child = tokio::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("{}: {e}", args[0]));
        let mut stdin = child.stdin.take().unwrap_or_else(|| panic!("stdin"));
        let mut stdout = child.stdout.take().unwrap_or_else(|| panic!("stdout"));
        let (out, mut out_rx) = mpsc::channel::<ShellCommand>(256);
        let (to_link, from_child) = mpsc::unbounded_channel::<Bytes>();
        tokio::spawn(async move {
            while let Some(c) = out_rx.recv().await {
                if let ShellCommand::Data(d) = c
                    && stdin.write_all(&d).await.is_err()
                {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if to_link.send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        (Link::new(from_child, out), child)
    }
}
