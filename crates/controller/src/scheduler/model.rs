use crate::domain::{AttemptId, SubmissionKey, TaskId, WorkerId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentClass {
    IdleFeed,
    Prefetch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduledAssignment {
    task_id: TaskId,
    worker_id: WorkerId,
    attempt_id: AttemptId,
    submission_key: SubmissionKey,
    class: AssignmentClass,
}

impl ScheduledAssignment {
    pub(crate) const fn new(
        task_id: TaskId,
        worker_id: WorkerId,
        attempt_id: AttemptId,
        submission_key: SubmissionKey,
        class: AssignmentClass,
    ) -> Self {
        Self {
            task_id,
            worker_id,
            attempt_id,
            submission_key,
            class,
        }
    }

    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn worker_id(self) -> WorkerId {
        self.worker_id
    }

    #[must_use]
    pub const fn attempt_id(self) -> AttemptId {
        self.attempt_id
    }

    #[must_use]
    pub const fn submission_key(self) -> SubmissionKey {
        self.submission_key
    }

    #[must_use]
    pub const fn class(self) -> AssignmentClass {
        self.class
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UploadPriority {
    IdleFeed,
    Prefetch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadCandidate {
    task_id: TaskId,
    worker_id: WorkerId,
    priority: UploadPriority,
}

impl UploadCandidate {
    pub(crate) const fn new(
        task_id: TaskId,
        worker_id: WorkerId,
        priority: UploadPriority,
    ) -> Self {
        Self {
            task_id,
            worker_id,
            priority,
        }
    }

    #[must_use]
    pub const fn task_id(self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn worker_id(self) -> WorkerId {
        self.worker_id
    }

    #[must_use]
    pub const fn priority(self) -> UploadPriority {
        self.priority
    }
}
