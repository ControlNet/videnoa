use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use tokio::net::TcpListener;

use super::checkpoints::{Checkpoint, CheckpointTicket};
use super::domain::{JobProgress, JobStatus};
use super::faults::{Fault, OfflineMode, RestartMode, RestartOutcome};
use super::journal::{sanitize_entries, JournalEntry, RouteCounters};
use super::persistence::{self, PersistentState};
use super::state::{HarnessError, SharedState};
use super::transport::{spawn_runtime, ServerRuntime};

pub struct MockVidenoa {
    _directory: TempDir,
    persistence_path: Option<PathBuf>,
    address: SocketAddr,
    base_url: String,
    state: Arc<SharedState>,
    runtime: Option<ServerRuntime>,
}

impl MockVidenoa {
    pub async fn start() -> Result<Self, HarnessError> {
        Self::start_with_persistence(false).await
    }

    pub async fn start_persistent() -> Result<Self, HarnessError> {
        Self::start_with_persistence(true).await
    }

    async fn start_with_persistence(enabled: bool) -> Result<Self, HarnessError> {
        let directory = TempDir::new()?;
        let persistence_path = enabled.then(|| directory.path().join("mock-state.json"));
        let persistent = PersistentState::default();
        if let Some(path) = &persistence_path {
            persistence::save(path, &persistent).await?;
        }
        let state = SharedState::new(persistent, persistence_path.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let runtime = spawn_runtime(listener, Arc::clone(&state));
        Ok(Self {
            _directory: directory,
            persistence_path,
            address,
            base_url: format!("http://{address}"),
            state,
            runtime: Some(runtime),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn pause(&self, checkpoint: Checkpoint) -> CheckpointTicket {
        self.state.checkpoints.pause(checkpoint).await
    }

    pub async fn await_checkpoint(&self, ticket: &CheckpointTicket) -> Result<(), HarnessError> {
        self.state.checkpoints.await_reached(ticket).await
    }

    pub async fn release(&self, ticket: CheckpointTicket) -> Result<(), HarnessError> {
        self.state.checkpoints.release(ticket).await
    }

    pub async fn set_fault(&self, fault: Fault) {
        self.state.install_fault(fault).await;
    }

    pub async fn set_job(
        &self,
        id: &str,
        status: JobStatus,
        progress: Option<JobProgress>,
    ) -> Result<(), HarnessError> {
        self.state.set_job(id, status, progress).await
    }

    pub async fn complete_job(
        &self,
        id: &str,
        output_path: &str,
        bytes: &[u8],
    ) -> Result<(), HarnessError> {
        self.state.store_file(output_path, bytes).await?;
        self.state.set_job(id, JobStatus::Completed, None).await
    }

    pub async fn store_file(&self, path: &str, bytes: &[u8]) -> Result<(), HarnessError> {
        self.state.store_file(path, bytes).await
    }

    pub async fn counters(&self) -> RouteCounters {
        self.state.counters().await
    }

    pub async fn journal(&self) -> Vec<JournalEntry> {
        self.state.journal().await
    }

    pub async fn job_count(&self) -> usize {
        self.state.job_count().await
    }

    pub async fn active_job_count(&self) -> usize {
        self.state.active_job_count().await
    }

    pub async fn peak_active_jobs(&self) -> usize {
        self.state.peak_active_jobs().await
    }

    pub async fn accepted_upload_bytes(&self) -> u64 {
        self.state.inner.lock().await.accepted_upload_bytes
    }

    pub async fn file_count(&self) -> usize {
        self.state.file_count().await
    }

    pub async fn set_offline(&mut self, mode: OfflineMode) -> Result<(), HarnessError> {
        match mode {
            OfflineMode::ServiceUnavailable => {
                self.state.set_service_unavailable(true).await;
            }
            OfflineMode::ConnectionRefused => {
                let runtime = self.runtime.take().ok_or(HarnessError::AlreadyOnline)?;
                runtime.stop().await?;
            }
        }
        Ok(())
    }

    pub async fn resume(&mut self) -> Result<(), HarnessError> {
        self.state.set_service_unavailable(false).await;
        if self.runtime.is_none() {
            let listener = TcpListener::bind(self.address).await?;
            self.runtime = Some(spawn_runtime(listener, Arc::clone(&self.state)));
        }
        Ok(())
    }

    pub async fn restart(&mut self, mode: RestartMode) -> Result<RestartOutcome, HarnessError> {
        if let Some(runtime) = self.runtime.take() {
            runtime.stop().await?;
        }
        let path = self
            .persistence_path
            .as_ref()
            .ok_or(HarnessError::PersistenceDisabled)?;
        let (persistent, outcome) = match mode {
            RestartMode::RetainState => {
                let mut persistent = persistence::load(path).await?;
                persistent.cancel_active_jobs();
                persistence::save(path, &persistent).await?;
                (persistent, RestartOutcome::Retained)
            }
            RestartMode::LoseState => {
                let persistent = PersistentState::default();
                persistence::save(path, &persistent).await?;
                (persistent, RestartOutcome::StateLostAmbiguous)
            }
        };
        self.state = SharedState::new(persistent, self.persistence_path.clone());
        let listener = TcpListener::bind(self.address).await?;
        self.runtime = Some(spawn_runtime(listener, Arc::clone(&self.state)));
        Ok(outcome)
    }

    pub async fn write_happy_evidence_if_requested(&self) -> Result<(), HarnessError> {
        let Some(path) = std::env::var_os("VIDENOA_MOCK_HAPPY_EVIDENCE") else {
            return Ok(());
        };
        self.write_journal(path).await
    }

    pub async fn write_journal(&self, path: impl AsRef<Path>) -> Result<(), HarnessError> {
        let entries = sanitize_entries(&self.journal().await);
        let mut bytes = serde_json::to_vec_pretty(&entries)?;
        bytes.push(b'\n');
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    pub async fn write_fault_evidence_if_requested(&self) -> Result<(), HarnessError> {
        let Some(path) = std::env::var_os("VIDENOA_MOCK_FAULT_EVIDENCE") else {
            return Ok(());
        };
        let output = format!(
            "supported=disconnect_before_accept,accept_then_drop_run_response,restart_cancelled,offline_connect,offline_503,truncated_stream,delayed_poll,delete_404,delete_5xx,corrupt_output\ncounters={}\njournal_entries={}\naccepted_upload_bytes={}\nremote_file_count={}\n",
            serde_json::to_string(&self.counters().await)?,
            self.journal().await.len(),
            self.accepted_upload_bytes().await,
            self.file_count().await
        );
        tokio::fs::write(path, output).await?;
        Ok(())
    }
}

impl Drop for MockVidenoa {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.abort();
        }
    }
}
