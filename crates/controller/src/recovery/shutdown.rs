use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::persistence::SettingsUpdate;
use crate::scheduler::{Scheduler, SchedulerError};

#[derive(Debug, thiserror::Error)]
pub enum ShutdownError {
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error(
        "shutdown drain timed out with {outstanding_stages} stages and {outstanding_writes} writes"
    )]
    DrainTimedOut {
        outstanding_stages: u64,
        outstanding_writes: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainOutcome {
    Drained,
    TimedOut { outstanding_writes: u64 },
}

#[derive(Debug)]
struct ShutdownState {
    accepting: AtomicBool,
    outstanding_stages: AtomicU64,
    outstanding_writes: AtomicU64,
    changes: watch::Sender<u64>,
    cancellation: CancellationToken,
}

#[derive(Clone, Debug)]
pub struct ShutdownCoordinator {
    state: Arc<ShutdownState>,
}

#[derive(Debug)]
pub struct StagePermit {
    state: Arc<ShutdownState>,
}

#[derive(Debug)]
pub struct WritePermit {
    state: Arc<ShutdownState>,
}

impl ShutdownCoordinator {
    #[must_use]
    pub fn new() -> Self {
        let (changes, _) = watch::channel(0);
        Self {
            state: Arc::new(ShutdownState {
                accepting: AtomicBool::new(true),
                outstanding_stages: AtomicU64::new(0),
                outstanding_writes: AtomicU64::new(0),
                changes,
                cancellation: CancellationToken::new(),
            }),
        }
    }

    #[must_use]
    pub fn begin_stage(&self) -> Option<StagePermit> {
        if !self.state.accepting.load(Ordering::SeqCst) {
            return None;
        }
        self.state.outstanding_stages.fetch_add(1, Ordering::SeqCst);
        self.state
            .changes
            .send_modify(|generation| *generation += 1);
        if self.state.accepting.load(Ordering::SeqCst) {
            Some(StagePermit {
                state: Arc::clone(&self.state),
            })
        } else {
            release_stage(&self.state);
            None
        }
    }

    pub fn stop_stage_intake(&self) {
        self.state.accepting.store(false, Ordering::SeqCst);
        self.state.cancellation.cancel();
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.state.cancellation.child_token()
    }

    #[must_use]
    pub fn outstanding_writes(&self) -> u64 {
        self.state.outstanding_writes.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn outstanding_stages(&self) -> u64 {
        self.state.outstanding_stages.load(Ordering::SeqCst)
    }

    pub async fn drain(&self, bound: Duration) -> DrainOutcome {
        let mut changes = self.state.changes.subscribe();
        let drained = tokio::time::timeout(bound, async {
            loop {
                if self.state.outstanding_stages.load(Ordering::SeqCst) == 0
                    && self.state.outstanding_writes.load(Ordering::SeqCst) == 0
                {
                    return;
                }
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
        match drained {
            Ok(()) => DrainOutcome::Drained,
            Err(_) => DrainOutcome::TimedOut {
                outstanding_writes: self.state.outstanding_writes.load(Ordering::SeqCst),
            },
        }
    }

    /// Persists scheduler pause before closing stage intake and draining durable writes.
    ///
    /// # Errors
    /// Returns an error when settings cannot be loaded or the pause cannot be committed.
    pub async fn shutdown(
        &self,
        scheduler: &Scheduler,
        now: chrono::DateTime<chrono::Utc>,
        bound: Duration,
    ) -> Result<DrainOutcome, ShutdownError> {
        let pause_result = async {
            let settings = scheduler.settings().await?;
            let mut status = settings.scheduler;
            status.paused = true;
            scheduler
                .update_settings(SettingsUpdate {
                    expected_version: settings.version,
                    scheduler: status,
                    timeouts: settings.timeouts,
                    retry: settings.retry,
                    updated_at: now,
                })
                .await
        }
        .await;
        self.stop_stage_intake();
        let outcome = self.drain(bound).await;
        pause_result?;
        match outcome {
            DrainOutcome::Drained => Ok(DrainOutcome::Drained),
            DrainOutcome::TimedOut { outstanding_writes } => Err(ShutdownError::DrainTimedOut {
                outstanding_stages: self.outstanding_stages(),
                outstanding_writes,
            }),
        }
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl StagePermit {
    #[must_use]
    pub fn begin_write(&self) -> WritePermit {
        let outstanding = self.state.outstanding_writes.fetch_add(1, Ordering::SeqCst) + 1;
        self.state.changes.send_replace(outstanding);
        WritePermit {
            state: Arc::clone(&self.state),
        }
    }
}

impl Drop for StagePermit {
    fn drop(&mut self) {
        release_stage(&self.state);
    }
}

impl Drop for WritePermit {
    fn drop(&mut self) {
        let outstanding = self.state.outstanding_writes.fetch_sub(1, Ordering::SeqCst) - 1;
        self.state.changes.send_replace(outstanding);
    }
}

fn release_stage(state: &ShutdownState) {
    state.outstanding_stages.fetch_sub(1, Ordering::SeqCst);
    state.changes.send_modify(|generation| *generation += 1);
}
