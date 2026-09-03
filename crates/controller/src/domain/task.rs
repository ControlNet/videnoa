use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use super::{
    AttemptId, FailureCode, FailureStage, InputExtension, InputPath, OutputExtension, OutputPath,
    PageRequest, RemoteJobId, RemotePath, SortDirection, SourceReference, SubmissionKey, TaskId,
    TaskProgress, TaskSortField, TaskSource, TaskStatus, WorkerId, WorkflowName,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCreateRequest {
    pub input_path: InputPath,
    pub output_path: OutputPath,
    pub workflow: WorkflowName,
    pub priority: i32,
    pub source: TaskSource,
    pub source_reference: Option<SourceReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FailureInfo {
    pub failure_stage: FailureStage,
    pub failure_code: FailureCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryMetadata {
    pub retry_count: u32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAttempt {
    pub id: AttemptId,
    pub task_id: TaskId,
    pub attempt_number: u32,
    pub worker_id: Option<WorkerId>,
    pub status: TaskStatus,
    pub submission_key: SubmissionKey,
    pub remote_job_id: Option<RemoteJobId>,
    pub remote_input_path: Option<RemotePath>,
    pub remote_output_path: Option<RemotePath>,
    pub progress: TaskProgress,
    pub retry: RetryMetadata,
    pub failure: Option<FailureInfo>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: TaskId,
    pub version: u64,
    pub status: TaskStatus,
    pub input_path: InputPath,
    pub output_path: OutputPath,
    pub input_extension: InputExtension,
    pub output_extension: OutputExtension,
    pub workflow: WorkflowName,
    pub priority: i32,
    pub source: TaskSource,
    pub source_reference: Option<SourceReference>,
    pub input_size: u64,
    pub worker_id: Option<WorkerId>,
    pub remote_job_id: Option<RemoteJobId>,
    pub progress: TaskProgress,
    pub attempt_count: u32,
    pub failure: Option<FailureInfo>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub type TaskSummary = Task;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDetailResponse {
    pub task: Task,
    pub attempts: Vec<TaskAttempt>,
    pub total: u64,
    pub limit: u16,
    pub offset: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TaskListQuery {
    #[serde(flatten)]
    pub page: PageRequest,
    pub status: Option<TaskStatus>,
    pub worker_id: Option<WorkerId>,
    pub workflow: Option<WorkflowName>,
    pub source: Option<TaskSource>,
    pub failure_stage: Option<FailureStage>,
    pub search: Option<String>,
    pub sort: TaskSortField,
    pub direction: SortDirection,
}

impl<'de> Deserialize<'de> for TaskListQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTaskListQuery {
            #[serde(default)]
            limit: Option<u64>,
            #[serde(default)]
            offset: i64,
            status: Option<TaskStatus>,
            worker_id: Option<WorkerId>,
            workflow: Option<WorkflowName>,
            source: Option<TaskSource>,
            failure_stage: Option<FailureStage>,
            search: Option<String>,
            #[serde(default)]
            sort: TaskSortField,
            #[serde(default)]
            direction: SortDirection,
        }

        let raw = RawTaskListQuery::deserialize(deserializer)?;
        Ok(Self {
            page: PageRequest::try_new(raw.limit, raw.offset).map_err(serde::de::Error::custom)?,
            status: raw.status,
            worker_id: raw.worker_id,
            workflow: raw.workflow,
            source: raw.source,
            failure_stage: raw.failure_stage,
            search: raw.search,
            sort: raw.sort,
            direction: raw.direction,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskListResponse {
    pub items: Vec<TaskSummary>,
    pub total: u64,
    pub limit: u16,
    pub offset: u64,
}
