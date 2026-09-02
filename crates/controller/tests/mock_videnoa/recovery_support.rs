use std::collections::BTreeMap;
use std::error::Error;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tempfile::TempDir;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, InputExtension, InputPath, OutputExtension, OutputPath,
    SourceReference, SubmissionKey, TaskCreateRequest, TaskId, TaskSource, TaskStatus,
    WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName, WorkflowKind, WorkflowName,
    WorkflowSummary,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, DownloadEvidence, LifecycleService, ReserveCommand, SubmissionEvidence,
    UploadEvidence,
};
use videnoa_controller::persistence::{
    Database, DatabaseOptions, InputIdentity, NewTask, NewWorker, SettingsUpdate, Store,
    TaskRecord, WorkerHealthUpdate,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts, UploadReceipt, VidenoaClient};

use super::mock_videnoa::server::MockVidenoa;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Fixture {
    pub _directory: TempDir,
    pub store: Store,
    pub service: LifecycleService,
    pub worker_id: WorkerId,
    pub now: DateTime<Utc>,
    server_url: WorkerApiUrl,
}

pub struct StateFixture {
    pub task_id: TaskId,
}

impl Fixture {
    pub async fn new(server: &MockVidenoa, slots: u64) -> TestResult<Self> {
        let directory = TempDir::new()?;
        let database = Database::open(DatabaseOptions::new(
            directory.path().join("controller.sqlite3"),
        ))
        .await?;
        let store = Store::new(database);
        let now = Utc
            .timestamp_opt(1_788_307_200, 0)
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
                compute_slots: ComputeSlots::try_from(slots)?,
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
        let settings = store.settings().await?;
        let mut scheduler = settings.scheduler;
        scheduler.prefetch_per_worker = u16::try_from(slots)?;
        store
            .update_settings(&SettingsUpdate {
                expected_version: settings.version,
                scheduler,
                timeouts: settings.timeouts,
                retry: settings.retry,
                updated_at: now,
            })
            .await?;
        Ok(Self {
            _directory: directory,
            service: LifecycleService::new(store.clone()),
            store,
            worker_id,
            now,
            server_url,
        })
    }

    pub async fn task_at(&self, status: TaskStatus) -> TestResult<StateFixture> {
        let task_id = self.insert_task().await?;
        if status == TaskStatus::Queued {
            return Ok(StateFixture { task_id });
        }
        let attempt_id = AttemptId::random();
        self.service
            .reserve(&ReserveCommand {
                task_id,
                expected_task_version: 0,
                worker_id: self.worker_id,
                attempt_id,
                submission_key: SubmissionKey::random(),
                reserved_at: self.now,
            })
            .await?;
        if status == TaskStatus::Reserved {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::StartUpload)
            .await?;
        if status == TaskStatus::Uploading {
            return Ok(StateFixture { task_id });
        }
        let (client, upload) = self.upload_input(task_id).await?;
        let output = videnoa_controller::remote::sibling_output_path(&upload.path, "output.mp4")?;
        self.advance(
            task_id,
            attempt_id,
            AdvanceCommand::FinishUpload(UploadEvidence {
                remote_input_path: upload.path.clone(),
                remote_output_path: output,
            }),
        )
        .await?;
        if status == TaskStatus::Staged {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::StartSubmission)
            .await?;
        if status == TaskStatus::Submitting {
            return Ok(StateFixture { task_id });
        }
        let remote_job_id = self
            .persist_submission(task_id, attempt_id, &client, upload)
            .await?;
        self.set_remote_running(remote_job_id, &client).await?;
        if status == TaskStatus::Processing {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::FinishProcessing)
            .await?;
        if status == TaskStatus::RemoteCompleted {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::StartDownload)
            .await?;
        if status == TaskStatus::Downloading {
            return Ok(StateFixture { task_id });
        }
        self.advance(
            task_id,
            attempt_id,
            AdvanceCommand::FinishDownload(DownloadEvidence {
                size: 4,
                sha256: videnoa_controller::persistence::Sha256Digest::new([1; 32]),
            }),
        )
        .await?;
        if status == TaskStatus::Verifying {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::FinishVerification)
            .await?;
        if status == TaskStatus::Publishing {
            return Ok(StateFixture { task_id });
        }
        self.advance(task_id, attempt_id, AdvanceCommand::FinishPublication)
            .await?;
        Ok(StateFixture { task_id })
    }

    async fn insert_task(&self) -> TestResult<TaskId> {
        let task_id = TaskId::random();
        self.store
            .insert_task(&NewTask {
                id: task_id,
                request: TaskCreateRequest {
                    input_path: InputPath::new(format!("/nas/input/{task_id}.mkv")),
                    output_path: OutputPath::new(format!("/nas/output/{task_id}.mp4")),
                    workflow: WorkflowName::new("eligible-workflow.json"),
                    priority: 10,
                    source: TaskSource::Api,
                    source_reference: Some(SourceReference::new("task-10")),
                },
                input_extension: InputExtension::new("mkv"),
                output_extension: OutputExtension::new("mp4"),
                input_size: 4,
                input_mtime: self.now,
                input_identity: InputIdentity::new([1; 16]),
                created_at: self.now,
            })
            .await?;
        Ok(task_id)
    }

    async fn upload_input(&self, task_id: TaskId) -> TestResult<(VidenoaClient, UploadReceipt)> {
        let client = self.client()?;
        let upload = client
            .upload(
                &videnoa_controller::remote::FileApiPath::parse(&format!("{task_id}/input.mkv"))?,
                4,
                std::io::Cursor::new(vec![1_u8, 2, 3, 4]),
            )
            .await?;
        Ok((client, upload))
    }

    async fn persist_submission(
        &self,
        task_id: TaskId,
        attempt_id: AttemptId,
        client: &VidenoaClient,
        upload: UploadReceipt,
    ) -> TestResult<videnoa_controller::domain::RemoteJobId> {
        let output = videnoa_controller::remote::sibling_output_path(&upload.path, "output.mp4")?;
        let mut params = BTreeMap::new();
        params.insert(
            "input".to_owned(),
            Value::String(upload.path.as_str().to_owned()),
        );
        params.insert(
            "output".to_owned(),
            Value::String(output.as_str().to_owned()),
        );
        let attempt = self
            .store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing"))?;
        let submitted = client
            .run(
                &WorkflowName::new("eligible-workflow.json"),
                attempt.attempt.submission_key,
                &params,
            )
            .await?;
        self.advance(
            task_id,
            attempt_id,
            AdvanceCommand::PersistSubmission(SubmissionEvidence {
                remote_job_id: submitted.receipt.id,
                remote_input_path: upload.path,
                remote_output_path: output,
            }),
        )
        .await?;
        Ok(submitted.receipt.id)
    }

    pub async fn load_task(&self, task_id: TaskId) -> TestResult<TaskRecord> {
        self.store
            .task(task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing").into())
    }

    async fn advance(
        &self,
        task_id: TaskId,
        attempt_id: AttemptId,
        command: AdvanceCommand,
    ) -> TestResult {
        let task = self.load_task(task_id).await?;
        let attempt = self
            .store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing"))?;
        self.service
            .advance(&task, &attempt, command, self.now)
            .await?;
        Ok(())
    }

    fn client(&self) -> TestResult<VidenoaClient> {
        Ok(VidenoaClient::new(
            self.server_url.clone(),
            RemoteTimeouts::new(
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(2),
                std::time::Duration::from_secs(1),
            )?,
            PayloadLimits::new(1024 * 1024, 4096)?,
        )?)
    }

    async fn set_remote_running(
        &self,
        remote_job_id: videnoa_controller::domain::RemoteJobId,
        client: &VidenoaClient,
    ) -> TestResult {
        let _ = client.job(remote_job_id).await?;
        Ok(())
    }
}
