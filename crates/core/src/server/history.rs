//! Retention of finished job history.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use tracing::{info, warn};

use super::persistence::JobsPersistence;
use super::{AppState, Job, JobStatus};

impl JobStatus {
    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Sort key of a finished job: finish time, falling back to creation time.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct FinishedJob {
    finished_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    id: String,
}

impl FinishedJob {
    fn of(job: &Job) -> Option<Self> {
        job.status.is_terminal().then(|| Self {
            finished_at: job.completed_at.unwrap_or(job.created_at),
            created_at: job.created_at,
            id: job.id.clone(),
        })
    }
}

/// Ids of the finished jobs beyond the newest `limit`, oldest first.
/// Queued and running jobs are neither counted nor returned; `0` keeps all.
fn overflow(jobs: impl Iterator<Item = Option<FinishedJob>>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let mut finished: Vec<FinishedJob> = jobs.flatten().collect();
    let excess = finished.len().saturating_sub(limit);
    if excess == 0 {
        return Vec::new();
    }
    finished.sort_unstable();
    finished
        .into_iter()
        .take(excess)
        .map(|job| job.id)
        .collect()
}

/// Drops restored history beyond `limit` before the runtime map is shared.
///
/// The rows go in one transaction; if it fails every job stays loaded.
pub(super) fn prune_restored(
    persistence: &JobsPersistence,
    jobs: Vec<Job>,
    limit: usize,
) -> Vec<Job> {
    let pruned: HashSet<String> = overflow(jobs.iter().map(FinishedJob::of), limit)
        .into_iter()
        .collect();
    if pruned.is_empty() {
        return jobs;
    }
    if let Err(error) = persistence.delete_jobs(pruned.iter().map(String::as_str)) {
        warn!(
            error = ?error,
            history_limit = limit,
            "Failed to prune restored job history; keeping every restored job"
        );
        return jobs;
    }
    info!(
        pruned = pruned.len(),
        history_limit = limit,
        "Pruned restored job history beyond the configured limit"
    );
    jobs.into_iter()
        .filter(|job| !pruned.contains(&job.id))
        .collect()
}

impl AppState {
    /// Deletes the oldest finished jobs beyond `[jobs] history_limit`.
    pub(super) async fn enforce_job_history_limit(&self) {
        let limit = self.inner.config.read().await.jobs.history_limit;
        let jobs = self.inner.jobs.iter();
        let pruned = overflow(jobs.map(|entry| FinishedJob::of(entry.value())), limit);
        for id in pruned {
            self.prune_finished_job(id);
        }
    }

    /// Mirrors history deletion: the runtime entry is held until the durable
    /// row is gone, so a failed delete leaves the job fully visible.
    fn prune_finished_job(&self, id: String) {
        let Entry::Occupied(entry) = self.inner.jobs.entry(id) else {
            return;
        };
        if !entry.get().status.is_terminal() {
            return;
        }
        if let Some(persistence) = &self.inner.jobs_persistence {
            if let Err(error) = persistence.delete_job(entry.key()) {
                warn!(
                    job_id = %entry.key(),
                    error = ?error,
                    "Failed to prune finished job history row; keeping the job"
                );
                return;
            }
        }
        let (id, _) = entry.remove_entry();
        self.inner.progress_senders.remove(&id);
        info!(job_id = %id, "Pruned finished job beyond the history limit");
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use dashmap::DashMap;
    use rusqlite::Connection;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    use super::super::{run_job, AppState, Job, JobStatus, PipelineGraph};
    use crate::config::{AppConfig, JobsConfig};
    use crate::model_registry::ModelRegistry;
    use crate::registry::NodeRegistry;

    fn state_with_limit(root: &Path, history_limit: usize) -> AppState {
        let config = AppConfig {
            jobs: JobsConfig { history_limit },
            ..AppConfig::default()
        };
        AppState::new(
            NodeRegistry::new(),
            ModelRegistry::with_builtin_models(root.join("models")),
            DashMap::new(),
            config,
            root.join("config.toml"),
            root.join("data"),
        )
    }

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
            + chrono::Duration::minutes(minutes)
    }

    /// A job with an empty workflow, created at `created` and finished at `finished`.
    fn job(id: &str, status: JobStatus, created: i64, finished: Option<i64>) -> Job {
        let workflow: PipelineGraph =
            serde_json::from_value(serde_json::json!({"nodes": [], "connections": []})).unwrap();
        Job {
            id: id.to_string(),
            status,
            workflow,
            created_at: at(created),
            started_at: finished.map(|_| at(created)),
            completed_at: finished.map(at),
            progress: None,
            error: None,
            cancel_token: CancellationToken::new(),
            params: None,
            workflow_name: "history".to_string(),
            workflow_source: "api_jobs".to_string(),
            rerun_of_job_id: None,
        }
    }

    fn insert(state: &AppState, job: Job) {
        state.persist_job_snapshot(&job).unwrap();
        state.inner.jobs.insert(job.id.clone(), job);
    }

    fn persisted_ids(root: &Path) -> Vec<String> {
        let conn = Connection::open(root.join("data/jobs.db")).unwrap();
        let mut statement = conn.prepare("SELECT id FROM jobs ORDER BY id").unwrap();
        let ids = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        ids
    }

    fn memory_ids(state: &AppState) -> Vec<String> {
        let mut ids: Vec<String> = state
            .inner
            .jobs
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        ids.sort();
        ids
    }

    async fn finish(state: &AppState, id: &str) {
        insert(state, job(id, JobStatus::Queued, 100, None));
        tokio::time::timeout(Duration::from_secs(5), run_job(state.clone(), id.into()))
            .await
            .unwrap();
        assert_eq!(
            state.inner.jobs.get(id).unwrap().status,
            JobStatus::Completed
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn finishing_a_job_prunes_oldest_terminal_history_beyond_limit() {
        let root = TempDir::new().unwrap();
        let state = state_with_limit(root.path(), 3);
        // Finished at 10/20/30; the cancelled one only carries its creation time.
        insert(
            &state,
            job("a-completed", JobStatus::Completed, 0, Some(10)),
        );
        insert(&state, job("b-failed", JobStatus::Failed, 1, Some(30)));
        insert(&state, job("c-cancelled", JobStatus::Cancelled, 20, None));
        // Active jobs older than all history are never pruned or counted.
        insert(&state, job("d-queued", JobStatus::Queued, -50, None));
        insert(&state, job("e-running", JobStatus::Running, -40, None));

        finish(&state, "f-new").await;

        let expected = ["b-failed", "c-cancelled", "d-queued", "e-running", "f-new"];
        assert_eq!(memory_ids(&state), expected);
        assert_eq!(persisted_ids(root.path()), expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zero_history_limit_keeps_every_finished_job() {
        let root = TempDir::new().unwrap();
        let state = state_with_limit(root.path(), 0);
        for index in 0..5 {
            let id = format!("old-{index}");
            insert(&state, job(&id, JobStatus::Completed, index, Some(index)));
        }

        finish(&state, "new").await;

        assert_eq!(memory_ids(&state).len(), 6);
        assert_eq!(persisted_ids(root.path()).len(), 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_history_prune_keeps_the_job_in_memory() {
        let root = TempDir::new().unwrap();
        let state = state_with_limit(root.path(), 1);
        insert(&state, job("old", JobStatus::Completed, 0, Some(1)));
        let conn = Connection::open(root.path().join("data/jobs.db")).unwrap();
        // Deliberate database fault injection: a failed delete must not drop the record.
        conn.execute_batch(
            "CREATE TRIGGER reject_delete BEFORE DELETE ON jobs \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        finish(&state, "new").await;

        assert_eq!(memory_ids(&state), ["new", "old"]);
        assert_eq!(persisted_ids(root.path()), ["new", "old"]);
    }

    #[test]
    fn startup_prunes_restored_history_beyond_limit() {
        let root = TempDir::new().unwrap();
        let first = state_with_limit(root.path(), 0);
        for index in 0..5 {
            let id = format!("done-{index}");
            insert(&first, job(&id, JobStatus::Failed, index, Some(index)));
        }
        // Reconciled to cancelled at startup, finishing now: the newest history entry.
        insert(&first, job("interrupted", JobStatus::Running, -10, None));
        drop(first);

        let restarted = state_with_limit(root.path(), 3);

        let expected = ["done-3", "done-4", "interrupted"];
        assert_eq!(memory_ids(&restarted), expected);
        assert_eq!(persisted_ids(root.path()), expected);
        assert_eq!(
            restarted.inner.jobs.get("interrupted").unwrap().status,
            JobStatus::Cancelled
        );
    }

    #[test]
    fn failed_startup_prune_keeps_every_restored_job() {
        let root = TempDir::new().unwrap();
        let first = state_with_limit(root.path(), 0);
        for index in 0..3 {
            let id = format!("done-{index}");
            insert(&first, job(&id, JobStatus::Completed, index, Some(index)));
        }
        drop(first);
        let conn = Connection::open(root.path().join("data/jobs.db")).unwrap();
        // Deliberate database fault injection: the pruning transaction must roll back.
        conn.execute_batch(
            "CREATE TRIGGER reject_delete BEFORE DELETE ON jobs \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        let restarted = state_with_limit(root.path(), 1);

        let expected = ["done-0", "done-1", "done-2"];
        assert_eq!(memory_ids(&restarted), expected);
        assert_eq!(persisted_ids(root.path()), expected);
    }
}
