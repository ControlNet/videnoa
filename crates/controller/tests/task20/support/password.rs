use super::{ControllerFixture, TestResult};
use crate::mock_videnoa::server::MockVidenoa;
use videnoa_controller::domain::{WorkerId, WorkerSummary};

impl ControllerFixture {
    pub async fn register_protected_worker(
        &self,
        worker: &MockVidenoa,
        password: &str,
    ) -> TestResult<WorkerSummary> {
        // The Controller administrator credential belongs only to the isolated fixture.
        Ok(self
            .client
            .post(format!("{}/api/workers", self.base_url))
            .bearer_auth("task-20-test-only-password")
            .json(
                &serde_json::json!({"name":"protected", "api_url":worker.base_url(),
                "enabled":true, "compute_slots":1, "password":password}),
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn replace_worker_password(&self, id: WorkerId, password: &str) -> TestResult {
        let worker = self.store.worker(id).await?.ok_or("worker missing")?;
        self.client
            .put(format!("{}/api/workers/{id}", self.base_url))
            .bearer_auth("task-20-test-only-password")
            .json(
                &serde_json::json!({"version":worker.version, "name":worker.name,
                "api_url":worker.api_url, "enabled":true, "compute_slots":1, "password":password}),
            )
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
