use std::collections::BTreeMap;

use reqwest::StatusCode;
use serde_json::Value;

use crate::domain::{RemoteJobId, SubmissionKey, WorkflowName};

use super::dto::{RunRequest, ValidateRunRequest};
use super::transport::ensure_success;
use super::{
    Job, RunOutcome, RunReceipt, RunSubmission, RunValidation, VidenoaClient, VidenoaClientError,
};

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
            .send_authenticated(
                self.http
                    .post(self.endpoint(&["api", "run"])?)
                    .header("idempotency-key", key.to_string())
                    .json(&RunRequest {
                        workflow_name: workflow.run_name(),
                        params,
                    }),
            )
            .await?;
        let outcome = match response.status() {
            StatusCode::CREATED => RunOutcome::Created,
            StatusCode::OK => RunOutcome::Replayed,
            StatusCode::BAD_REQUEST => {
                // The Worker validates the workflow on submission; keep its
                // reason so the task failure says what to fix.
                return Err(VidenoaClientError::ClientStatus {
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    reason: self.rejection_reason(response).await,
                });
            }
            status => {
                ensure_success(status)?;
                return Err(VidenoaClientError::MalformedPayload);
            }
        };
        let receipt: RunReceipt = self.json(response).await?;
        Ok(RunSubmission { outcome, receipt })
    }

    /// Asks the Worker whether `run` would accept this workflow, without
    /// creating a job.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport or unexpected status failures.
    pub async fn validate_run(
        &self,
        workflow: &WorkflowName,
    ) -> Result<RunValidation, VidenoaClientError> {
        let response = self
            .send_authenticated(
                self.http
                    .post(self.endpoint(&["api", "run", "validate"])?)
                    .json(&ValidateRunRequest {
                        workflow_name: workflow.run_name(),
                    }),
            )
            .await?;
        match response.status() {
            StatusCode::NO_CONTENT => Ok(RunValidation::Valid),
            StatusCode::BAD_REQUEST => Ok(RunValidation::Invalid {
                reason: self
                    .rejection_reason(response)
                    .await
                    .unwrap_or_else(|| "the worker rejected this workflow".to_owned()),
            }),
            StatusCode::NOT_FOUND => Ok(RunValidation::Unknown),
            status => {
                ensure_success(status)?;
                Err(VidenoaClientError::UnexpectedStatus {
                    status: status.as_u16(),
                })
            }
        }
    }

    /// Polls one remote job by typed identifier.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn job(&self, id: RemoteJobId) -> Result<Job, VidenoaClientError> {
        let id = id.to_string();
        let response = self
            .send_authenticated(self.http.get(self.endpoint(&["api", "jobs", &id])?))
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
            .send_authenticated(self.http.delete(self.endpoint(&["api", "jobs", &id])?))
            .await?;
        ensure_success(response.status())
    }
}
