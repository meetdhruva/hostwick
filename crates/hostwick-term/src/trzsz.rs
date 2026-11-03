//! trzsz (`trz` / `tsz`): a file transfer that rides on the terminal's own
//! text stream, works through tmux and needs no special channel.
//!
//! The server program prints `::TRZSZ:TRANSFER:<S|R|D>:<version>:<id>[:port]`
//! and then speaks line-based messages (`#TYPE:payload`). Strings and binary
//! values are zlib-compressed and base64-encoded; numbers are plain text;
//! file data is sent either that way (base64 mode, always works) or as raw
//! bytes with a few escaped values (binary mode, `-b`).
//!
//! Flow (this side is the "client", like iTerm2 or trzsz-ssh):
//! `#ACT` (we are here, and confirm) → `#CFG` (the server's options) →
//! `#NUM`, then per file `#NAME`, `#SIZE`, `#DATA`…, `#MD5`, each answered
//! with `#SUCC` → `#EXIT`. Directory transfers (`-d`) are declined.
//!
//! [`download`] is `tsz` (the server sends), [`upload`] is `trz` (the server
//! receives).

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use md5::{Digest as _, Md5};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::xfer::{
    Link, Outcome, Progress, ProgressFn, SendFile, XferError, next_chunk, safe_file_name,
    unique_path,
};

const MAGIC: &[u8] = b"::TRZSZ:TRANSFER:";
/// Wait this long for each message from the server.
const PATIENCE: Duration = Duration::from_secs(30);
/// Protocol version we speak.
const PROTOCOL: u32 = 1;

/// What the server's magic line announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magic {
    pub mode: Mode,
    pub version: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `tsz`: the server sends, this side downloads.
    Download,
    /// `trz`: the server receives, this side uploads.
    Upload,
    /// `trz -d`: the server receives files and folders.
    UploadDirs,
}

/// Find a complete magic line in `buf`: (start of the magic, end of the
/// line including its newline, parsed). `None` when absent or when the line
/// is not complete yet (see [`partial_magic_len`]).
pub fn find_magic(buf: &[u8]) -> Option<(usize, usize, Magic)> {
    let at = buf.windows(MAGIC.len()).position(|w| w == MAGIC)?;
    let rest = &buf[at + MAGIC.len()..];
    let nl = rest.iter().position(|&b| b == b'\n')?;
    let line = String::from_utf8_lossy(&rest[..nl]);
    let line = line.trim_end_matches('\r');
    let mut parts = line.split(':');
    let mode = match parts.next()? {
        "S" => Mode::Download,
        "R" => Mode::Upload,
        "D" => Mode::UploadDirs,
        _ => return None,
    };
    let version = parts.next()?.to_owned();
    let id = parts.next().unwrap_or_default().to_owned();
    Some((
        at,
        at + MAGIC.len() + nl + 1,
        Magic { mode, version, id },
    ))
}

/// Bytes at the end of `buf` that may begin a magic line (the prefix of
/// the marker, or a marker whose line has no newline yet).
pub fn partial_magic_len(buf: &[u8]) -> usize {
    if let Some(at) = buf.windows(MAGIC.len()).rposition(|w| w == MAGIC) {
        let tail = &buf[at..];
        if !tail.contains(&b'\n') && tail.len() < 200 {
            return tail.len();
        }
        return 0;
    }
    (2..MAGIC.len())
        .rev()
        .find(|&k| buf.len() >= k && buf.ends_with(&MAGIC[..k]))
        .unwrap_or(0)
}

// ---- message encoding -------------------------------------------------------

/// zlib + base64, as trzsz sends strings and binary values.
pub fn encode_buffer(data: &[u8]) -> String {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(data);
    let packed = z.finish().unwrap_or_default();
    B64.encode(packed)
}

pub fn decode_buffer(text: &str) -> Result<Vec<u8>, XferError> {
    let packed = B64
        .decode(text.trim())
        .map_err(|e| XferError::Protocol(format!("bad base64 from the server: {e}")))?;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(&packed[..])
        .read_to_end(&mut out)
        .map_err(|e| XferError::Protocol(format!("bad compressed data from the server: {e}")))?;
    Ok(out)
}

/// `#TYPE:payload\n`.
pub fn line(kind: &str, payload: &str) -> Vec<u8> {
    format!("#{kind}:{payload}\n").into_bytes()
}

/// The pairs of values replaced in binary mode: (original byte, escaped).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Escapes(Vec<(u8, [u8; 2])>);

impl Escapes {
    /// From the server's `escape_chars` (`[["î", "îî"], …]`).
    pub fn from_json(v: &serde_json::Value) -> Self {
        let latin1 = |s: &str| -> Vec<u8> { s.chars().map(|c| c as u32 as u8).collect() };
        let mut out = Vec::new();
        if let Some(list) = v.as_array() {
            for pair in list {
                let (Some(a), Some(b)) = (
                    pair.get(0).and_then(|x| x.as_str()),
                    pair.get(1).and_then(|x| x.as_str()),
                ) else {
                    continue;
                };
                let (a, b) = (latin1(a), latin1(b));
                if a.len() == 1 && b.len() == 2 {
                    out.push((a[0], [b[0], b[1]]));
                }
            }
        }
        Self(out)
    }

    pub fn escape(&self, data: &[u8]) -> Vec<u8> {
        if self.0.is_empty() {
            return data.to_vec();
        }
        let mut out = Vec::with_capacity(data.len() + data.len() / 16);
        for &b in data {
            match self.0.iter().find(|(o, _)| *o == b) {
                Some((_, e)) => out.extend_from_slice(e),
                None => out.push(b),
            }
        }
        out
    }

    pub fn unescape(&self, data: &[u8]) -> Vec<u8> {
        if self.0.is_empty() {
            return data.to_vec();
        }
        // Every escape starts with the same marker byte (0xee).
        let marker = self.0.iter().map(|(_, e)| e[0]).next();
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            if Some(data[i]) == marker
                && i + 1 < data.len()
                && let Some((o, _)) = self.0.iter().find(|(_, e)| e[1] == data[i + 1])
            {
                out.push(*o);
                i += 2;
                continue;
            }
            out.push(data[i]);
            i += 1;
        }
        out
    }
}

/// The server's options from `#CFG`.
#[derive(Debug, Clone, Default)]
struct Config {
    binary: bool,
    escapes: Escapes,
    bufsize: usize,
    directory: bool,
}

// ---- the line reader --------------------------------------------------------

struct Wire<'a> {
    link: &'a mut Link,
    buf: Vec<u8>,
    cancel: &'a CancellationToken,
}

impl<'a> Wire<'a> {
    fn new(link: &'a mut Link, cancel: &'a CancellationToken, initial: &[u8]) -> Self {
        Self {
            link,
            buf: initial.to_vec(),
            cancel,
        }
    }

    async fn fill(&mut self) -> Result<(), XferError> {
        let c = next_chunk(self.link, self.cancel, PATIENCE).await?;
        self.buf.extend_from_slice(&c);
        Ok(())
    }

    /// One line, without its newline (and carriage return).
    async fn read_line(&mut self) -> Result<String, XferError> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let mut l: Vec<u8> = self.buf.drain(..=i).collect();
                l.pop();
                while l.last() == Some(&b'\r') || l.last() == Some(&0) {
                    l.pop();
                }
                return Ok(String::from_utf8_lossy(&l).into_owned());
            }
            if self.buf.len() > 64 * 1024 * 1024 {
                return Err(XferError::Protocol("line too long".into()));
            }
            self.fill().await?;
        }
    }

    async fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, XferError> {
        while self.buf.len() < n {
            self.fill().await?;
        }
        Ok(self.buf.drain(..n).collect())
    }

    /// The payload of the next `#kind:` line. A `#FAIL`/`#fail` from the
    /// server ends the transfer with its message; `#EXIT` too.
    async fn expect(&mut self, kind: &str) -> Result<String, XferError> {
        let raw = self.read_line().await?;
        // Terminal output can put junk before the message.
        let l = match raw.rfind('#') {
            Some(i) => &raw[i..],
            None => &raw[..],
        };
        let Some(rest) = l.strip_prefix('#') else {
            return Err(XferError::Protocol(format!("unexpected text from the server: {raw:?}")));
        };
        let (k, payload) = rest.split_once(':').unwrap_or((rest, ""));
        if k == kind {
            return Ok(payload.to_owned());
        }
        if matches!(k, "FAIL" | "fail" | "EXIT") {
            let msg = decode_buffer(payload)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| payload.to_owned());
            return Err(XferError::Remote(msg));
        }
        Err(XferError::Protocol(format!(
            "expected #{kind} from the server, got #{k}"
        )))
    }

    async fn expect_int(&mut self, kind: &str) -> Result<u64, XferError> {
        let p = self.expect(kind).await?;
        p.trim()
            .parse()
            .map_err(|_| XferError::Protocol(format!("#{kind} is not a number: {p:?}")))
    }

    async fn expect_string(&mut self, kind: &str) -> Result<String, XferError> {
        let p = self.expect(kind).await?;
        Ok(String::from_utf8_lossy(&decode_buffer(&p)?).into_owned())
    }

    async fn expect_binary(&mut self, kind: &str) -> Result<Vec<u8>, XferError> {
        let p = self.expect(kind).await?;
        decode_buffer(&p)
    }

    async fn say(&self, kind: &str, payload: &str) -> Result<(), XferError> {
        if self.cancel.is_cancelled() {
            return Err(XferError::Cancelled);
        }
        self.link.send(line(kind, payload)).await
    }

    async fn say_string(&self, kind: &str, text: &str) -> Result<(), XferError> {
        self.say(kind, &encode_buffer(text.as_bytes())).await
    }

    /// Tell the server we are stopping, so it prints why and returns the
    /// shell prompt.
    async fn fail(&self, message: &str) {
        let _ = self
            .link
            .send(line("FAIL", &encode_buffer(message.as_bytes())))
            .await;
    }
}

fn action(confirm: bool, version: &str) -> String {
    serde_json::json!({
        "lang": "rs",
        "confirm": confirm,
        "version": version,
        "support_dir": false,
        "protocol": PROTOCOL,
    })
    .to_string()
}

/// Answer a transfer the person does not want ("Cancelled" on the server).
pub async fn decline(link: &mut Link, version: &str) -> Result<(), XferError> {
    link.send(line("ACT", &encode_buffer(action(false, version).as_bytes())))
        .await
}

async fn handshake(w: &mut Wire<'_>, version: &str) -> Result<Config, XferError> {
    // The server sets its terminal to raw mode right after printing the
    // magic line; give it a moment so our answer is not echoed.
    tokio::time::sleep(Duration::from_millis(150)).await;
    w.say("ACT", &encode_buffer(action(true, version).as_bytes()))
        .await?;
    let cfg = w.expect_string("CFG").await?;
    let v: serde_json::Value = serde_json::from_str(&cfg)
        .map_err(|e| XferError::Protocol(format!("bad #CFG from the server: {e}")))?;
    Ok(Config {
        binary: v.get("binary").and_then(|x| x.as_bool()).unwrap_or(false),
        escapes: v
            .get("escape_chars")
            .map(Escapes::from_json)
            .unwrap_or_default(),
        bufsize: v
            .get("bufsize")
            .and_then(|x| x.as_u64())
            .map_or(10 * 1024 * 1024, |n| n as usize),
        directory: v.get("directory").and_then(|x| x.as_bool()).unwrap_or(false),
    })
}

/// End the transfer: on an error, tell the server; keep leftover text for
/// the terminal.
async fn finish<T>(w: Wire<'_>, res: Result<T, XferError>) -> Result<T, XferError> {
    match &res {
        Err(XferError::Cancelled) => w.fail("Cancelled").await,
        Err(XferError::Timeout | XferError::Protocol(_) | XferError::Io(_)) => {
            let msg = res.as_ref().err().map(ToString::to_string).unwrap_or_default();
            w.fail(&msg).await;
        }
        _ => {}
    }
    w.link.spare = w.buf;
    res
}

/// `tsz`: the server sends files; save them into `dest`. `initial` is what
/// followed the magic line. `exit_message` is shown on the server when done.
pub async fn download(
    link: &mut Link,
    initial: &[u8],
    version: &str,
    dest: &Path,
    exit_message: &str,
    cancel: &CancellationToken,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let mut w = Wire::new(link, cancel, initial);
    let res = download_inner(&mut w, version, dest, exit_message, progress).await;
    finish(w, res).await
}

async fn download_inner(
    w: &mut Wire<'_>,
    version: &str,
    dest: &Path,
    exit_message: &str,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let cfg = handshake(w, version).await?;
    if cfg.directory {
        return Err(XferError::Protocol(
            "folders are not supported; send single files".into(),
        ));
    }
    let mut out = Outcome::default();
    let count = w.expect_int("NUM").await?;
    w.say("SUCC", &count.to_string()).await?;
    for index in 1..=count as usize {
        let name = w.expect_string("NAME").await?;
        let name = safe_file_name(&name).unwrap_or_else(|| format!("file-{index}"));
        let path = unique_path(dest, &name);
        let local = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.clone());
        let mut file = tokio::fs::File::create(&path).await?;
        w.say("SUCC", &encode_buffer(local.as_bytes())).await?;
        let size = w.expect_int("SIZE").await?;
        w.say("SUCC", &size.to_string()).await?;
        let mut md5 = Md5::new();
        let mut done = 0u64;
        let report = |progress: &mut (dyn FnMut(&Progress) + Send), done: u64| {
            progress(&Progress {
                name: local.clone(),
                done,
                total: size,
                index,
                count: count as usize,
            });
        };
        report(progress, 0);
        while done < size {
            let data = if cfg.binary {
                let n = w.expect_int("DATA").await? as usize;
                let raw = w.read_exact(n).await?;
                cfg.escapes.unescape(&raw)
            } else {
                w.expect_binary("DATA").await?
            };
            file.write_all(&data).await?;
            md5.update(&data);
            done += data.len() as u64;
            w.say("SUCC", &data.len().to_string()).await?;
            report(progress, done);
        }
        file.flush().await?;
        let digest = md5.finalize().to_vec();
        let theirs = w.expect_binary("MD5").await?;
        if theirs != digest {
            return Err(XferError::Protocol(format!("{local}: the MD5 check failed")));
        }
        w.say("SUCC", &encode_buffer(&digest)).await?;
        out.files.push(path);
        out.bytes += size;
    }
    w.say_string("EXIT", exit_message).await?;
    Ok(out)
}

/// `trz`: the server receives; send `files`. `initial` is what followed
/// the magic line.
pub async fn upload(
    link: &mut Link,
    initial: &[u8],
    version: &str,
    files: &[SendFile],
    exit_message: &str,
    cancel: &CancellationToken,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let mut w = Wire::new(link, cancel, initial);
    let res = upload_inner(&mut w, version, files, exit_message, progress).await;
    finish(w, res).await
}

async fn upload_inner(
    w: &mut Wire<'_>,
    version: &str,
    files: &[SendFile],
    exit_message: &str,
    progress: ProgressFn<'_>,
) -> Result<Outcome, XferError> {
    let cfg = handshake(w, version).await?;
    if cfg.directory {
        return Err(XferError::Protocol(
            "folders are not supported; send single files".into(),
        ));
    }
    let mut out = Outcome::default();
    w.say("NUM", &files.len().to_string()).await?;
    let acked = w.expect_int("SUCC").await?;
    if acked != files.len() as u64 {
        return Err(XferError::Protocol("the server counted a different number of files".into()));
    }
    let max_chunk = cfg.bufsize.clamp(1024, 2 * 1024 * 1024);
    let mut chunk = (64 * 1024usize).min(max_chunk);
    for (ix, f) in files.iter().enumerate() {
        let meta = tokio::fs::metadata(&f.path).await?;
        let size = meta.len();
        let mut file = tokio::fs::File::open(&f.path).await?;
        w.say_string("NAME", &f.name).await?;
        let _remote_name = w.expect_string("SUCC").await?;
        w.say("SIZE", &size.to_string()).await?;
        let echoed = w.expect_int("SUCC").await?;
        if echoed != size {
            return Err(XferError::Protocol("the server saw a different file size".into()));
        }
        let mut md5 = Md5::new();
        let mut done = 0u64;
        let report = |progress: &mut (dyn FnMut(&Progress) + Send), done: u64| {
            progress(&Progress {
                name: f.name.clone(),
                done,
                total: size,
                index: ix + 1,
                count: files.len(),
            });
        };
        report(progress, 0);
        let mut buf = vec![0u8; max_chunk];
        while done < size {
            let want = chunk.min((size - done) as usize);
            let mut n = 0;
            while n < want {
                let r = file.read(&mut buf[n..want]).await?;
                if r == 0 {
                    return Err(XferError::Io(format!("{} changed while sending", f.name)));
                }
                n += r;
            }
            let data = &buf[..n];
            md5.update(data);
            let started = Instant::now();
            if cfg.binary {
                let esc = cfg.escapes.escape(data);
                let mut msg = format!("#DATA:{}\n", esc.len()).into_bytes();
                msg.extend_from_slice(&esc);
                if w.cancel.is_cancelled() {
                    return Err(XferError::Cancelled);
                }
                w.link.send(msg).await?;
            } else {
                w.say("DATA", &encode_buffer(data)).await?;
            }
            let ok = w.expect_int("SUCC").await?;
            if ok != n as u64 {
                return Err(XferError::Protocol("the server acknowledged a different amount".into()));
            }
            done += n as u64;
            report(progress, done);
            // Larger chunks while the link keeps up, smaller when it lags.
            let took = started.elapsed();
            if took < Duration::from_millis(500) && chunk < max_chunk {
                chunk = (chunk * 2).min(max_chunk);
            } else if took > Duration::from_secs(2) && chunk > 4096 {
                chunk /= 2;
            }
        }
        let digest = md5.finalize().to_vec();
        w.say("MD5", &encode_buffer(&digest)).await?;
        let theirs = w.expect_binary("SUCC").await?;
        if theirs != digest {
            return Err(XferError::Protocol(format!("{}: the MD5 check failed", f.name)));
        }
        out.files.push(f.path.clone());
        out.bytes += size;
    }
    w.say_string("EXIT", exit_message).await?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_lines() {
        let buf = b"\x1b7::TRZSZ:TRANSFER:S:1.1.7:1234567890123:5000\r\nrest";
        let (at, end, m) = find_magic(buf).unwrap_or_else(|| panic!("no magic"));
        assert_eq!(at, 2);
        assert_eq!(&buf[end..], b"rest");
        assert_eq!(m.mode, Mode::Download);
        assert_eq!(m.version, "1.1.7");
        assert_eq!(m.id, "1234567890123");
        let (_, _, m) = find_magic(b"::TRZSZ:TRANSFER:R:1.0.0\n").unwrap_or_else(|| panic!("r"));
        assert_eq!(m.mode, Mode::Upload);
        let (_, _, m) =
            find_magic(b"::TRZSZ:TRANSFER:D:1.1.7:1234\n").unwrap_or_else(|| panic!("d"));
        assert_eq!(m.mode, Mode::UploadDirs);
        // Not complete yet, or not a magic at all.
        assert!(find_magic(b"::TRZSZ:TRANSFER:S:1.1.7").is_none());
        assert!(find_magic(b"::TRZSZ:TRANSFER:X:1.1.7\n").is_none());
        assert!(find_magic(b"plain").is_none());
    }

    #[test]
    fn partial_magics_are_held_back() {
        assert_eq!(partial_magic_len(b"abc"), 0);
        assert_eq!(partial_magic_len(b"abc::TR"), 4);
        assert_eq!(partial_magic_len(b"abc:"), 0);
        assert_eq!(partial_magic_len(b"x::TRZSZ:TRANSFER:S:1.1"), 22);
        assert_eq!(partial_magic_len(b"::TRZSZ:TRANSFER:S:1.1.7\n"), 0);
    }

    #[test]
    fn strings_round_trip_and_match_trzsz_encoding() {
        for s in ["", "hello", "héllo wörld ✓", &"x".repeat(100_000)] {
            let e = encode_buffer(s.as_bytes());
            assert_eq!(decode_buffer(&e).unwrap_or_default(), s.as_bytes());
        }
        // What the Python implementation sends for "ok":
        // base64(zlib.compress(b"ok")).
        assert_eq!(decode_buffer("eJzLzwYAAUsA2w==").unwrap_or_default(), b"ok");
        assert!(decode_buffer("!!!").is_err());
        assert!(decode_buffer("AAAA").is_err());
    }

    #[test]
    fn binary_escapes() {
        let v = serde_json::json!([["\u{ee}", "\u{ee}\u{ee}"], ["~", "\u{ee}1"], ["\u{2}", "\u{ee}A"]]);
        let e = Escapes::from_json(&v);
        let data: Vec<u8> = (0..=255u8).chain([0xee, 0x31, 0xee, 0xee, b'~']).collect();
        let esc = e.escape(&data);
        assert!(!esc.contains(&b'~'));
        assert!(!esc.contains(&2));
        assert_eq!(e.unescape(&esc), data);
        assert_eq!(Escapes::default().escape(b"abc"), b"abc");
    }

    // ---- against the real trzsz server (Python `trz` / `tsz`) --------------

    fn have(cmd: &str) -> bool {
        std::process::Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn sample(n: usize) -> Vec<u8> {
        let mut x = 0x2468_ace1u32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if i % 5 == 0 { (i % 256) as u8 } else { (x >> 24) as u8 }
            })
            .collect()
    }

    /// Run `args` on the far side of a pseudo terminal.
    async fn pty_link(args: &[&str], cwd: &Path) -> (Link, tokio::process::Child) {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/ptyrun.py");
        let script = script.to_string_lossy().into_owned();
        let mut full = vec!["python3", "-I", script.as_str()];
        full.extend_from_slice(args);
        crate::xfer::testutil::spawn_link(&full, cwd).await
    }

    /// Read until the server's magic line is complete.
    async fn wait_magic(link: &mut Link) -> (Magic, Vec<u8>) {
        let mut buf = Vec::new();
        loop {
            if let Some((_, end, m)) = find_magic(&buf) {
                return (m, buf[end..].to_vec());
            }
            let c = tokio::time::timeout(Duration::from_secs(10), link.rx.recv())
                .await
                .unwrap_or_else(|_| panic!("no magic line; got {buf:?}"))
                .unwrap_or_else(|| panic!("server closed; got {buf:?}"));
            buf.extend_from_slice(&c);
        }
    }

    #[tokio::test]
    async fn downloads_from_tsz_in_both_modes() {
        if !have("tsz") {
            eprintln!("trzsz (tsz) not installed; skipping");
            return;
        }
        let work = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let big = sample(700_000);
        std::fs::write(work.path().join("big.bin"), &big).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(work.path().join("small.txt"), b"hello trzsz\n").unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(work.path().join("empty"), b"").unwrap_or_else(|e| panic!("{e}"));
        for flags in [&[][..], &["-b"][..], &["-b", "-e"][..]] {
            let out = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
            let mut args = vec!["tsz"];
            args.extend_from_slice(flags);
            args.extend(["big.bin", "small.txt", "empty"]);
            let (mut link, mut child) = pty_link(&args, work.path()).await;
            let (magic, rest) = wait_magic(&mut link).await;
            assert_eq!(magic.mode, Mode::Download);
            let cancel = CancellationToken::new();
            let mut last = 0;
            let got = download(&mut link, &rest, "0.4.0", out.path(), "Received", &cancel, &mut |p| {
                last = p.done;
            })
            .await
            .unwrap_or_else(|e| panic!("download {flags:?}: {e}"));
            assert_eq!(got.files.len(), 3, "{flags:?}");
            assert_eq!(std::fs::read(out.path().join("big.bin")).unwrap_or_default(), big, "{flags:?}");
            assert_eq!(
                std::fs::read(out.path().join("small.txt")).unwrap_or_default(),
                b"hello trzsz\n"
            );
            assert_eq!(std::fs::read(out.path().join("empty")).unwrap_or_default(), b"");
            let _ = last;
            let _ = child.wait().await;
        }
    }

    #[tokio::test]
    async fn uploads_to_trz_in_both_modes() {
        if !have("trz") {
            eprintln!("trzsz (trz) not installed; skipping");
            return;
        }
        let src = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let big = sample(900_000);
        std::fs::write(src.path().join("up.bin"), &big).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.path().join("note.txt"), "grüße ✓\n").unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.path().join("zero"), b"").unwrap_or_else(|e| panic!("{e}"));
        let files: Vec<SendFile> = ["up.bin", "note.txt", "zero"]
            .iter()
            .filter_map(|n| SendFile::from_path(&src.path().join(n)))
            .collect();
        for flags in [&[][..], &["-b"][..], &["-b", "-e"][..]] {
            let dest = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
            let dest_s = dest.path().to_string_lossy().into_owned();
            let mut args = vec!["trz", "-y"];
            args.extend_from_slice(flags);
            args.push(dest_s.as_str());
            let (mut link, mut child) = pty_link(&args, dest.path()).await;
            let (magic, rest) = wait_magic(&mut link).await;
            assert_eq!(magic.mode, Mode::Upload);
            let cancel = CancellationToken::new();
            let sent = upload(&mut link, &rest, "0.4.0", &files, "Sent", &cancel, &mut |_| {})
                .await
                .unwrap_or_else(|e| panic!("upload {flags:?}: {e}"));
            assert_eq!(sent.files.len(), 3);
            let _ = child.wait().await;
            assert_eq!(std::fs::read(dest.path().join("up.bin")).unwrap_or_default(), big, "{flags:?}");
            assert_eq!(
                std::fs::read_to_string(dest.path().join("note.txt")).unwrap_or_default(),
                "grüße ✓\n"
            );
            assert_eq!(std::fs::read(dest.path().join("zero")).unwrap_or_default(), b"");
        }
    }

    #[tokio::test]
    async fn declining_and_cancelling_leave_the_server_clean() {
        if !have("trz") {
            eprintln!("trzsz (trz) not installed; skipping");
            return;
        }
        let dest = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let dest_s = dest.path().to_string_lossy().into_owned();
        // Decline: the server prints "Cancelled" and exits.
        let (mut link, mut child) = pty_link(&["trz", &dest_s], dest.path()).await;
        let (_, _rest) = wait_magic(&mut link).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        decline(&mut link, "0.4.0").await.unwrap_or_else(|e| panic!("{e}"));
        let mut seen = String::new();
        while let Ok(Some(c)) =
            tokio::time::timeout(Duration::from_secs(5), link.rx.recv()).await
        {
            seen.push_str(&String::from_utf8_lossy(&c));
        }
        assert!(seen.contains("Cancelled"), "{seen:?}");
        let st = child.wait().await.unwrap_or_else(|e| panic!("{e}"));
        assert!(st.success() || st.code().is_some());

        // Cancel half way through an upload.
        let src = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(src.path().join("big.bin"), sample(40_000_000)).unwrap_or_else(|e| panic!("{e}"));
        let files = [SendFile::from_path(&src.path().join("big.bin")).unwrap_or_else(|| panic!("n"))];
        let (mut link, mut child) = pty_link(&["trz", "-y", &dest_s], dest.path()).await;
        let (_, rest) = wait_magic(&mut link).await;
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(600)).await;
            c2.cancel();
        });
        let r = upload(&mut link, &rest, "0.4.0", &files, "Sent", &cancel, &mut |_| {}).await;
        let _ = stopper.await;
        assert_eq!(r.err(), Some(XferError::Cancelled));
        let mut seen = String::new();
        while let Ok(Some(c)) =
            tokio::time::timeout(Duration::from_secs(5), link.rx.recv()).await
        {
            seen.push_str(&String::from_utf8_lossy(&c));
        }
        assert!(seen.contains("Cancelled"), "server did not stop: {seen:?}");
        let _ = child.wait().await;
    }
}
