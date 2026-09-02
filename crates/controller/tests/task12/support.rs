use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tempfile::TempDir;
use videnoa_controller::config::PathConfig;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, InputExtension, InputPath, OutputExtension, OutputPath, RemoteJobId,
    SourceReference, SubmissionKey, TaskCreateRequest, TaskId, TaskSource, WorkerApiUrl,
    WorkerCapabilities, WorkerId, WorkerName, WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleService, ReserveCommand, SubmissionEvidence,
};
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{
    Database, DatabaseOptions, InputIdentity, NewTask, NewWorker, Store, TaskRecord,
    WorkerHealthUpdate,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts, VidenoaClient};
use videnoa_controller::scheduler::{
    RuntimeSettings, TransferConfig, TransferCoordinator, TransferExecutor, TransferResources,
};

use crate::mock_videnoa::server::MockVidenoa;

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
                temp_root: self.temp_root.clone(),
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

    pub async fn reserved_task(&self, input_bytes: Vec<u8>) -> TestResult<PreparedTask> {
        let task_id = TaskId::random();
        let input_path = self.input_root.join(format!("{task_id}.mkv"));
        tokio::fs::write(&input_path, &input_bytes).await?;
        let rooted = self.paths.open_input(&input_path)?;
        let attempt_id = AttemptId::random();
        self.store
            .insert_task(&NewTask {
                id: task_id,
                request: TaskCreateRequest {
                    input_path: InputPath::new(path_string(&input_path)?),
                    output_path: OutputPath::new(path_string(
                        &self.output_root.join(format!("{task_id}.mp4")),
                    )?),
                    workflow: WorkflowName::new("eligible-workflow.json"),
                    priority: 10,
                    source: TaskSource::Api,
                    source_reference: Some(SourceReference::new("task-12")),
                },
                input_extension: InputExtension::new("mkv"),
                output_extension: OutputExtension::new("mp4"),
                input_size: rooted.snapshot().length,
                input_mtime: DateTime::<Utc>::from(rooted.snapshot().modified),
                input_identity: InputIdentity::new(rooted.snapshot().platform_identity()),
                created_at: self.now,
            })
            .await?;
        LifecycleService::new(self.store.clone())
            .reserve(&ReserveCommand {
                task_id,
                expected_task_version: 0,
                worker_id: self.worker_id,
                attempt_id,
                submission_key: SubmissionKey::random(),
                reserved_at: self.now,
            })
            .await?;
        Ok(PreparedTask {
            task_id,
            attempt_id,
        })
    }

    pub async fn remote_completed(
        &self,
        server: &MockVidenoa,
        output_bytes: &[u8],
    ) -> TestResult<PreparedTask> {
        let prepared = self.reserved_task(vec![7_u8; 20_000]).await?;
        let executor = self.executor()?;
        let upload = executor
            .upload(prepared.task_id, self.now, JitterSample::try_from(0)?)
            .await?;
        let evidence = match upload {
            videnoa_controller::scheduler::UploadOutcome::Staged(evidence) => evidence,
            other => {
                return Err(std::io::Error::other(format!("unexpected upload: {other:?}")).into())
            }
        };
        let service = LifecycleService::new(self.store.clone());
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartSubmission,
        )
        .await?;
        let attempt = self.attempt(prepared.attempt_id).await?;
        let params = BTreeMap::from([
            (
                "input".to_owned(),
                Value::String(evidence.remote_input_path.as_str().to_owned()),
            ),
            (
                "output".to_owned(),
                Value::String(evidence.remote_output_path.as_str().to_owned()),
            ),
        ]);
        let submitted = self
            .client()?
            .run(
                &WorkflowName::new("eligible-workflow.json"),
                attempt.attempt.submission_key,
                &params,
            )
            .await?;
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::PersistSubmission(SubmissionEvidence {
                remote_job_id: submitted.receipt.id,
                remote_input_path: evidence.remote_input_path,
                remote_output_path: evidence.remote_output_path,
            }),
        )
        .await?;
        server
            .complete_job(
                &submitted.receipt.id.to_string(),
                &format!("{}/output.mp4", prepared.task_id),
                output_bytes,
            )
            .await?;
        let task = self.task(prepared.task_id).await?;
        let attempt = self.attempt(prepared.attempt_id).await?;
        service
            .advance(&task, &attempt, AdvanceCommand::FinishProcessing, self.now)
            .await?;
        Ok(prepared)
    }

    pub async fn mark_uploading(&self, prepared: &PreparedTask) -> TestResult {
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartUpload,
        )
        .await
    }

    pub async fn mark_downloading(&self, prepared: &PreparedTask) -> TestResult {
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartDownload,
        )
        .await
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

    async fn advance(
        &self,
        task_id: TaskId,
        attempt_id: AttemptId,
        command: AdvanceCommand,
    ) -> TestResult {
        let task = self.task(task_id).await?;
        let attempt = self.attempt(attempt_id).await?;
        LifecycleService::new(self.store.clone())
            .advance(&task, &attempt, command, self.now)
            .await?;
        Ok(())
    }
}

pub fn zero_jitter() -> TestResult<JitterSample> {
    Ok(JitterSample::try_from(0)?)
}

pub fn verified_path(root: &Path, task_id: TaskId) -> PathBuf {
    root.join(task_id.to_string()).join("output.mp4.verified")
}

pub fn part_path(root: &Path, task_id: TaskId) -> PathBuf {
    root.join(task_id.to_string()).join("output.mp4.part")
}

fn path_string(path: &Path) -> TestResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("test path is not UTF-8").into())
}
