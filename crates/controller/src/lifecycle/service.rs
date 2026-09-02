use chrono::{DateTime, Utc};

use crate::domain::TaskStatus;
use crate::persistence::{AttemptRecord, CasOutcome, ReservationOutcome, Store, TaskRecord};

use super::{
    AdvanceCommand, AttemptCas, CommandKind, CommittedCommand, DurableAction, FailureWrite,
    Lifecycle, LifecycleError, LifecycleFailure, PairedTransition, ReserveCommand,
    TransitionTarget,
};

#[derive(Clone, Debug)]
pub struct LifecycleService {
    store: Store,
}

impl LifecycleService {
    #[must_use]
    pub const fn new(store: Store) -> Self {
        Self { store }
    }

    pub(crate) const fn store(&self) -> &Store {
        &self.store
    }

    /// Atomically reserves a queued task and creates its durable attempt.
    ///
    /// # Errors
    /// Returns a conflict when task, worker capacity, or version preconditions changed.
    pub async fn reserve(
        &self,
        command: &ReserveCommand,
    ) -> Result<CommittedCommand, LifecycleError> {
        Lifecycle::destination(TaskStatus::Queued, CommandKind::Reserve)?;
        match self.store.reserve_task(command).await? {
            ReservationOutcome::Reserved(_) => Ok(CommittedCommand::new(
                TaskStatus::Reserved,
                command.expected_task_version + 1,
                DurableAction::None,
            )),
            ReservationOutcome::Conflict => Err(LifecycleError::Conflict),
        }
    }

    /// Commits one normal task-and-attempt transition before exposing its side effect.
    ///
    /// # Errors
    /// Returns a typed policy, snapshot, persistence, or CAS error.
    pub async fn advance(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        command: AdvanceCommand,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        if task.cancel_requested_at.is_some() {
            return Err(LifecycleError::Conflict);
        }
        let target = Lifecycle::destination(task.status, command.kind())?;
        let TransitionTarget::Status(next_status) = target else {
            return Err(LifecycleError::IllegalCommand);
        };
        let write = PairedTransition {
            task_id: task.id,
            task_version: task.version,
            from: task.status,
            to: next_status,
            attempt: attempt_cas(task, attempt)?,
            occurred_at,
            submission: command.submission().cloned(),
        };
        let action = command.action();
        let version = applied(self.store.apply_lifecycle_transition(&write).await?)?;
        Ok(CommittedCommand::new(next_status, version, action))
    }

    /// Closes the current lifecycle state and attempt with a typed failure.
    ///
    /// # Errors
    /// Returns an error when the failure does not belong to the current state or CAS conflicts.
    pub async fn fail(
        &self,
        task: &TaskRecord,
        attempt: Option<&AttemptRecord>,
        failure: LifecycleFailure,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        if task.status != failure.expected_status() || task.cancel_requested_at.is_some() {
            return Err(LifecycleError::IllegalCommand);
        }
        Lifecycle::destination(task.status, CommandKind::Fail)?;
        let attempt = match attempt {
            Some(attempt) => Some(attempt_cas(task, attempt)?),
            None if task.status == TaskStatus::Queued => None,
            None => return Err(LifecycleError::AttemptRequired),
        };
        let write = FailureWrite {
            task_id: task.id,
            task_version: task.version,
            from: task.status,
            attempt,
            failure: failure.info(),
            occurred_at,
        };
        let version = applied(self.store.fail_lifecycle(&write).await?)?;
        Ok(CommittedCommand::new(
            TaskStatus::Failed,
            version,
            DurableAction::None,
        ))
    }

    /// Closes malformed durable recovery state without requiring a usable attempt snapshot.
    ///
    /// # Errors
    /// Returns an error when the failure does not belong to the current state or CAS conflicts.
    pub async fn fail_recovery(
        &self,
        task: &TaskRecord,
        attempt: Option<&AttemptRecord>,
        failure: LifecycleFailure,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        if task.status != failure.expected_status() {
            return Err(LifecycleError::IllegalCommand);
        }
        let attempt = attempt.map(|value| attempt_cas(task, value)).transpose()?;
        let write = FailureWrite {
            task_id: task.id,
            task_version: task.version,
            from: task.status,
            attempt,
            failure: failure.info(),
            occurred_at,
        };
        let version = applied(self.store.fail_lifecycle(&write).await?)?;
        Ok(CommittedCommand::new(
            TaskStatus::Failed,
            version,
            DurableAction::None,
        ))
    }
}

pub(super) fn attempt_cas(
    task: &TaskRecord,
    attempt: &AttemptRecord,
) -> Result<AttemptCas, LifecycleError> {
    if attempt.attempt.task_id != task.id || attempt.attempt.status != task.status {
        return Err(LifecycleError::AttemptMismatch);
    }
    Ok(AttemptCas {
        id: attempt.attempt.id,
        version: attempt.version,
        status: attempt.attempt.status,
    })
}

pub(super) const fn applied(outcome: CasOutcome) -> Result<u64, LifecycleError> {
    match outcome {
        CasOutcome::Applied { new_version } => Ok(new_version),
        CasOutcome::Conflict => Err(LifecycleError::Conflict),
    }
}
