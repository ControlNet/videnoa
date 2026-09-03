use crate::domain::{Task, TaskAttempt, TaskDetailResponse, TaskListResponse};
use crate::persistence::{AttemptRecord, PageResult, TaskRecord};

pub(crate) fn task(record: TaskRecord) -> Task {
    Task {
        id: record.id,
        version: record.version,
        status: record.status,
        input_path: record.request.input_path,
        output_path: record.request.output_path,
        input_extension: record.input_extension,
        output_extension: record.output_extension,
        workflow: record.request.workflow,
        priority: record.request.priority,
        source: record.request.source,
        source_reference: record.request.source_reference,
        input_size: record.input_size,
        worker_id: record.worker_id,
        remote_job_id: record.remote_job_id,
        progress: record.progress,
        attempt_count: record.attempt_count,
        failure: record.failure,
        cancel_requested_at: record.cancel_requested_at,
        created_at: record.created_at,
        updated_at: record.updated_at,
        completed_at: record.lifecycle.completed_at,
    }
}

pub(crate) fn detail(record: TaskRecord, attempts: Vec<AttemptRecord>) -> TaskDetailResponse {
    TaskDetailResponse {
        task: task(record),
        attempts: attempts
            .into_iter()
            .map(|record| record.attempt)
            .collect::<Vec<TaskAttempt>>(),
    }
}

pub(crate) fn list(page: PageResult<TaskRecord>, limit: u16, offset: u64) -> TaskListResponse {
    TaskListResponse {
        items: page.items.into_iter().map(task).collect(),
        total: page.total,
        limit,
        offset,
    }
}
