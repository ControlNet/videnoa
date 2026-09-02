use std::error::Error;

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;
use uuid::Uuid;
use videnoa_controller::domain::{
    ComputeSlots, InputExtension, InputPath, OutputExtension, OutputPath, SourceReference,
    TaskCreateRequest, TaskId, TaskSource, WorkerApiUrl, WorkerCapabilities, WorkerCreateRequest,
    WorkerId, WorkerName, WorkflowKind, WorkflowName, WorkflowSummary,
};
use videnoa_controller::persistence::{
    Database, DatabaseOptions, InputContentIdentity, InputIdentity, NewTask, NewWorker, Store,
    WorkerHealthUpdate, WorkerRecord,
};
use videnoa_controller::workers::WorkerRegistry;

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Fixture {
    pub _directory: TempDir,
    pub store: Store,
    pub registry: WorkerRegistry,
    pub now: DateTime<Utc>,
}

pub async fn fixture() -> TestResult<Fixture> {
    let directory = TempDir::new()?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let store = Store::new(database);
    let now = timestamp(1_788_307_200)?;
    Ok(Fixture {
        _directory: directory,
        registry: WorkerRegistry::new(store.clone()),
        store,
        now,
    })
}

pub fn timestamp(seconds: i64) -> TestResult<DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or_else(|| std::io::Error::other("invalid test timestamp").into())
}

pub const fn task_id(value: u128) -> TaskId {
    TaskId::from_uuid(Uuid::from_u128(value))
}

pub const fn worker_id(value: u128) -> WorkerId {
    WorkerId::from_uuid(Uuid::from_u128(value))
}

pub fn worker_request(name: &str, url: &str, slots: u64) -> TestResult<WorkerCreateRequest> {
    Ok(WorkerCreateRequest {
        name: WorkerName::new(name),
        api_url: WorkerApiUrl::parse(url)?,
        enabled: true,
        compute_slots: ComputeSlots::try_from(slots)?,
    })
}

pub async fn create_worker_with_id(
    fixture: &Fixture,
    id: WorkerId,
    request: WorkerCreateRequest,
) -> TestResult<WorkerRecord> {
    fixture
        .store
        .insert_worker(&NewWorker {
            id,
            name: request.name,
            api_url: request.api_url,
            enabled: request.enabled,
            online: false,
            compute_slots: request.compute_slots,
            created_at: fixture.now,
        })
        .await?;
    fixture
        .registry
        .worker(id)
        .await?
        .ok_or_else(|| std::io::Error::other("created worker missing").into())
}

pub fn capabilities(workflows: &[&str], refreshed_at: DateTime<Utc>) -> WorkerCapabilities {
    WorkerCapabilities {
        workflows: workflows
            .iter()
            .map(|name| WorkflowSummary {
                name: WorkflowName::new(*name),
                kind: WorkflowKind::Workflow,
            })
            .collect(),
        refreshed_at: Some(refreshed_at),
    }
}

pub async fn online(
    fixture: &Fixture,
    id: WorkerId,
    version: u64,
    workflows: &[&str],
) -> TestResult {
    fixture
        .registry
        .refresh_health(WorkerHealthUpdate {
            id,
            expected_version: version,
            online: true,
            capabilities: capabilities(workflows, fixture.now),
            last_seen_at: Some(fixture.now),
            health_retry_count: 0,
            next_health_check_at: None,
            last_error: None,
            updated_at: fixture.now,
        })
        .await?;
    Ok(())
}

pub fn task(id: TaskId, workflow: &str, priority: i32, created_at: DateTime<Utc>) -> NewTask {
    NewTask {
        id,
        request: TaskCreateRequest {
            input_path: InputPath::new(format!("/input/{id}.mkv")),
            output_path: OutputPath::new(format!("/output/{id}.mp4")),
            workflow: WorkflowName::new(workflow),
            priority,
            source: TaskSource::Api,
            source_reference: Some(SourceReference::new("task-11")),
        },
        input_extension: InputExtension::new("mkv"),
        output_extension: OutputExtension::new("mp4"),
        input_size: 4_096,
        input_mtime: created_at,
        input_identity: InputIdentity::new([1; 16]),
        input_content_identity: InputContentIdentity::new([2; 16]),
        created_at,
    }
}
