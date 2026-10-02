use crate::domain::{FieldErrorCode, WorkflowName};
use crate::persistence::WorkerRecord;

use super::error::TaskApiError;
use super::TaskService;

impl TaskService {
    /// Rejects a workflow that every worker reporting it would refuse to run.
    ///
    /// A workflow no worker has reported yet is accepted and queues until a
    /// worker can run it.
    pub(super) async fn ensure_runnable_workflow(
        &self,
        workflow: &WorkflowName,
    ) -> Result<(), TaskApiError> {
        let workers = self
            .store()
            .workers()
            .await
            .map_err(|_| TaskApiError::Internal)?;
        if rejected_everywhere(&workers, workflow) {
            return Err(TaskApiError::invalid(
                "workflow",
                FieldErrorCode::InvalidValue,
                "Every worker with this workflow reports that it cannot run it. Fix the workflow on a worker, wait for its next health check, then retry.",
            ));
        }
        Ok(())
    }
}

fn rejected_everywhere(workers: &[WorkerRecord], workflow: &WorkflowName) -> bool {
    let runnable = workers.iter().any(|worker| {
        worker
            .capabilities
            .workflows
            .iter()
            .any(|summary| &summary.name == workflow)
    });
    let rejected = workers.iter().any(|worker| {
        worker
            .capabilities
            .invalid_workflows
            .iter()
            .any(|invalid| &invalid.name == workflow)
    });
    rejected && !runnable
}
