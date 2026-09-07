use crate::domain::{TaskId, WorkerId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulerCandidate {
    pub task_id: TaskId,
    pub task_version: u64,
    pub worker_id: WorkerId,
    pub idle_feed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadCandidateRecord {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub idle_feed: bool,
}
