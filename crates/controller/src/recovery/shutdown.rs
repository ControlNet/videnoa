use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::persistence::{CasOutcome, PersistenceError, SettingsUpdate, Store};

#[derive(Debug, thiserror::Error)]
pub enum ShutdownError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("controller settings changed while shutdown was being persisted")]
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainOutcome {
    Drained,
    TimedOut { outstanding_writes: u64 },
}

#[derive(Debug)]
struct ShutdownState {
    accepting: AtomicBool,
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
                outstanding_writes: AtomicU64::new(0),
                changes,
                cancellation: CancellationToken::new(),
            }),
        }
    }

    #[must_use]
    pub fn begin_stage(&self) -> Option<StagePermit> {
        self.state
            .accepting
            .load(Ordering::SeqCst)
            .then(|| StagePermit {
                state: Arc::clone(&self.state),
            })
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

    pub async fn drain(&self, bound: Duration) -> DrainOutcome {
        let mut changes = self.state.changes.subscribe();
        let drained = tokio::time::timeout(bound, async {
            loop {
                if self.state.outstanding_writes.load(Ordering::SeqCst) == 0 {
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
        store: &Store,
        now: chrono::DateTime<chrono::Utc>,
        bound: Duration,
    ) -> Result<DrainOutcome, ShutdownError> {
        let settings = store.settings().await?;
        let mut scheduler = settings.scheduler;
        scheduler.paused = true;
        let outcome = store
            .update_settings(&SettingsUpdate {
                expected_version: settings.version,
                scheduler,
                timeouts: settings.timeouts,
                retry: settings.retry,
                updated_at: now,
            })
            .await?;
        if outcome == CasOutcome::Conflict {
            return Err(ShutdownError::Conflict);
        }
        self.stop_stage_intake();
        Ok(self.drain(bound).await)
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

impl Drop for WritePermit {
    fn drop(&mut self) {
        let outstanding = self.state.outstanding_writes.fetch_sub(1, Ordering::SeqCst) - 1;
        self.state.changes.send_replace(outstanding);
    }
}
