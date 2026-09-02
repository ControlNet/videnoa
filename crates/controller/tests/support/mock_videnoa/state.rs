use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::checkpoints::{Checkpoint, CheckpointHub};
use super::domain::{JobProgress, JobStatus};
use super::faults::{Fault, FaultState, ResponseFault};
use super::journal::{JournalEntry, LogicalTimestamp, Route, RouteCounters};
use super::persistence::{self, PersistentState};

#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("mock I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("mock JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("mock server task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("mock checkpoint {0} timed out")]
    CheckpointTimeout(&'static str),
    #[error("mock checkpoint channel closed")]
    CheckpointClosed,
    #[error("unknown checkpoint generation")]
    UnknownCheckpointGeneration,
    #[error("mock server shutdown timed out")]
    ShutdownTimeout,
    #[error("mock job not found: {0}")]
    JobNotFound(String),
    #[error("mock persistent mode was not enabled")]
    PersistenceDisabled,
    #[error("mock server is already online")]
    AlreadyOnline,
}

pub(crate) struct RuntimeState {
    pub persistent: PersistentState,
    pub faults: FaultState,
    pub journal: Vec<JournalEntry>,
    pub counters: RouteCounters,
    pub next_sequence: u64,
    pub next_timestamp: u64,
}

impl RuntimeState {
    pub fn new(persistent: PersistentState) -> Self {
        Self {
            persistent,
            faults: FaultState::default(),
            journal: Vec::new(),
            counters: RouteCounters::empty(),
            next_sequence: 0,
            next_timestamp: 0,
        }
    }

    pub fn begin(&mut self, route: Route) -> u64 {
        self.next_sequence += 1;
        self.counters.increment(route);
        self.next_sequence
    }

    pub fn timestamp(&mut self) -> LogicalTimestamp {
        self.next_timestamp += 1;
        LogicalTimestamp(self.next_timestamp)
    }
}

pub(crate) struct SharedState {
    pub inner: Mutex<RuntimeState>,
    pub checkpoints: CheckpointHub,
    persistence_path: Option<PathBuf>,
}

impl SharedState {
    pub fn new(persistent: PersistentState, persistence_path: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(RuntimeState::new(persistent)),
            checkpoints: CheckpointHub::new(),
            persistence_path,
        })
    }

    pub async fn checkpoint(
        &self,
        checkpoint: Checkpoint,
        timestamps: &mut BTreeMap<String, LogicalTimestamp>,
    ) {
        let timestamp = self.inner.lock().await.timestamp();
        let timestamp = self.checkpoints.arrive(checkpoint, timestamp).await;
        timestamps.insert(checkpoint.name().to_owned(), timestamp);
    }

    pub async fn persist_locked(&self, state: &RuntimeState) -> Result<(), HarnessError> {
        if let Some(path) = &self.persistence_path {
            persistence::save(path, &state.persistent).await?;
        }
        Ok(())
    }

    pub async fn install_fault(&self, fault: Fault) {
        self.inner.lock().await.faults.install(fault);
    }

    pub async fn take_disconnect(&self) -> bool {
        let mut state = self.inner.lock().await;
        std::mem::take(&mut state.faults.disconnect_before_accept)
    }

    pub async fn service_unavailable(&self) -> bool {
        self.inner.lock().await.faults.service_unavailable
    }

    pub async fn take_response_fault(&self, route: Route) -> Option<ResponseFault> {
        self.inner
            .lock()
            .await
            .faults
            .response_scripts
            .get_mut(&route)
            .and_then(std::collections::VecDeque::pop_front)
    }

    pub async fn set_service_unavailable(&self, enabled: bool) {
        self.inner.lock().await.faults.service_unavailable = enabled;
    }

    pub async fn journal(&self) -> Vec<JournalEntry> {
        let mut journal = self.inner.lock().await.journal.clone();
        journal.sort_by_key(|entry| entry.sequence);
        journal
    }

    pub async fn counters(&self) -> RouteCounters {
        self.inner.lock().await.counters.clone()
    }

    pub async fn store_file(&self, path: &str, bytes: &[u8]) -> Result<(), HarnessError> {
        let mut state = self.inner.lock().await;
        state
            .persistent
            .files
            .insert(path.to_owned(), bytes.to_vec());
        self.persist_locked(&state).await
    }

    pub async fn set_job(
        &self,
        id: &str,
        status: JobStatus,
        progress: Option<JobProgress>,
    ) -> Result<(), HarnessError> {
        let mut state = self.inner.lock().await;
        let job = state
            .persistent
            .jobs
            .get_mut(id)
            .ok_or_else(|| HarnessError::JobNotFound(id.to_owned()))?;
        job.response.status = status;
        job.response.progress = progress;
        if status == JobStatus::Running {
            job.response.started_at = Some("2026-09-02T00:00:01Z".to_owned());
        }
        if matches!(
            status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        ) {
            job.response.completed_at = Some("2026-09-02T00:00:02Z".to_owned());
        }
        self.persist_locked(&state).await
    }

    pub async fn job_count(&self) -> usize {
        self.inner.lock().await.persistent.jobs.len()
    }
}
