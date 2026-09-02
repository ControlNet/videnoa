use std::collections::BTreeMap;

use reqwest::StatusCode;
use serde_json::Value;

use crate::domain::{RemoteJobId, SubmissionKey, WorkflowName};

use super::dto::RunRequest;
use super::transport::ensure_success;
use super::{Job, RunOutcome, RunReceipt, RunSubmission, VidenoaClient, VidenoaClientError};

impl VidenoaClient {
    /// Submits a saved workflow with a durable idempotency key.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn run(
        &self,
        workflow: &WorkflowName,
        key: SubmissionKey,
        params: &BTreeMap<String, Value>,
    ) -> Result<RunSubmission, VidenoaClientError> {
        let response = self
            .send(
                self.http
                    .post(self.endpoint(&["api", "run"])?)
                    .header("idempotency-key", key.to_string())
                    .json(&RunRequest {
                        workflow_name: workflow,
                        params,
                    }),
            )
            .await?;
        let outcome = match response.status() {
            StatusCode::CREATED => RunOutcome::Created,
            StatusCode::OK => RunOutcome::Replayed,
            status => {
                ensure_success(status)?;
                return Err(VidenoaClientError::MalformedPayload);
            }
        };
        let receipt: RunReceipt = self.json(response).await?;
        Ok(RunSubmission { outcome, receipt })
    }

    /// Polls one remote job by typed identifier.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn job(&self, id: RemoteJobId) -> Result<Job, VidenoaClientError> {
        let id = id.to_string();
        let response = self
            .send(self.http.get(self.endpoint(&["api", "jobs", &id])?))
            .await?;
        self.json(response).await
    }

    /// Cancels and removes one remote job.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport or typed status failures.
    pub async fn cancel_job(&self, id: RemoteJobId) -> Result<(), VidenoaClientError> {
        let id = id.to_string();
        let response = self
            .send(self.http.delete(self.endpoint(&["api", "jobs", &id])?))
            .await?;
        ensure_success(response.status())
    }
}
