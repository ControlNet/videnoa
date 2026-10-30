use videnoa_controller::domain::{RetryTaskResponse, Task, WorkerId};

use super::http::require_status;
use super::{ControllerFixture, TestResult};

const PASSWORD: &str = "task-20-test-only-password";

impl ControllerFixture {
    pub async fn pause_scheduler(&self) -> TestResult {
        self.set_scheduler_paused(true).await
    }

    pub async fn resume_scheduler(&self) -> TestResult {
        self.set_scheduler_paused(false).await
    }

    pub async fn cancel_status(&self, task: &Task) -> TestResult<reqwest::StatusCode> {
        self.task_action_status(task, "cancel").await
    }

    pub async fn abandon_status(&self, task: &Task) -> TestResult<reqwest::StatusCode> {
        self.task_action_status(task, "abandon").await
    }

    /// Deletes a Worker at its current version. Health probes keep bumping an
    /// offline Worker's version, so a stale-version conflict reads it again.
    pub async fn delete_worker_status(
        &self,
        worker_id: WorkerId,
    ) -> TestResult<reqwest::StatusCode> {
        for _ in 0..50 {
            let version = self
                .workers()
                .await?
                .into_iter()
                .find(|worker| worker.id == worker_id)
                .ok_or_else(|| std::io::Error::other("worker is not listed"))?
                .version;
            let response = self
                .client
                .delete(format!(
                    "{}/api/workers/{worker_id}?version={version}",
                    self.base_url
                ))
                .bearer_auth(PASSWORD)
                .send()
                .await?;
            let status = response.status();
            let stale = status == reqwest::StatusCode::CONFLICT
                && response.text().await?.contains("changed");
            if !stale {
                return Ok(status);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Err(std::io::Error::other("worker version kept changing").into())
    }

    pub async fn retry_task(&self, task: &Task) -> TestResult<RetryTaskResponse> {
        let detail = self.task(task).await?;
        let response = self
            .client
            .post(format!("{}/api/tasks/{}/retry", self.base_url, task.id))
            .bearer_auth(PASSWORD)
            .json(&serde_json::json!({ "version": detail.task.version }))
            .send()
            .await?;
        require_status(response.status(), reqwest::StatusCode::OK, "retry")?;
        Ok(response.json().await?)
    }

    async fn task_action_status(
        &self,
        task: &Task,
        action: &str,
    ) -> TestResult<reqwest::StatusCode> {
        let detail = self.task(task).await?;
        Ok(self
            .client
            .post(format!("{}/api/tasks/{}/{action}", self.base_url, task.id))
            .bearer_auth(PASSWORD)
            .json(&serde_json::json!({ "version": detail.task.version }))
            .send()
            .await?
            .status())
    }

    async fn set_scheduler_paused(&self, paused: bool) -> TestResult {
        let settings = self.store.config_manager().settings()?;
        let action = if paused { "pause" } else { "resume" };
        let response = self
            .client
            .post(format!("{}/api/scheduler/{action}", self.base_url))
            .bearer_auth(PASSWORD)
            .json(&serde_json::json!({ "version": settings.version }))
            .send()
            .await?;
        require_status(response.status(), reqwest::StatusCode::OK, action)
    }
}
