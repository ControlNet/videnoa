use std::collections::BTreeMap;

use reqwest::StatusCode;
use serde_json::Value;

use crate::domain::{RemoteJobId, SubmissionKey, WorkflowName};

use videnoa_protocol::{ErrorCode, RunWorkflowRequest};

use super::transport::{ensure_success, Rejection};
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
                    .json(&RunWorkflowRequest {
                        workflow_name: Some(workflow.run_name().to_owned()),
                        params: Some(params.clone()),
                    }),
            )
            .await?;
        let outcome = match response.status() {
            StatusCode::CREATED => RunOutcome::Created,
            StatusCode::OK => RunOutcome::Replayed,
            status if status.is_client_error() || status.is_server_error() => {
                let rejection = self.rejection(response).await;
                return Err(run_rejection(status, rejection));
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
                    .json(&RunWorkflowRequest {
                        workflow_name: Some(workflow.run_name().to_owned()),
                        params: None,
                    }),
            )
            .await?;
        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(RunValidation::Valid);
        }
        if !(status.is_client_error() || status.is_server_error()) {
            ensure_success(status)?;
            return Err(VidenoaClientError::UnexpectedStatus {
                status: status.as_u16(),
            });
        }
        let rejection = self.rejection(response).await;
        match run_rejection(status, rejection) {
            VidenoaClientError::ClientStatus {
                status: 400 | 422,
                reason,
                ..
            }
            | VidenoaClientError::ClientStatus {
                reason,
                code: Some(_),
                ..
            } => Ok(RunValidation::Invalid {
                reason: reason.unwrap_or_else(|| "the worker rejected this workflow".to_owned()),
            }),
            // Workers that predate the route answer a bare 404.
            VidenoaClientError::NotFound => Ok(RunValidation::Unknown),
            error => Err(error),
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

/// Why a submission failed, when a Worker answers 422 to the run request
/// itself rather than to the workflow.
const INCOMPATIBLE_RUN_REQUEST: &str =
    "the worker cannot read this Controller's run request; update the worker";

/// Reads a rejected run by its code first, since the code says whether a
/// retry can help whatever the status. Without a known code the status
/// decides, as it does for protocol-1 Workers.
fn run_rejection(status: StatusCode, rejection: Rejection) -> VidenoaClientError {
    let Rejection { reason, code } = rejection;
    match code {
        Some(ErrorCode::QueueFull | ErrorCode::ShuttingDown) => VidenoaClientError::ServerStatus {
            status: status.as_u16(),
        },
        Some(ErrorCode::IdempotencyConflict) => VidenoaClientError::Conflict,
        Some(
            code @ (ErrorCode::InvalidIdempotencyKey
            | ErrorCode::WorkflowNameInvalid
            | ErrorCode::WorkflowNotFound
            | ErrorCode::WorkflowUnreadable
            | ErrorCode::WorkflowInvalid
            | ErrorCode::JobStoreUnavailable),
        ) => VidenoaClientError::ClientStatus {
            status: status.as_u16(),
            reason,
            code: Some(code),
        },
        _ => match status {
            // The Worker validates the workflow on submission; keep its
            // reason so the task failure says what to fix.
            StatusCode::BAD_REQUEST => VidenoaClientError::ClientStatus {
                status: status.as_u16(),
                reason,
                code,
            },
            StatusCode::UNPROCESSABLE_ENTITY => VidenoaClientError::ClientStatus {
                status: status.as_u16(),
                reason: Some(INCOMPATIBLE_RUN_REQUEST.to_owned()),
                code: None,
            },
            status => match ensure_success(status) {
                Err(error) => error,
                Ok(()) => VidenoaClientError::UnexpectedStatus {
                    status: status.as_u16(),
                },
            },
        },
    }
}
