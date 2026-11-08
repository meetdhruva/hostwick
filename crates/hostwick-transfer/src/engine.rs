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
use crate::copy::{CopySpec, ExistingPolicy, Outcome, PauseGate, copy_file};
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
        /// Where the file goes, and whether that is a folder on this
        /// computer (for "show in folder").
        dst_path: String,
        dst_local: bool,
        /// Where it comes from, and whether that is this computer.
        src_path: String,
        src_local: bool,
        /// A retry of this earlier (failed or cancelled) transfer, which it
        /// replaces in lists.
        replaces: Option<TransferId>,
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
    /// One transfer was paused or resumed.
    Paused { id: TransferId, paused: bool },
    /// Every transfer was paused (or resumed) at once.
    PausedAll { paused: bool },
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
    /// Kept to retry the transfer after it failed.
    job: Job,
    paused: watch::Sender<bool>,
}

fn is_local(fs: &Arc<dyn RemoteFs>) -> bool {
    fs.protocol() == "local"
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
    paused_all: Arc<watch::Sender<bool>>,
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
            paused_all: Arc::new(watch::channel(false).0),
        }
    }

    /// Progress/state stream. Lagging receivers skip old progress events
    /// rather than slowing transfers down.
    pub fn subscribe(&self) -> broadcast::Receiver<TransferEvent> {
        self.events.subscribe()
    }

    pub fn enqueue(&self, job: Job) -> TransferId {
        self.enqueue_inner(job, None)
    }

    fn enqueue_inner(&self, job: Job, replaces: Option<TransferId>) -> TransferId {
        let id = TransferId(Uuid::new_v4());
        let cancel = self.root_cancel.child_token();
        let (state_tx, state_rx) = watch::channel(TransferState::Queued);
        let (pause_tx, pause_rx) = watch::channel(false);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(
                id,
                Slot {
                    cancel: cancel.clone(),
                    state: state_rx,
                    job: job.clone(),
                    paused: pause_tx,
                },
            );
        }
        let _ = self.events.send(TransferEvent::Queued {
            id,
            label: job.label(),
            dst_path: job.dst_path.clone(),
            dst_local: is_local(&job.dst),
            src_path: job.src_path.clone(),
            src_local: is_local(&job.src),
            replaces,
        });

        let gate = PauseGate::new(pause_rx, self.paused_all.subscribe());
        let this = self.clone();
        tokio::spawn(async move {
            let state = this.run(id, &job, &cancel, &state_tx, gate).await;
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
        mut gate: PauseGate,
    ) -> TransferState {
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(15))
            .with_max_attempts(self.cfg.max_attempts.saturating_sub(1));
        loop {
            // A paused transfer holds no slot: wait here, not in the queue.
            tokio::select! {
                _ = gate.resumed() => {}
                _ = cancel.cancelled() => return TransferState::Cancelled,
            }
            let permit = tokio::select! {
                p = self.permits.clone().acquire_owned() => match p {
                    Ok(p) => p,
                    Err(_) => return TransferState::Cancelled,
                },
                _ = cancel.cancelled() => return TransferState::Cancelled,
            };
            if gate.is_paused() {
                // Paused while waiting for a slot.
                continue;
            }
            let _ = state_tx.send(TransferState::Running);

            let spec = CopySpec {
                src: &job.src,
                src_path: &job.src_path,
                dst: &job.dst,
                dst_path: &job.dst_path,
                policy: job.policy,
                cancel,
                rate: self.rate.as_ref(),
                pause: gate.clone(),
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
            drop(permit);
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
                // Back to the top: wait for the resume, then continue from
                // the partial file.
                Err(TransferError::Paused) => {
                    let _ = state_tx.send(TransferState::Queued);
                }
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

    /// Pause one transfer: the stream stops and the partial file stays;
    /// [`TransferEngine::resume`] continues from it (protocols that cannot
    /// continue, like SCP, start over). A queued transfer waits without
    /// taking a slot.
    pub fn pause(&self, id: TransferId) {
        self.set_paused(id, true);
    }

    pub fn resume(&self, id: TransferId) {
        self.set_paused(id, false);
    }

    fn set_paused(&self, id: TransferId, paused: bool) {
        let changed = self.jobs.lock().ok().is_some_and(|jobs| {
            jobs.get(&id).is_some_and(|s| {
                !s.state.borrow().is_finished() && s.paused.send_replace(paused) != paused
            })
        });
        if changed {
            let _ = self.events.send(TransferEvent::Paused { id, paused });
        }
    }

    /// Pause every transfer, including ones queued later, until
    /// [`TransferEngine::resume_all`].
    pub fn pause_all(&self) {
        self.set_paused_all(true);
    }

    pub fn resume_all(&self) {
        self.set_paused_all(false);
    }

    fn set_paused_all(&self, paused: bool) {
        if self.paused_all.send_replace(paused) != paused {
            let _ = self.events.send(TransferEvent::PausedAll { paused });
        }
    }

    pub fn is_paused_all(&self) -> bool {
        *self.paused_all.borrow()
    }

    /// Run a failed (or cancelled) transfer again as a new one; a partial
    /// file left by the first try is continued. `None` when `id` is unknown
    /// or has not ended in failure or cancellation.
    pub fn retry(&self, id: TransferId) -> Option<TransferId> {
        let job = {
            let jobs = self.jobs.lock().ok()?;
            let slot = jobs.get(&id)?;
            match &*slot.state.borrow() {
                TransferState::Failed { .. } | TransferState::Cancelled => {}
                _ => return None,
            }
            slot.job.clone()
        };
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(&id);
        }
        Some(self.enqueue_inner(job, Some(id)))
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
