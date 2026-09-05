use std::error::Error;
use std::fs;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use videnoa_controller::auth::hash_password;
use videnoa_controller::config::{AuthConfig, PathConfig};
use videnoa_controller::domain::{
    InputPath, OutputPath, SourceReference, Task, TaskCreateRequest, TaskDetailResponse,
    TaskSource, WorkerSummary, WorkflowName,
};
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::scheduler::TransferCheckpointObserver;

use crate::mock_videnoa::server::MockVidenoa;

use super::admission::FixturePermit;
use super::http::{path_string, require_status};
use super::runtime::{start_runtime, ControllerRuntime, RuntimeOptions};

pub type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const PASSWORD: &str = "task-20-test-only-password";

#[path = "controller/capacity.rs"]
mod capacity;

pub struct ControllerFixture {
    _fixture_permit: FixturePermit,
    directory: TempDir,
    pub store: Store,
    pub input_root: PathBuf,
    pub output_root: PathBuf,
    pub temp_root: PathBuf,
    database_path: PathBuf,
    pub(super) path_config: PathConfig,
    auth_config: AuthConfig,
    pub(super) base_url: String,
    pub(super) client: reqwest::Client,
    runtime: Option<ControllerRuntime>,
    checkpoint_observer: Option<Arc<dyn TransferCheckpointObserver>>,
    recovery_page_size: Option<NonZeroU16>,
}

impl ControllerFixture {
    pub async fn start() -> TestResult<Self> {
        Self::start_with_options(None, None).await
    }

    pub async fn start_with_checkpoint_observer(
        checkpoint_observer: Option<Arc<dyn TransferCheckpointObserver>>,
    ) -> TestResult<Self> {
        Self::start_with_options(checkpoint_observer, None).await
    }

    pub async fn start_with_recovery_page_size_and_checkpoint(
        recovery_page_size: NonZeroU16,
        checkpoint_observer: Arc<dyn TransferCheckpointObserver>,
    ) -> TestResult<Self> {
        Self::start_with_options(Some(checkpoint_observer), Some(recovery_page_size)).await
    }

    async fn start_with_options(
        checkpoint_observer: Option<Arc<dyn TransferCheckpointObserver>>,
        recovery_page_size: Option<NonZeroU16>,
    ) -> TestResult<Self> {
        let fixture_permit = FixturePermit::acquire().await?;
        let directory = TempDir::new()?;
        let input_root = directory.path().join("input");
        let output_root = directory.path().join("output");
        let data_root = directory.path().join("data");
        let temp_root = data_root.join("temp");
        for path in [&input_root, &output_root, &data_root, &temp_root] {
            fs::create_dir_all(path)?;
        }
        let hash_file = data_root.join("admin-password.phc");
        fs::write(&hash_file, hash_password(PASSWORD)?)?;
        let path_config = PathConfig {
            input_roots: vec![input_root.clone()],
            output_roots: vec![output_root.clone()],
            data_root: data_root.clone(),
            temp_root: temp_root.clone(),
        };
        let auth_config = AuthConfig {
            password_hash_file: hash_file,
            secure_cookie: false,
            session_absolute: Duration::from_secs(86_400),
            session_idle: Duration::from_secs(3_600),
        };
        let database_path = data_root.join("controller.sqlite3");
        let database = Database::open(DatabaseOptions::new(database_path.clone())).await?;
        let store = Store::new(database);
        let runtime = start_runtime(
            directory.path(),
            &store,
            &path_config,
            &auth_config,
            RuntimeOptions {
                checkpoint_observer: checkpoint_observer.clone(),
                recovery_page_size,
            },
        )
        .await?;
        let base_url = format!("http://{}", runtime.address);
        Ok(Self {
            _fixture_permit: fixture_permit,
            directory,
            store,
            input_root,
            output_root,
            temp_root,
            database_path,
            path_config,
            auth_config,
            base_url,
            client: reqwest::Client::new(),
            runtime: Some(runtime),
            checkpoint_observer,
            recovery_page_size,
        })
    }

    pub async fn crash(&mut self) -> TestResult {
        let runtime = self
            .runtime
            .take()
            .ok_or_else(|| std::io::Error::other("Controller runtime is not running"))?;
        runtime.crash().await;
        Ok(())
    }

    pub async fn restart(&mut self) -> TestResult {
        if self.runtime.is_some() {
            return Err(std::io::Error::other("Controller runtime is already running").into());
        }
        let database = Database::open(DatabaseOptions::new(self.database_path.clone())).await?;
        self.store = Store::new(database);
        let runtime = start_runtime(
            self.directory.path(),
            &self.store,
            &self.path_config,
            &self.auth_config,
            RuntimeOptions {
                checkpoint_observer: self.checkpoint_observer.clone(),
                recovery_page_size: self.recovery_page_size,
            },
        )
        .await?;
        self.base_url = format!("http://{}", runtime.address);
        self.runtime = Some(runtime);
        Ok(())
    }

    pub async fn wait_for_orchestration_error(
        &mut self,
    ) -> TestResult<videnoa_controller::orchestration::OrchestrationError> {
        self.runtime
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Controller runtime is not running"))?
            .wait_for_orchestration_error()
            .await
    }

    pub async fn register_worker(
        &self,
        server: &MockVidenoa,
        name: &str,
    ) -> TestResult<WorkerSummary> {
        self.register_worker_enabled(server, name, true).await
    }

    pub async fn register_worker_enabled(
        &self,
        server: &MockVidenoa,
        name: &str,
        enabled: bool,
    ) -> TestResult<WorkerSummary> {
        let worker = self
            .register_worker_without_wait_with_slots(server, name, enabled, 1)
            .await?;
        if enabled {
            let worker_id = worker.id;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if self
                        .store
                        .worker(worker_id)
                        .await?
                        .is_some_and(|record| record.online)
                    {
                        return TestResult::Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .map_err(|_| std::io::Error::other("worker did not become online"))??;
        }
        Ok(worker)
    }

    pub async fn register_worker_without_wait(
        &self,
        server: &MockVidenoa,
        name: &str,
        enabled: bool,
    ) -> TestResult<WorkerSummary> {
        self.register_worker_without_wait_with_slots(server, name, enabled, 1)
            .await
    }

    pub async fn stop(mut self) -> TestResult {
        let runtime = self
            .runtime
            .take()
            .ok_or_else(|| std::io::Error::other("Controller runtime is not running"))?;
        runtime.stop().await
    }

    pub async fn create_task(&self, name: &str, input: &[u8]) -> TestResult<Task> {
        let input_path = self.input_root.join(format!("{name}.mkv"));
        let output_path = self.output_root.join(format!("{name}.mp4"));
        tokio::fs::write(&input_path, input).await?;
        let response = self
            .client
            .post(format!("{}/api/tasks", self.base_url))
            .bearer_auth(PASSWORD)
            .header("idempotency-key", format!("task-20-{name}"))
            .json(&TaskCreateRequest {
                input_path: InputPath::new(path_string(&input_path)?),
                output_path: OutputPath::new(path_string(&output_path)?),
                workflow: WorkflowName::new("eligible-workflow.json"),
                priority: 20,
                source: TaskSource::Api,
                source_reference: Some(SourceReference::new("task-20")),
            })
            .send()
            .await?;
        require_status(
            response.status(),
            reqwest::StatusCode::CREATED,
            "create task",
        )?;
        Ok(response.json::<Task>().await?)
    }

    pub async fn task(&self, task: &Task) -> TestResult<TaskDetailResponse> {
        let response = self
            .client
            .get(format!("{}/api/tasks/{}", self.base_url, task.id))
            .bearer_auth(PASSWORD)
            .send()
            .await?;
        require_status(response.status(), reqwest::StatusCode::OK, "read task")?;
        Ok(response.json::<TaskDetailResponse>().await?)
    }
}
