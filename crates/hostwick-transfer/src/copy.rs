use std::sync::Arc;
use std::time::Duration;

use hostwick_sftp::RemoteFs;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::TransferError;
use crate::rate::RateLimiter;
use tokio::sync::watch;

/// Pause switches of one job: its own and the engine-wide one. Either being
/// set pauses the copy.
#[derive(Clone)]
pub(crate) struct PauseGate {
    job: watch::Receiver<bool>,
    all: watch::Receiver<bool>,
}

impl PauseGate {
    pub(crate) fn new(job: watch::Receiver<bool>, all: watch::Receiver<bool>) -> Self {
        Self { job, all }
    }

    pub(crate) fn is_paused(&self) -> bool {
        *self.job.borrow() || *self.all.borrow()
    }

    /// Resolves when paused (at once if it already is).
    pub(crate) async fn paused(&mut self) {
        while !self.is_paused() {
            self.changed().await;
        }
    }

    /// Resolves when not paused.
    pub(crate) async fn resumed(&mut self) {
        while self.is_paused() {
            self.changed().await;
        }
    }

    async fn changed(&mut self) {
        tokio::select! {
            r = self.job.changed() => if r.is_err() { std::future::pending::<()>().await },
            r = self.all.changed() => if r.is_err() { std::future::pending::<()>().await },
        }
    }
}

pub const PART_SUFFIX: &str = ".filepart";
const CHUNK: usize = 256 * 1024;
const VERIFY_TAIL: u64 = 64 * 1024;
/// FAT and some servers store mtimes with 2 s granularity.
const MTIME_SLACK: u64 = 2;

/// What to do when the destination already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingPolicy {
    /// Replace it; resume a matching `.filepart` if one exists.
    #[default]
    Overwrite,
    /// Leave it alone.
    Skip,
    /// Skip when size and mtime match (cheap "sync"), else overwrite.
    SkipIfSame,
}

pub(crate) enum Outcome {
    Copied { bytes: u64, resumed_from: u64 },
    Skipped,
}

pub(crate) struct CopySpec<'a> {
    pub src: &'a Arc<dyn RemoteFs>,
    pub src_path: &'a str,
    pub dst: &'a Arc<dyn RemoteFs>,
    pub dst_path: &'a str,
    pub policy: ExistingPolicy,
    pub cancel: &'a CancellationToken,
    pub rate: Option<&'a RateLimiter>,
    pub pause: PauseGate,
}

/// Copy one file with resume. `progress(done, total)` is called at most
/// every 100 ms and once at the end.
pub(crate) async fn copy_file(
    s: &CopySpec<'_>,
    mut progress: impl FnMut(u64, u64),
) -> Result<Outcome, TransferError> {
    let src_meta = s
        .src
        .stat(s.src_path)
        .await?
        .ok_or_else(|| TransferError::SourceMissing(s.src_path.to_owned()))?;
    let total = src_meta.size;

    if let Some(existing) = s.dst.stat(s.dst_path).await? {
        match s.policy {
            ExistingPolicy::Skip => return Ok(Outcome::Skipped),
            ExistingPolicy::SkipIfSame
                if existing.size == total
                    && matches!((existing.modified, src_meta.modified),
                        (Some(a), Some(b)) if a.abs_diff(b) <= MTIME_SLACK) =>
            {
                return Ok(Outcome::Skipped);
            }
            _ => {}
        }
    }

    let part = format!("{}{PART_SUFFIX}", s.dst_path);
    let mut offset = resume_offset(s, &part, total).await?;

    let mut streams = None;
    if offset < total || total == 0 {
        streams = Some(match open_streams(s, &part, offset, total).await {
            Ok(rw) => rw,
            // SCP and friends move whole files only: start over.
            Err(TransferError::Fs(e)) if offset > 0 && e.is_unsupported() => {
                tracing::info!(
                    path = s.dst_path,
                    "cannot resume with this protocol; restarting"
                );
                offset = 0;
                open_streams(s, &part, 0, total).await?
            }
            Err(e) => return Err(e),
        });
    }
    let mut done = offset;
    progress(done, total);

    if let Some((mut reader, mut writer)) = streams {
        let mut buf = vec![0u8; CHUNK];
        let mut gate = s.pause.clone();
        let mut last_tick = tokio::time::Instant::now();
        loop {
            let n = tokio::select! {
                biased;
                _ = s.cancel.cancelled() => {
                    // Flush what we have so the .filepart is resumable.
                    let _ = writer.shutdown().await;
                    return Err(TransferError::Cancelled);
                }
                _ = gate.paused() => {
                    // Stop the stream, keep the partial file for resuming.
                    let _ = writer.shutdown().await;
                    return Err(TransferError::Paused);
                }
                r = reader.read(&mut buf) => r?,
            };
            if n == 0 {
                break;
            }
            if let Some(rl) = s.rate {
                rl.acquire(n).await;
            }
            writer.write_all(&buf[..n]).await?;
            done += n as u64;
            if last_tick.elapsed() >= Duration::from_millis(100) {
                progress(done, total);
                last_tick = tokio::time::Instant::now();
            }
        }
        // Waits for every in-flight write to be acknowledged.
        writer.shutdown().await?;
    }

    let written = s.dst.stat(&part).await?.map(|e| e.size).unwrap_or(0);
    if written != total {
        // Source changed size mid-copy, or a short write: don't publish it.
        return Err(TransferError::SizeMismatch {
            expected: total,
            actual: written,
        });
    }
    // Replace atomically where the protocol can (SFTP v3 rename alone does
    // not overwrite); the old file is never deleted before the new one is in
    // place.
    s.dst.rename_replace(&part, s.dst_path).await?;
    if let Some(m) = src_meta.modified
        && let Err(e) = s.dst.set_modified(s.dst_path, m).await
    {
        tracing::debug!(error = %e, path = s.dst_path, "could not preserve mtime");
    }
    progress(total, total);
    Ok(Outcome::Copied {
        bytes: total - offset,
        resumed_from: offset,
    })
}

async fn open_streams(
    s: &CopySpec<'_>,
    part: &str,
    offset: u64,
    total: u64,
) -> Result<(hostwick_sftp::BoxRead, hostwick_sftp::BoxWrite), TransferError> {
    let reader = s.src.open_read(s.src_path, offset).await?;
    let writer = s.dst.open_write_sized(part, offset, total - offset).await?;
    Ok((reader, writer))
}

/// Where to continue from: the length of an existing `.filepart`, if its last
/// bytes match the source at the same offset; otherwise 0 (also when either
/// side cannot read at an offset).
async fn resume_offset(s: &CopySpec<'_>, part: &str, total: u64) -> Result<u64, TransferError> {
    let Some(p) = s.dst.stat(part).await? else {
        return Ok(0);
    };
    if p.size == 0 || p.size > total {
        return Ok(0);
    }
    let len = p.size.min(VERIFY_TAIL);
    let at = p.size - len;
    let (a, b) = match tokio::try_join!(
        s.dst.read_range(part, at, len as usize),
        s.src.read_range(s.src_path, at, len as usize),
    ) {
        Ok(ab) => ab,
        Err(e) if e.is_unsupported() => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    if a == b && a.len() as u64 == len {
        tracing::info!(
            path = s.dst_path,
            offset = p.size,
            "resuming partial transfer"
        );
        Ok(p.size)
    } else {
        tracing::info!(
            path = s.dst_path,
            "partial file does not match source; restarting"
        );
        Ok(0)
    }
}
