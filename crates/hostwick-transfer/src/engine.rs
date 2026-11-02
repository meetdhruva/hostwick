use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hostwick_core::Backoff;
use hostwick_sftp::RemoteFs;
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, broadcast, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::TransferError;
use crate::copy::{CopySpec, ExistingPolicy, Outcome, copy_file};
use crate::rate::RateLimiter;
use crate::sync::walk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Running,
    Done { bytes: u64, resumed_from: u64 },
    Skipped,
    Failed { error: String },
    Cancelled,
}

impl TransferState {
    pub fn is_finished(&self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TransferEvent {
    Queued {
        id: TransferId,
        label: String,
    },
    Progress {
        id: TransferId,
        done: u64,
        total: u64,
        bytes_per_sec: f64,
    },
    Finished {
        id: TransferId,
        state: TransferState,
    },
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Files transferred in parallel (each file is itself pipelined).
    pub max_concurrent: usize,
    /// Attempts per file for transient errors (resume makes retries cheap).
    pub max_attempts: u32,
    /// Global cap in bytes/s; `None` = unlimited.
    pub rate_limit: Option<u64>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_attempts: 4,
            rate_limit: None,
        }
    }
}

/// One file to copy.
#[derive(Clone)]
pub struct Job {
    pub src: Arc<dyn RemoteFs>,
    pub src_path: String,
    pub dst: Arc<dyn RemoteFs>,
    pub dst_path: String,
    pub policy: ExistingPolicy,
}

impl Job {
    fn label(&self) -> String {
        format!("{} → {}", self.src_path, self.dst_path)
    }
}

struct Slot {
    cancel: CancellationToken,
    state: watch::Receiver<TransferState>,
}

/// Queue + worker pool for file transfers. Cheap to clone.
#[derive(Clone)]
pub struct TransferEngine {
    cfg: EngineConfig,
    permits: Arc<Semaphore>,
    rate: Option<RateLimiter>,
    events: broadcast::Sender<TransferEvent>,
    jobs: Arc<Mutex<HashMap<TransferId, Slot>>>,
    root_cancel: CancellationToken,
}

impl TransferEngine {
    pub fn new(cfg: EngineConfig) -> Self {
        let (events, _) = broadcast::channel(1024);
        Self {
            permits: Arc::new(Semaphore::new(cfg.max_concurrent.max(1))),
            rate: cfg.rate_limit.map(RateLimiter::new),
            cfg,
            events,
            jobs: Arc::default(),
            root_cancel: CancellationToken::new(),
        }
    }

    /// Progress/state stream. Lagging receivers skip old progress events
    /// rather than slowing transfers down.
    pub fn subscribe(&self) -> broadcast::Receiver<TransferEvent> {
        self.events.subscribe()
    }

    pub fn enqueue(&self, job: Job) -> TransferId {
        let id = TransferId(Uuid::new_v4());
        let cancel = self.root_cancel.child_token();
        let (state_tx, state_rx) = watch::channel(TransferState::Queued);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(
                id,
                Slot {
                    cancel: cancel.clone(),
                    state: state_rx,
                },
            );
        }
        let _ = self.events.send(TransferEvent::Queued {
            id,
            label: job.label(),
        });

        let this = self.clone();
        tokio::spawn(async move {
            let state = this.run(id, &job, &cancel, &state_tx).await;
            let _ = state_tx.send(state.clone());
            let _ = this.events.send(TransferEvent::Finished { id, state });
        });
        id
    }

    async fn run(
        &self,
        id: TransferId,
        job: &Job,
        cancel: &CancellationToken,
        state_tx: &watch::Sender<TransferState>,
    ) -> TransferState {
        let _permit = tokio::select! {
            p = self.permits.clone().acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => return TransferState::Cancelled,
            },
            _ = cancel.cancelled() => return TransferState::Cancelled,
        };
        let _ = state_tx.send(TransferState::Running);

        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(15))
            .with_max_attempts(self.cfg.max_attempts.saturating_sub(1));
        loop {
            let spec = CopySpec {
                src: &job.src,
                src_path: &job.src_path,
                dst: &job.dst,
                dst_path: &job.dst_path,
                policy: job.policy,
                cancel,
                rate: self.rate.as_ref(),
            };
            let started = Instant::now();
            let mut first: Option<u64> = None;
            let events = &self.events;
            let result = copy_file(&spec, |done, total| {
                let base = *first.get_or_insert(done);
                let secs = started.elapsed().as_secs_f64().max(1e-3);
                let rate = (done - base) as f64 / secs;
                let _ = events.send(TransferEvent::Progress {
                    id,
                    done,
                    total,
                    bytes_per_sec: rate,
                });
            })
            .await;
            match result {
                Ok(Outcome::Copied {
                    bytes,
                    resumed_from,
                }) => {
                    return TransferState::Done {
                        bytes,
                        resumed_from,
                    };
                }
                Ok(Outcome::Skipped) => return TransferState::Skipped,
                Err(TransferError::Cancelled) => return TransferState::Cancelled,
                Err(e) if e.is_transient() => match backoff.next_delay() {
                    Some(d) => {
                        tracing::warn!(error = %e, delay = ?d, "transfer failed; retrying with resume");
                        tokio::select! {
                            _ = tokio::time::sleep(d) => {}
                            _ = cancel.cancelled() => return TransferState::Cancelled,
                        }
                    }
                    None => {
                        return TransferState::Failed {
                            error: e.to_string(),
                        };
                    }
                },
                Err(e) => {
                    return TransferState::Failed {
                        error: e.to_string(),
                    };
                }
            }
        }
    }

    /// Copy a directory tree: creates directories, enqueues every file.
    pub async fn enqueue_tree(
        &self,
        src: Arc<dyn RemoteFs>,
        src_dir: &str,
        dst: Arc<dyn RemoteFs>,
        dst_dir: &str,
        policy: ExistingPolicy,
    ) -> Result<Vec<TransferId>, TransferError> {
        let files = walk(src.as_ref(), src_dir).await?;
        dst.mkdir_all(dst_dir).await?;
        let mut made = std::collections::HashSet::new();
        let mut ids = Vec::with_capacity(files.len());
        for f in files {
            // Defence in depth: listings already drop unsafe names, but a
            // path built from them must never leave `dst_dir`.
            let dst_path = hostwick_sftp::join_checked(dst_dir, &f.rel_path)?;
            let parent = hostwick_sftp::parent(&dst_path);
            if made.insert(parent.clone()) {
                dst.mkdir_all(&parent).await?;
            }
            ids.push(self.enqueue(Job {
                src: src.clone(),
                src_path: hostwick_sftp::join(src_dir, &f.rel_path),
                dst: dst.clone(),
                dst_path,
                policy,
            }));
        }
        Ok(ids)
    }

    pub fn cancel(&self, id: TransferId) {
        if let Some(slot) = self
            .jobs
            .lock()
            .ok()
            .and_then(|j| j.get(&id).map(|s| s.cancel.clone()))
        {
            slot.cancel();
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for slot in jobs.values() {
                slot.cancel.cancel();
            }
        }
    }

    pub fn state(&self, id: TransferId) -> Option<TransferState> {
        self.jobs
            .lock()
            .ok()?
            .get(&id)
            .map(|s| s.state.borrow().clone())
    }

    /// Wait for one job to finish.
    pub async fn wait(&self, id: TransferId) -> Option<TransferState> {
        let mut rx = self.jobs.lock().ok()?.get(&id).map(|s| s.state.clone())?;
        loop {
            let s = rx.borrow().clone();
            if s.is_finished() {
                return Some(s);
            }
            if rx.changed().await.is_err() {
                return Some(rx.borrow().clone());
            }
        }
    }

    /// Drop finished jobs from the table (the UI keeps its own history).
    pub fn prune_finished(&self) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.retain(|_, s| !s.state.borrow().is_finished());
        }
    }
}
