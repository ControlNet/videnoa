use std::error::Error;

use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;
use videnoa_controller::config::PathConfig;
use videnoa_controller::domain::{
    ComputeSlots, TaskId, WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName, WorkflowKind,
    WorkflowName, WorkflowSummary,
};
use videnoa_controller::lifecycle::LifecycleService;
use videnoa_controller::paths::PathCapabilities;
use videnoa_controller::persistence::{
    Database, DatabaseOptions, NewWorker, SettingsUpdate, Store, TaskRecord, WorkerHealthUpdate,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts, VidenoaClient};

use super::mock_videnoa::server::MockVidenoa;

#[path = "recovery_support/state_builder.rs"]
mod state_builder;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Fixture {
    pub _directory: TempDir,
    pub store: Store,
    pub service: LifecycleService,
    pub paths: PathCapabilities,
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
        let temp_root = directory.path().join("temp");
        let input_root = directory.path().join("input");
        let output_root = directory.path().join("output");
        let data_root = directory.path().join("data");
        for path in [&temp_root, &input_root, &output_root, &data_root] {
            std::fs::create_dir_all(path)?;
        }
        let paths = PathCapabilities::open(&PathConfig {
            input_roots: vec![input_root],
            output_roots: vec![output_root],
            data_root,
            temp_root: temp_root.clone(),
        })?;
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
            paths,
            worker_id,
            now,
            server_url,
        })
    }

    pub async fn load_task(&self, task_id: TaskId) -> TestResult<TaskRecord> {
        self.store
            .task(task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing").into())
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
}
