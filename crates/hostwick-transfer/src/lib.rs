//! Transfer engine.
//!
//! Every transfer is "copy a file from one [`RemoteFs`] to another", so upload
//! (local → SFTP), download (SFTP → local) and server-to-server all share one
//! code path — and the engine is tested with two local filesystems.
//!
//! Reliability properties:
//! - Data lands in `name.filepart` and is renamed into place only when
//!   complete and size-checked, so a crash never leaves a truncated file under
//!   the real name.
//! - Resume continues a `.filepart` after verifying its tail against the
//!   source, so a changed source is never spliced onto stale bytes.
//! - Transient failures retry with exponential backoff + jitter, resuming.
//! - Concurrency is bounded by a semaphore; bandwidth by a shared token bucket.
//! - Every job is cancellable.

mod copy;
mod engine;
mod rate;
mod sync;

pub use copy::{ExistingPolicy, PART_SUFFIX};
pub use engine::{EngineConfig, Job, TransferEngine, TransferEvent, TransferId, TransferState};
pub use rate::RateLimiter;
pub use sync::{FileInfo, SyncAction, SyncMode, plan_sync, walk};

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Fs(#[from] hostwick_sftp::FsError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("source not found: {0}")]
    SourceMissing(String),
    #[error("size mismatch after copy: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("cancelled")]
    Cancelled,
}

impl TransferError {
    pub(crate) fn is_transient(&self) -> bool {
        use hostwick_sftp::FsError;
        match self {
            Self::Io(_) => true,
            Self::Fs(FsError::Timeout(_) | FsError::Io(_)) => true,
            Self::Fs(FsError::Ssh(e)) => e.is_transient(),
            _ => false,
        }
    }
}
