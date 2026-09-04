use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;
use videnoa_controller::config::PathConfig;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, RemoteJobId, TaskId, WorkerApiUrl, WorkerCapabilities, WorkerId,
    WorkerName, WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{
    Database, DatabaseOptions, NewWorker, Store, TaskRecord, WorkerHealthUpdate,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts, VidenoaClient};
use videnoa_controller::scheduler::{
    RuntimeSettings, TransferConfig, TransferCoordinator, TransferExecutor, TransferResources,
};

use crate::mock_videnoa::server::MockVidenoa;

#[path = "support/artifact_paths.rs"]
mod artifact_paths;
#[path = "support/task_lifecycle.rs"]
mod task_lifecycle;

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Fixture {
    pub directory: TempDir,
    _output_directory: Option<TempDir>,
    pub store: Store,
    pub paths: PathCapabilities,
    pub coordinator: TransferCoordinator,
    pub worker_id: WorkerId,
    pub now: DateTime<Utc>,
    pub input_root: PathBuf,
    pub output_root: PathBuf,
    pub temp_root: PathBuf,
    server_url: WorkerApiUrl,
}

pub fn zero_jitter() -> TestResult<videnoa_controller::lifecycle::JitterSample> {
    artifact_paths::zero_jitter()
}

pub fn verified_path(root: &Path, task_id: TaskId) -> PathBuf {
    artifact_paths::verified_path(root, task_id)
}

pub fn part_path(root: &Path, task_id: TaskId) -> PathBuf {
    artifact_paths::part_path(root, task_id)
}

pub fn evidence_path(root: &Path, task_id: TaskId) -> PathBuf {
    artifact_paths::evidence_path(root, task_id)
}

pub struct PreparedTask {
    pub task_id: TaskId,
    pub attempt_id: AttemptId,
}

impl Fixture {
    pub async fn new(server: &MockVidenoa, uploads: u16, downloads: u16) -> TestResult<Self> {
        let directory = TempDir::new()?;
        Self::open(server, uploads, downloads, directory, None).await
    }

    pub async fn new_with_output_directory(
        server: &MockVidenoa,
        uploads: u16,
        downloads: u16,
        output_directory: TempDir,
    ) -> TestResult<Self> {
        let directory = TempDir::new()?;
        Self::open(
            server,
            uploads,
            downloads,
            directory,
            Some(output_directory),
        )
        .await
    }

    async fn open(
        server: &MockVidenoa,
        uploads: u16,
        downloads: u16,
        directory: TempDir,
        output_directory: Option<TempDir>,
    ) -> TestResult<Self> {
        let input_root = directory.path().join("input");
        let output_root = output_directory.as_ref().map_or_else(
            || directory.path().join("output"),
            |root| root.path().to_path_buf(),
        );
        let data_root = directory.path().join("data");
        let temp_root = data_root.join("temp");
        for path in [&input_root, &output_root, &data_root, &temp_root] {
            std::fs::create_dir_all(path)?;
        }
        let path_config = PathConfig {
            input_roots: vec![input_root.clone()],
            output_roots: vec![output_root.clone()],
            data_root,
            temp_root: temp_root.clone(),
        };
        let paths = PathCapabilities::open(&path_config)?;
        let database = Database::open(DatabaseOptions::new(
            directory.path().join("controller.sqlite3"),
        ))
        .await?;
        let store = Store::new(database);
        let now = Utc
            .timestamp_opt(1_788_393_600, 0)
            .single()
            .ok_or_else(|| std::io::Error::other("invalid timestamp"))?;
        let worker_id = WorkerId::random();
        let server_url = WorkerApiUrl::parse(server.base_url())?;
        store
            .insert_worker(&NewWorker {
                id: worker_id,
                name: WorkerName::new("worker-a"),
                api_url: server_url.clone(),
                enabled: true,
                online: true,
                compute_slots: ComputeSlots::try_from(3_u64)?,
                created_at: now,
            })
            .await?;
        store
            .update_worker_health(&WorkerHealthUpdate {
                id: worker_id,
                expected_version: 0,
                online: true,
                capabilities: WorkerCapabilities {
                    workflows: vec![WorkflowSummary {
                        name: WorkflowName::new("eligible-workflow.json"),
                        kind: WorkflowKind::Workflow,
                    }],
                    refreshed_at: Some(now),
                },
                last_seen_at: Some(now),
                health_retry_count: 0,
                next_health_check_at: None,
                last_error: None,
                updated_at: now,
            })
            .await?;
        Ok(Self {
            directory,
            _output_directory: output_directory,
            store,
            paths,
            coordinator: TransferCoordinator::new(uploads, downloads)?,
            worker_id,
            now,
            input_root,
            output_root,
            temp_root,
            server_url,
        })
    }

    pub fn executor(&self) -> TestResult<TransferExecutor> {
        Ok(TransferExecutor::new(
            TransferResources {
                store: self.store.clone(),
                paths: self.paths.clone(),
                coordinator: self.coordinator.clone(),
            },
            TransferConfig {
                payload_limits: PayloadLimits::new(1024 * 1024, 4096)?,
                runtime_settings: RuntimeSettings::new(
                    &videnoa_controller::domain::TimeoutSettingsDto {
                        health_seconds: 1,
                        poll_seconds: 3,
                        transfer_seconds: 1,
                    },
                    &videnoa_controller::domain::RetrySettingsDto {
                        initial_seconds: 1,
                        maximum_seconds: 4,
                        max_attempts: 3,
                    },
                )?,
            },
        ))
    }

    pub async fn task(&self, task_id: TaskId) -> TestResult<TaskRecord> {
        self.store
            .task(task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing").into())
    }

    pub async fn attempt(
        &self,
        attempt_id: AttemptId,
    ) -> TestResult<videnoa_controller::persistence::AttemptRecord> {
        self.store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing").into())
    }

    pub fn client(&self) -> TestResult<VidenoaClient> {
        Ok(VidenoaClient::new(
            self.server_url.clone(),
            RemoteTimeouts::new(
                Duration::from_secs(1),
                Duration::from_secs(3),
                Duration::from_secs(1),
            )?,
            PayloadLimits::new(1024 * 1024, 4096)?,
        )?)
    }

    pub async fn remote_job_id(&self, attempt_id: AttemptId) -> TestResult<RemoteJobId> {
        self.attempt(attempt_id)
            .await?
            .attempt
            .remote_job_id
            .ok_or_else(|| std::io::Error::other("remote job missing").into())
    }
}
