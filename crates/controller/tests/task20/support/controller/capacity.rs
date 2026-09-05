use std::time::Duration;

use videnoa_controller::domain::{ComputeSlots, WorkerCreateRequest, WorkerSummary};

use crate::mock_videnoa::server::MockVidenoa;

use super::super::http::require_status;
use super::{ControllerFixture, TestResult, PASSWORD};

impl ControllerFixture {
    pub async fn register_worker_with_slots(
        &self,
        server: &MockVidenoa,
        name: &str,
        slots: u64,
    ) -> TestResult<WorkerSummary> {
        let worker = self
            .register_worker_without_wait_with_slots(server, name, true, slots)
            .await?;
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
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| std::io::Error::other("worker did not become online"))??;
        Ok(worker)
    }

    pub(super) async fn register_worker_without_wait_with_slots(
        &self,
        server: &MockVidenoa,
        name: &str,
        enabled: bool,
        slots: u64,
    ) -> TestResult<WorkerSummary> {
        let response = self
            .client
            .post(format!("{}/api/workers", self.base_url))
            .bearer_auth(PASSWORD)
            .json(&WorkerCreateRequest {
                name: videnoa_controller::domain::WorkerName::new(name),
                api_url: videnoa_controller::domain::WorkerApiUrl::parse(server.base_url())?,
                enabled,
                compute_slots: ComputeSlots::try_from(slots)?,
            })
            .send()
            .await?;
        require_status(
            response.status(),
            reqwest::StatusCode::CREATED,
            "create worker",
        )?;
        Ok(response.json::<WorkerSummary>().await?)
    }

    pub async fn workers(&self) -> TestResult<Vec<WorkerSummary>> {
        let response = self
            .client
            .get(format!("{}/api/workers", self.base_url))
            .bearer_auth(PASSWORD)
            .send()
            .await?;
        require_status(response.status(), reqwest::StatusCode::OK, "list workers")?;
        Ok(response
            .json::<videnoa_controller::domain::WorkerListResponse>()
            .await?
            .items)
    }
}
