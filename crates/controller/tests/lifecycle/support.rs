use std::error::Error;

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, InputExtension, InputPath, OutputExtension, OutputPath,
    SourceReference, SubmissionKey, TaskCreateRequest, TaskId, TaskSource, WorkerApiUrl,
    WorkerCapabilities, WorkerId, WorkerName, WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, CancelAction, DurableAction, LifecycleService, ReserveCommand,
};
use videnoa_controller::persistence::{
    AttemptRecord, Database, DatabaseOptions, InputIdentity, NewTask, NewWorker, Store, TaskRecord,
    WorkerHealthUpdate,
};

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Fixture {
    pub _directory: TempDir,
    pub store: Store,
    pub service: LifecycleService,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub now: DateTime<Utc>,
}

pub fn timestamp(seconds: i64) -> TestResult<DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or_else(|| std::io::Error::other("invalid test timestamp").into())
}

pub async fn fixture() -> TestResult<Fixture> {
    let directory = TempDir::new()?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let now = timestamp(1_788_307_200)?;
    let task_id = TaskId::random();
    let worker_id = WorkerId::random();
    store
        .insert_worker(&NewWorker {
            id: worker_id,
            name: WorkerName::new("worker-a"),
            api_url: WorkerApiUrl::parse("https://worker.example/api/")?,
            enabled: true,
            online: true,
            compute_slots: ComputeSlots::try_from(2_u64)?,
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
                    name: WorkflowName::new("anime-upscale"),
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
    store
        .insert_task(&NewTask {
            id: task_id,
            request: TaskCreateRequest {
                input_path: InputPath::new("/nas/input/episode.v1.mkv"),
                output_path: OutputPath::new("/nas/output/episode.final.mp4"),
                workflow: WorkflowName::new("anime-upscale"),
                priority: 10,
                source: TaskSource::Api,
                source_reference: Some(SourceReference::new("fixture")),
            },
            input_extension: InputExtension::new("mkv"),
            output_extension: OutputExtension::new("mp4"),
            input_size: 4_096,
            input_mtime: now,
            input_identity: InputIdentity::new([1; 16]),
            created_at: now,
        })
        .await?;
    let service = LifecycleService::new(store.clone());
    Ok(Fixture {
        _directory: directory,
        store,
        service,
        task_id,
        worker_id,
        now,
    })
}

pub async fn reserve(fixture: &Fixture) -> TestResult<AttemptId> {
    let attempt_id = AttemptId::random();
    fixture
        .service
        .reserve(&ReserveCommand {
            task_id: fixture.task_id,
            expected_task_version: 0,
            worker_id: fixture.worker_id,
            attempt_id,
            submission_key: SubmissionKey::random(),
            reserved_at: fixture.now,
        })
        .await?;
    Ok(attempt_id)
}

pub async fn submitting_attempt(fixture: &Fixture) -> TestResult<AttemptId> {
    let attempt_id = reserve(fixture).await?;
    for command in [
        AdvanceCommand::StartUpload,
        AdvanceCommand::FinishUpload,
        AdvanceCommand::StartSubmission,
    ] {
        let task = load_task(fixture).await?;
        let attempt = load_attempt(fixture, attempt_id).await?;
        fixture
            .service
            .advance(&task, &attempt, command, fixture.now)
            .await?;
    }
    Ok(attempt_id)
}

pub async fn request_submitting_cancellation(
    fixture: &Fixture,
    attempt_id: AttemptId,
) -> TestResult {
    let task = load_task(fixture).await?;
    let attempt = load_attempt(fixture, attempt_id).await?;
    let requested = fixture
        .service
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await?;
    assert_eq!(
        requested.action(),
        DurableAction::Cancel(CancelAction::ReconcileSubmission)
    );
    Ok(())
}

pub async fn load_task(fixture: &Fixture) -> TestResult<TaskRecord> {
    fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing").into())
}

pub async fn load_attempt(fixture: &Fixture, attempt_id: AttemptId) -> TestResult<AttemptRecord> {
    fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing").into())
}
