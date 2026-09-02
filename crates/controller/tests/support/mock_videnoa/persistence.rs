use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::domain::{JobRecord, JobStatus};
use super::state::HarnessError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct IdempotencyRecord {
    pub fingerprint: String,
    pub job_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct PersistentState {
    pub files: BTreeMap<String, Vec<u8>>,
    pub jobs: BTreeMap<String, JobRecord>,
    pub idempotency: BTreeMap<String, IdempotencyRecord>,
    pub next_job: u64,
}

impl PersistentState {
    pub fn cancel_active_jobs(&mut self) {
        for job in self.jobs.values_mut() {
            if job.response.status.is_active() {
                job.response.status = JobStatus::Cancelled;
                job.response.completed_at = Some("2026-09-02T00:00:02Z".to_owned());
                job.response.progress = None;
            }
        }
    }
}

pub(crate) async fn load(path: &Path) -> Result<PersistentState, HarnessError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistentState::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn save(path: &Path, state: &PersistentState) -> Result<(), HarnessError> {
    let bytes = serde_json::to_vec_pretty(state)?;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}
