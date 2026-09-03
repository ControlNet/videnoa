use std::path::PathBuf;
use std::time::Duration;

use crate::domain::TaskId;
use crate::remote::{PayloadLimits, RemoteTimeouts};

use super::RecoveryCommandKind;

#[derive(Clone, Debug)]
pub struct RecoveryConfig {
    pub(crate) temp_root: PathBuf,
    pub(crate) timeouts: RemoteTimeouts,
    pub(crate) limits: PayloadLimits,
    pub(crate) health_initial: Duration,
    pub(crate) health_maximum: Duration,
    pub(crate) health_max_attempts: u32,
}

impl RecoveryConfig {
    #[must_use]
    pub fn new(
        temp_root: PathBuf,
        timeouts: RemoteTimeouts,
        limits: PayloadLimits,
        health_initial: Duration,
        health_maximum: Duration,
        health_max_attempts: u32,
    ) -> Self {
        Self {
            temp_root,
            timeouts,
            limits,
            health_initial,
            health_maximum,
            health_max_attempts,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryTrace {
    pub task_id: TaskId,
    pub command: RecoveryCommandKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeferredRecovery {
    pub task_id: TaskId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    traces: Vec<RecoveryTrace>,
    deferred: Vec<DeferredRecovery>,
}

impl RecoveryReport {
    pub(crate) fn push(&mut self, task_id: TaskId, command: RecoveryCommandKind) {
        self.traces.push(RecoveryTrace { task_id, command });
    }

    pub(crate) fn defer(&mut self, task_id: TaskId) {
        self.deferred.push(DeferredRecovery { task_id });
    }

    #[must_use]
    pub fn traces(&self) -> &[RecoveryTrace] {
        &self.traces
    }

    #[must_use]
    pub fn deferred(&self) -> &[DeferredRecovery] {
        &self.deferred
    }

    #[must_use]
    pub fn command_kind(&self, task_id: TaskId) -> Option<RecoveryCommandKind> {
        self.traces
            .iter()
            .find(|trace| trace.task_id == task_id)
            .map(|trace| trace.command)
    }
}
