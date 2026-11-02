use std::sync::Arc;
use std::time::Duration;

use hostwick_sftp::RemoteFs;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::TransferError;
use crate::rate::RateLimiter;

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
    let offset = resume_offset(s, &part, total).await?;
    let mut done = offset;
    progress(done, total);

    if done < total || total == 0 {
        let mut reader = s.src.open_read(s.src_path, offset).await?;
        let mut writer = s.dst.open_write(&part, offset).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut last_tick = tokio::time::Instant::now();
        loop {
            let n = tokio::select! {
                biased;
                _ = s.cancel.cancelled() => {
                    // Flush what we have so the .filepart is resumable.
                    let _ = writer.shutdown().await;
                    return Err(TransferError::Cancelled);
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
    if s.dst.stat(s.dst_path).await?.is_some() {
        // SFTP v3 rename does not overwrite.
        s.dst.remove_file(s.dst_path).await?;
    }
    s.dst.rename(&part, s.dst_path).await?;
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

/// Where to continue from: the length of an existing `.filepart`, if its last
/// bytes match the source at the same offset; otherwise 0.
async fn resume_offset(s: &CopySpec<'_>, part: &str, total: u64) -> Result<u64, TransferError> {
    let Some(p) = s.dst.stat(part).await? else {
        return Ok(0);
    };
    if p.size == 0 || p.size > total {
        return Ok(0);
    }
    let len = p.size.min(VERIFY_TAIL);
    let at = p.size - len;
    let (a, b) = tokio::try_join!(
        s.dst.read_range(part, at, len as usize),
        s.src.read_range(s.src_path, at, len as usize),
    )?;
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
