//! SCP adapter for servers without SFTP (routers, ESXi, busybox systems,
//! locked-down appliances).
//!
//! Listings come from `ls -la` (parsed by [`crate::ls`]), file operations
//! from shell commands with every path quoted, and contents travel with the
//! classic `scp -f` / `scp -t` protocol. SCP moves whole files only: reading
//! or writing at an offset is [`FsError::Unsupported`], and the transfer
//! engine restarts such copies from the beginning instead of resuming.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use hostwick_ssh::{Connection, ExitStatus};
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, DuplexStream,
    ReadBuf, ReadHalf, WriteHalf,
};
use tokio::sync::{OnceCell, watch};

use crate::ls::{LsFlavor, parse_ls_line, parse_ls_output};
use crate::remote_cmd::{self, command_error};
use crate::shell::{path_arg, quote};
use crate::{BoxRead, BoxWrite, DiskUsage, Entry, FsError, HashAlgo, RemoteFs, join, sort_entries};

type BoxFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

/// How long to wait for `scp` to acknowledge a protocol step.
const ACK_TIMEOUT: Duration = Duration::from_secs(60);

/// [`RemoteFs`] over `ls`, shell commands and the SCP protocol.
#[derive(Clone)]
pub struct ScpFs {
    inner: Arc<Inner>,
}

struct Inner {
    conn: Connection,
    /// `ls` options this server understands, found on first listing.
    ls: OnceCell<(LsFlavor, &'static str)>,
    /// Server UTC offset (seconds), for `ls` dates without a time zone.
    tz: OnceCell<i64>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn base_name(path: &str) -> String {
    let t = path.trim_end_matches('/');
    t.rsplit('/').next().unwrap_or(t).to_owned()
}

impl ScpFs {
    /// Check that the server has a shell with `scp` and `ls`.
    pub async fn open(conn: &Connection) -> Result<Self, FsError> {
        let out = conn
            .exec("command -v scp >/dev/null 2>&1 || which scp >/dev/null 2>&1 || type scp >/dev/null 2>&1")
            .await?;
        if out.exit_status != Some(0) {
            return Err(FsError::Unsupported(
                "the server offers neither SFTP nor scp".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                conn: conn.clone(),
                ls: OnceCell::new(),
                tz: OnceCell::new(),
            }),
        })
    }

    fn conn(&self) -> &Connection {
        &self.inner.conn
    }

    async fn ls_style(&self) -> (LsFlavor, &'static str) {
        *self
            .inner
            .ls
            .get_or_init(|| async {
                // Most precise first; keep the first whose output parses with
                // the expected date shape.
                for (flavor, opts, tokens) in [
                    (LsFlavor::Epoch, " --time-style=+%s", 1),
                    (LsFlavor::Full, "e", 5),
                    (LsFlavor::Full, "T", 4),
                ] {
                    let cmd = format!("LC_ALL=C ls -lad{opts} /");
                    if let Ok(out) = self.conn().exec(&cmd).await
                        && out.exit_status == Some(0)
                        && let Some(l) = out
                            .stdout_lossy()
                            .lines()
                            .find_map(|l| parse_ls_line(l, 0, now()))
                        && l.date_tokens == tokens
                    {
                        return (flavor, opts);
                    }
                }
                (LsFlavor::Plain, "")
            })
            .await
    }

    async fn tz_offset(&self, flavor: LsFlavor) -> i64 {
        if flavor == LsFlavor::Epoch {
            return 0;
        }
        *self
            .inner
            .tz
            .get_or_init(|| async {
                let Ok(out) = self.conn().exec("date +%z").await else {
                    return 0;
                };
                let s = out.stdout_lossy();
                let s = s.trim();
                let b = s.as_bytes();
                if b.len() == 5 && (b[0] == b'+' || b[0] == b'-') {
                    let h: i64 = s[1..3].parse().unwrap_or(0);
                    let m: i64 = s[3..5].parse().unwrap_or(0);
                    let off = h * 3600 + m * 60;
                    if b[0] == b'-' { -off } else { off }
                } else {
                    0
                }
            })
            .await
    }

    /// `ls` with the server's best options. `extra`: more flags (`d`, `L`, `n`).
    async fn ls(&self, path: &str, extra: &str) -> Result<(String, LsFlavor, i64), FsError> {
        let (flavor, opts) = self.ls_style().await;
        let tz = self.tz_offset(flavor).await;
        let (letters, long) = match opts.strip_prefix(' ') {
            Some(long) => ("", format!(" {long}")),
            None => (opts, String::new()),
        };
        let cmd = format!("LC_ALL=C ls -la{extra}{letters}{long} {}", path_arg(path));
        let out = self.conn().exec(&cmd).await?;
        if out.exit_status != Some(0) {
            return Err(command_error(path, &String::from_utf8_lossy(&out.stderr)));
        }
        Ok((out.stdout_lossy(), flavor, tz))
    }

    /// Start an `scp -f` / `scp -t` exchange.
    async fn start(&self, mode: &str, path: &str) -> Result<ScpSession, FsError> {
        let cmd = format!("scp {mode} {}", path_arg(path));
        let stream = self.conn().open_exec_stream(&cmd).await?;
        let (stdio, stderr, exit) = stream.into_parts();
        let (rd, wr) = tokio::io::split(stdio);
        Ok(ScpSession {
            rd: BufReader::new(rd),
            wr,
            stderr,
            exit,
            path: path.to_owned(),
        })
    }
}

/// One running `scp` process.
struct ScpSession {
    rd: BufReader<ReadHalf<DuplexStream>>,
    wr: WriteHalf<DuplexStream>,
    stderr: tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>,
    exit: watch::Receiver<Option<ExitStatus>>,
    path: String,
}

impl ScpSession {
    /// The error to report when the protocol broke off: what scp or the
    /// shell printed, else the exit status.
    async fn failure(&mut self, context: &str) -> FsError {
        let status = tokio::time::timeout(
            Duration::from_secs(3),
            hostwick_ssh::wait_exit(&mut self.exit),
        )
        .await
        .unwrap_or(ExitStatus::Closed);
        let mut text = String::new();
        while let Ok(b) = self.stderr.try_recv() {
            text.push_str(&String::from_utf8_lossy(&b));
        }
        if matches!(status, ExitStatus::Code(127)) {
            return FsError::Unsupported("scp is not available on the server".into());
        }
        if text.trim().is_empty() {
            return FsError::Remote {
                path: self.path.clone(),
                message: format!("scp {context}: {status}"),
            };
        }
        command_error(&self.path, &text)
    }

    async fn ack(&mut self, context: &str) -> Result<(), FsError> {
        match tokio::time::timeout(ACK_TIMEOUT, read_ack(&mut self.rd)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(AckError::Remote(msg))) => Err(command_error(&self.path, &msg)),
            Ok(Err(AckError::Io(_))) => Err(self.failure(context).await),
            Err(_) => Err(FsError::Timeout(format!("{}: scp {context}", self.path))),
        }
    }
}

enum AckError {
    Remote(String),
    Io(io::Error),
}

/// `\0` = OK; `\1msg\n` warning, `\2msg\n` fatal (both errors for us).
async fn read_ack<R: AsyncBufReadExt + Unpin>(rd: &mut R) -> Result<(), AckError> {
    let b = rd.read_u8().await.map_err(AckError::Io)?;
    match b {
        0 => Ok(()),
        1 | 2 => {
            let mut msg = String::new();
            rd.read_line(&mut msg).await.map_err(AckError::Io)?;
            Err(AckError::Remote(msg.trim().to_owned()))
        }
        other => {
            let mut msg = String::from(char::from(other));
            let _ = rd.read_line(&mut msg).await;
            Err(AckError::Remote(format!(
                "unexpected reply: {}",
                msg.trim()
            )))
        }
    }
}

/// `C0644 1234 name` → size.
pub(crate) fn parse_header(line: &str) -> Option<u64> {
    let rest = line.strip_prefix('C')?;
    let mut parts = rest.splitn(3, ' ');
    let mode = parts.next()?;
    if mode.len() != 4 || !mode.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return None;
    }
    let size = parts.next()?.parse().ok()?;
    parts.next()?;
    Some(size)
}

/// Header line for `scp -t`. A newline in the name would break the
/// protocol; the name is not used for the target path anyway.
pub(crate) fn header(size: u64, name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c == '\n' { '_' } else { c })
        .collect();
    format!("C0644 {size} {name}\n")
}

fn unsupported_offset(path: &str) -> FsError {
    FsError::Unsupported(format!("{path}: SCP cannot resume transfers"))
}

#[async_trait]
impl RemoteFs for ScpFs {
    async fn list_dir(&self, path: &str) -> Result<Vec<Entry>, FsError> {
        // A trailing slash lists the target of a symlinked directory.
        let target = if path.ends_with('/') {
            path.to_owned()
        } else {
            format!("{path}/")
        };
        let (out, _, tz) = self.ls(&target, "").await?;
        let mut entries = parse_ls_output(&out, path, tz, now());
        sort_entries(&mut entries);
        Ok(entries)
    }

    async fn stat(&self, path: &str) -> Result<Option<Entry>, FsError> {
        match self.ls(path, "dLn").await {
            Ok((out, _, tz)) => {
                Ok(out
                    .lines()
                    .find_map(|l| parse_ls_line(l, tz, now()))
                    .map(|l| {
                        let mut e = l.into_entry("");
                        e.name = base_name(path);
                        e.path = path.to_owned();
                        e
                    }))
            }
            Err(e) if e.is_not_found() => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        remote_cmd::run(self.conn(), path, &format!("mkdir {}", path_arg(path))).await
    }

    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        remote_cmd::run(self.conn(), path, &format!("rm {}", path_arg(path))).await
    }

    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        remote_cmd::run(self.conn(), path, &format!("rmdir {}", path_arg(path))).await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        // Like SFTP: never replace an existing target (and never move into
        // a directory that happens to have the target's name).
        let (f, t) = (path_arg(from), path_arg(to));
        let cmd = format!(
            "if [ -e {t} ] || [ -L {t} ]; then echo \"{}: File exists\" >&2; exit 1; fi; mv {f} {t}",
            to.replace(['"', '\\', '$', '`'], "_")
        );
        remote_cmd::run(self.conn(), from, &cmd).await
    }

    async fn open_read(&self, path: &str, offset: u64) -> Result<BoxRead, FsError> {
        if offset > 0 {
            return Err(unsupported_offset(path));
        }
        let mut s = self.start("-f", path).await?;
        s.wr.write_all(b"\0").await?;
        let mut line = String::new();
        let size = loop {
            line.clear();
            let n = tokio::time::timeout(ACK_TIMEOUT, s.rd.read_line(&mut line))
                .await
                .map_err(|_| FsError::Timeout(format!("{path}: scp")))?;
            match n {
                Ok(0) | Err(_) => return Err(s.failure("download").await),
                Ok(_) => {}
            }
            match line.as_bytes().first() {
                Some(1) | Some(2) => return Err(command_error(path, line[1..].trim())),
                // Timestamps (only sent with -p): acknowledge and go on.
                Some(b'T') => s.wr.write_all(b"\0").await?,
                Some(b'D') => {
                    return Err(FsError::Remote {
                        path: path.to_owned(),
                        message: "is a directory".into(),
                    });
                }
                _ => match parse_header(line.trim_end_matches('\n')) {
                    Some(size) => break size,
                    None => {
                        return Err(FsError::Remote {
                            path: path.to_owned(),
                            message: format!("unexpected scp reply: {}", line.trim()),
                        });
                    }
                },
            }
        };
        s.wr.write_all(b"\0").await?;
        s.wr.flush().await?;
        Ok(Box::pin(ScpReader {
            session: Some(s),
            remaining: size,
            finish: None,
            tmp: vec![0u8; 64 * 1024],
        }))
    }

    async fn open_write(&self, path: &str, offset: u64) -> Result<BoxWrite, FsError> {
        if offset > 0 {
            return Err(unsupported_offset(path));
        }
        // Size unknown: collect in a local temporary file first.
        let spool =
            std::env::temp_dir().join(format!("hostwick-scp-{}-{:016x}", std::process::id(), {
                use rand::Rng as _;
                rand::thread_rng().r#gen::<u64>()
            }));
        let file = tokio::fs::File::create(&spool).await?;
        Ok(Box::pin(SpoolWriter {
            file: Some(file),
            spool,
            fs: self.clone(),
            dest: path.to_owned(),
            finish: None,
        }))
    }

    async fn open_write_sized(
        &self,
        path: &str,
        offset: u64,
        size: u64,
    ) -> Result<BoxWrite, FsError> {
        if offset > 0 {
            return Err(unsupported_offset(path));
        }
        let mut s = self.start("-t", path).await?;
        s.ack("upload").await?;
        s.wr.write_all(header(size, &base_name(path)).as_bytes())
            .await?;
        s.wr.flush().await?;
        s.ack("upload").await?;
        Ok(Box::pin(ScpWriter {
            session: Some(s),
            remaining: size,
            finish: None,
        }))
    }

    async fn set_modified(&self, path: &str, unix_secs: u64) -> Result<(), FsError> {
        let p = path_arg(path);
        let cmd = format!(
            "TZ=UTC0 touch -m -t {} {p} 2>/dev/null || touch -m -d @{unix_secs} {p}",
            crate::ls::touch_stamp(unix_secs)
        );
        remote_cmd::run(self.conn(), path, &cmd).await
    }

    async fn canonicalize(&self, path: &str) -> Result<String, FsError> {
        let p = path_arg(if path.is_empty() { "." } else { path });
        let cmd = format!(
            "p={p}; if [ -d \"$p\" ]; then cd \"$p\" && pwd -P; \
             else cd \"$(dirname \"$p\")\" && pwd -P && basename \"$p\"; fi"
        );
        let out = self.conn().exec(&cmd).await?;
        if out.exit_status != Some(0) {
            return Err(command_error(path, &String::from_utf8_lossy(&out.stderr)));
        }
        let text = out.stdout_lossy();
        let mut lines = text.lines();
        let dir = lines.next().unwrap_or("/").to_owned();
        Ok(match lines.next() {
            Some(base) => join(&dir, base),
            None => dir,
        })
    }

    fn protocol(&self) -> String {
        "SCP".into()
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> Result<(), FsError> {
        let cmd = format!("chmod {:o} {}", mode & 0o7777, path_arg(path));
        remote_cmd::run(self.conn(), path, &cmd).await
    }

    async fn set_owner(
        &self,
        path: &str,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<(), FsError> {
        let p = path_arg(path);
        let cmd = match (uid, gid) {
            (Some(u), Some(g)) => format!("chown {u}:{g} {p}"),
            (Some(u), None) => format!("chown {u} {p}"),
            (None, Some(g)) => format!("chgrp {g} {p}"),
            (None, None) => return Ok(()),
        };
        remote_cmd::run(self.conn(), path, &cmd).await
    }

    async fn symlink(&self, target: &str, link: &str) -> Result<(), FsError> {
        let cmd = format!("ln -s -- {} {}", quote(target), path_arg(link));
        remote_cmd::run(self.conn(), link, &cmd).await
    }

    async fn read_link(&self, path: &str) -> Result<String, FsError> {
        let out = self
            .conn()
            .exec(&format!("readlink {}", path_arg(path)))
            .await?;
        if out.exit_status != Some(0) {
            return Err(command_error(path, &String::from_utf8_lossy(&out.stderr)));
        }
        Ok(out.stdout_lossy().trim_end_matches('\n').to_owned())
    }

    async fn lookup_ids(
        &self,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> (Option<String>, Option<String>) {
        remote_cmd::lookup_ids(self.conn(), uid, gid).await
    }

    async fn checksum(&self, path: &str, algo: HashAlgo) -> Result<String, FsError> {
        if let Some(d) = remote_cmd::checksum(self.conn(), path, algo).await {
            return Ok(d);
        }
        crate::stream_checksum(self, path, algo).await
    }

    async fn disk_usage(&self, path: &str) -> Result<DiskUsage, FsError> {
        if let Some(u) = remote_cmd::disk_usage(self.conn(), path).await {
            return Ok(u);
        }
        crate::walk_disk_usage(self, path).await
    }
}

/// File contents from `scp -f`: exactly the announced size, then the
/// closing handshake before end-of-file is reported.
struct ScpReader {
    session: Option<ScpSession>,
    remaining: u64,
    finish: Option<BoxFuture>,
    tmp: Vec<u8>,
}

impl AsyncRead for ScpReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.remaining > 0 {
            let Some(s) = this.session.as_mut() else {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            };
            let max = usize::try_from(this.remaining)
                .unwrap_or(usize::MAX)
                .min(buf.remaining())
                .min(this.tmp.len());
            let mut tmp = ReadBuf::new(&mut this.tmp[..max]);
            ready!(Pin::new(&mut s.rd).poll_read(cx, &mut tmp))?;
            let n = tmp.filled().len();
            if n == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "scp: connection closed before the whole file arrived",
                )));
            }
            buf.put_slice(tmp.filled());
            this.remaining -= n as u64;
            return Poll::Ready(Ok(()));
        }
        if this.finish.is_none() {
            let Some(mut s) = this.session.take() else {
                return Poll::Ready(Ok(()));
            };
            this.finish = Some(Box::pin(async move {
                // The file is followed by its status byte; confirm it.
                match read_ack(&mut s.rd).await {
                    Ok(()) => {}
                    Err(AckError::Remote(m)) => return Err(io::Error::other(m)),
                    Err(AckError::Io(e)) => return Err(e),
                }
                s.wr.write_all(b"\0").await?;
                s.wr.shutdown().await?;
                Ok(())
            }));
        }
        if let Some(f) = this.finish.as_mut() {
            let r = ready!(f.as_mut().poll(cx));
            this.finish = Some(Box::pin(async { Ok(()) }));
            r?;
        }
        Poll::Ready(Ok(()))
    }
}

/// File contents into `scp -t`: the size was announced, so writing more or
/// less than that is an error; `shutdown` completes the handshake and
/// waits for `scp` to confirm and exit.
struct ScpWriter {
    session: Option<ScpSession>,
    remaining: u64,
    finish: Option<BoxFuture>,
}

impl AsyncWrite for ScpWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        if buf.len() as u64 > this.remaining {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the file grew while it was being uploaded",
            )));
        }
        let Some(s) = this.session.as_mut() else {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        };
        let n = ready!(Pin::new(&mut s.wr).poll_write(cx, buf))?;
        this.remaining -= n as u64;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.session.as_mut() {
            Some(s) => Pin::new(&mut s.wr).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.finish.is_none() {
            let remaining = this.remaining;
            let Some(mut s) = this.session.take() else {
                return Poll::Ready(Ok(()));
            };
            this.finish = Some(Box::pin(async move {
                if remaining > 0 {
                    // Closing early makes scp discard nothing useful; report it.
                    let _ = s.wr.shutdown().await;
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!(
                            "the file shrank while it was being uploaded ({remaining} bytes missing)"
                        ),
                    ));
                }
                s.wr.write_all(b"\0").await?;
                s.wr.flush().await?;
                if let Err(e) = s.ack("upload").await {
                    return Err(io::Error::other(e.to_string()));
                }
                s.wr.shutdown().await?;
                let status =
                    tokio::time::timeout(ACK_TIMEOUT, hostwick_ssh::wait_exit(&mut s.exit))
                        .await
                        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "scp did not exit"))?;
                if !status.success() && status != ExitStatus::Closed {
                    return Err(io::Error::other(s.failure("upload").await.to_string()));
                }
                Ok(())
            }));
        }
        match this.finish.as_mut() {
            Some(f) => {
                let r = ready!(f.as_mut().poll(cx));
                this.finish = Some(Box::pin(async { Ok(()) }));
                Poll::Ready(r)
            }
            None => Poll::Ready(Ok(())),
        }
    }
}

/// Writer for when the size is not known up front: spools to a local
/// temporary file and uploads it on `shutdown`.
struct SpoolWriter {
    file: Option<tokio::fs::File>,
    spool: std::path::PathBuf,
    fs: ScpFs,
    dest: String,
    finish: Option<BoxFuture>,
}

impl Drop for SpoolWriter {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.spool);
    }
}

impl AsyncWrite for SpoolWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.file.as_mut() {
            Some(f) => Pin::new(f).poll_write(cx, buf),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.file.as_mut() {
            Some(f) => Pin::new(f).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.finish.is_none() {
            let Some(mut file) = this.file.take() else {
                return Poll::Ready(Ok(()));
            };
            let (fs, dest, spool) = (this.fs.clone(), this.dest.clone(), this.spool.clone());
            this.finish = Some(Box::pin(async move {
                file.flush().await?;
                drop(file);
                let size = tokio::fs::metadata(&spool).await?.len();
                let mut src = tokio::fs::File::open(&spool).await?;
                let mut w = fs
                    .open_write_sized(&dest, 0, size)
                    .await
                    .map_err(|e| io::Error::other(e.to_string()))?;
                tokio::io::copy(&mut src, &mut w).await?;
                w.shutdown().await
            }));
        }
        match this.finish.as_mut() {
            Some(f) => {
                let r = ready!(f.as_mut().poll(cx));
                this.finish = Some(Box::pin(async { Ok(()) }));
                Poll::Ready(r)
            }
            None => Poll::Ready(Ok(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers() {
        assert_eq!(parse_header("C0644 1234 my file.txt"), Some(1234));
        assert_eq!(parse_header("C0755 0 x"), Some(0));
        assert_eq!(parse_header("D0755 0 dir"), None);
        assert_eq!(parse_header("C0644 12"), None);
        assert_eq!(parse_header("C06x4 12 a"), None);
        assert_eq!(header(5, "a\nb"), "C0644 5 a_b\n");
    }

    #[tokio::test]
    async fn acks() {
        let mut ok: &[u8] = b"\0";
        assert!(read_ack(&mut ok).await.is_ok());
        let mut err: &[u8] = b"\x01scp: /x: No such file or directory\n";
        assert!(matches!(
            read_ack(&mut err).await,
            Err(AckError::Remote(m)) if m == "scp: /x: No such file or directory"
        ));
        let mut eof: &[u8] = b"";
        assert!(matches!(read_ack(&mut eof).await, Err(AckError::Io(_))));
    }
}
