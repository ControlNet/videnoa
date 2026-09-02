use std::path::Path;

use chrono::{DateTime, Utc};

use crate::domain::{
    FieldErrorCode, IdempotencyKey, InputExtension, OutputExtension, SseEvent, SseEventId, Task,
    TaskCreateRequest, TaskId,
};
use crate::operations::EventHub;
use crate::paths::{PathCapabilities, PathError};
use crate::persistence::{IdempotencyRecord, InputIdentity, NewTask, Store, TaskIngressOutcome};

use super::error::TaskApiError;
use super::{fingerprint, mapping};

const PRIORITY_MIN: i32 = -100;
const PRIORITY_MAX: i32 = 100;
const WORKFLOW_MAX_BYTES: usize = 128;
const SOURCE_REFERENCE_MAX_BYTES: usize = 512;

#[derive(Clone)]
pub struct TaskService {
    store: Store,
    paths: PathCapabilities,
    events: EventHub,
}

pub(crate) enum IntakeOutcome {
    Created(Task),
    Replayed(Task),
}

impl TaskService {
    #[must_use]
    pub fn new(store: Store, paths: PathCapabilities) -> Self {
        Self {
            store,
            paths,
            events: EventHub::new(),
        }
    }

    #[must_use]
    pub fn with_events(store: Store, paths: PathCapabilities, events: EventHub) -> Self {
        Self {
            store,
            paths,
            events,
        }
    }

    #[must_use]
    pub fn with_event_hub(mut self, events: EventHub) -> Self {
        self.events = events;
        self
    }

    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    pub(crate) async fn create(
        &self,
        key: IdempotencyKey,
        request: TaskCreateRequest,
    ) -> Result<IntakeOutcome, TaskApiError> {
        let request_fingerprint =
            fingerprint::fingerprint(&request).map_err(|_| TaskApiError::Internal)?;
        if let Some(outcome) = self.preflight(&key, request_fingerprint).await? {
            return Ok(outcome);
        }
        validate_request(&request)?;
        let input_extension = extension(request.input_path.as_str(), "input_path")?;
        let output_extension = extension(request.output_path.as_str(), "output_path")?;
        let input = self
            .paths
            .open_input(request.input_path.as_str())
            .map_err(|error| path_error("input_path", &error))?;
        let output = self
            .paths
            .open_output(request.output_path.as_str())
            .map_err(|error| path_error("output_path", &error))?;
        input
            .reopen_checked()
            .map_err(|error| path_error("input_path", &error))?;
        output
            .revalidate_missing()
            .map_err(|error| path_error("output_path", &error))?;
        let now = Utc::now();
        let task_id = TaskId::random();
        let task = NewTask {
            id: task_id,
            request,
            input_extension: InputExtension::new(input_extension),
            output_extension: OutputExtension::new(output_extension),
            input_size: input.snapshot().length,
            input_mtime: DateTime::<Utc>::from(input.snapshot().modified),
            input_identity: InputIdentity::new(input.snapshot().platform_identity()),
            created_at: now,
        };
        let record = IdempotencyRecord {
            key,
            request_fingerprint,
            task_id,
            created_at: now,
        };
        match self
            .store
            .insert_task_with_idempotency(&task, &record)
            .await
            .map_err(|_| TaskApiError::Internal)?
        {
            TaskIngressOutcome::Inserted => {
                let task = self.load(task_id).await?;
                self.events.publish(SseEvent::TaskUpdated {
                    event_id: SseEventId::random(),
                    task: task.clone(),
                });
                Ok(IntakeOutcome::Created(task))
            }
            TaskIngressOutcome::Replay(existing) => {
                Ok(IntakeOutcome::Replayed(self.load(existing).await?))
            }
            TaskIngressOutcome::Conflict => Err(TaskApiError::Conflict),
        }
    }

    async fn preflight(
        &self,
        key: &IdempotencyKey,
        request_fingerprint: [u8; 32],
    ) -> Result<Option<IntakeOutcome>, TaskApiError> {
        let Some(record) = self
            .store
            .task_idempotency(key)
            .await
            .map_err(|_| TaskApiError::Internal)?
        else {
            return Ok(None);
        };
        if record.request_fingerprint == request_fingerprint {
            return self
                .load(record.task_id)
                .await
                .map(IntakeOutcome::Replayed)
                .map(Some);
        }
        Err(TaskApiError::Conflict)
    }

    async fn load(&self, id: TaskId) -> Result<Task, TaskApiError> {
        self.store
            .task(id)
            .await
            .map_err(|_| TaskApiError::Internal)?
            .map(mapping::task)
            .ok_or(TaskApiError::Internal)
    }
}

fn validate_request(request: &TaskCreateRequest) -> Result<(), TaskApiError> {
    if !(PRIORITY_MIN..=PRIORITY_MAX).contains(&request.priority) {
        return Err(TaskApiError::invalid(
            "priority",
            FieldErrorCode::OutOfRange,
            "priority must be between -100 and 100",
        ));
    }
    bounded_string(request.workflow.as_str(), "workflow", WORKFLOW_MAX_BYTES)?;
    if let Some(reference) = &request.source_reference {
        bounded_string(
            reference.as_str(),
            "source_reference",
            SOURCE_REFERENCE_MAX_BYTES,
        )?;
    }
    Ok(())
}

fn bounded_string(value: &str, field: &'static str, maximum: usize) -> Result<(), TaskApiError> {
    if value.is_empty() {
        return Err(TaskApiError::invalid(
            field,
            FieldErrorCode::Required,
            "value must not be empty",
        ));
    }
    if value.len() > maximum {
        return Err(TaskApiError::invalid(
            field,
            FieldErrorCode::OutOfRange,
            "value is too long",
        ));
    }
    Ok(())
}

fn extension(path: &str, field: &'static str) -> Result<String, TaskApiError> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            TaskApiError::invalid(
                field,
                FieldErrorCode::InvalidValue,
                "final filename must have an extension",
            )
        })
}

fn path_error(field: &'static str, error: &PathError) -> TaskApiError {
    let message = match error {
        PathError::OutputExists { .. } => "output must not already exist",
        PathError::InputNotRegular { .. } => "input must be an existing regular file",
        PathError::InputChanged { .. } => "input changed during task intake",
        PathError::OutsideRoots { .. }
        | PathError::InvalidPath { .. }
        | PathError::SymlinkComponent { .. }
        | PathError::RootChanged { .. }
        | PathError::OutputParentChanged { .. }
        | PathError::Io { .. } => "path is not available through configured roots",
    };
    TaskApiError::invalid(field, FieldErrorCode::InvalidValue, message)
}
