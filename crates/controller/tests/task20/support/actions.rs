use videnoa_controller::domain::{RetryTaskResponse, Task};

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
        let settings = self.store.settings().await?;
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
