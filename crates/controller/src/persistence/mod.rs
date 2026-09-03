mod attempt;
mod attempt_query;
mod codec;
mod database;
mod error;
mod idempotency;
mod lifecycle_evidence;
mod lifecycle_retry;
mod lifecycle_status;
mod lifecycle_transition;
mod models;
mod readiness;
mod reservation;
mod scheduler;
mod session;
mod settings;
mod task;
mod task_mutation;
mod task_query;
mod task_row;
mod transfer_retry;
mod worker;
mod worker_query;
mod worker_registry;

pub use database::{Database, DatabaseOptions};
pub use error::PersistenceError;
pub use models::{
    AttemptFailureUpdate, AttemptProgressUpdate, AttemptRecord, AttemptRemoteUpdate, AuthDigest,
    CasOutcome, IdempotencyRecord, InputContentIdentity, InputIdentity, NewSession, NewTask,
    NewWorker, PageResult, PublicationUpdate, Reservation, ReservationOutcome, SchedulerCandidate,
    SessionRecord, SettingsRecord, SettingsUpdate, Sha256Digest, TaskFailureUpdate,
    TaskIngressOutcome, TaskProgressUpdate, TaskRecord, TaskRetryUpdate, UploadCandidateRecord,
    WorkerDeleteOutcome, WorkerHealthUpdate, WorkerIdentityConflict, WorkerRecord, WorkerUpdate,
    WorkerUpdateOutcome,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum DurableChange {
    Task(TaskId),
    Worker(WorkerId),
    WorkerDeleted,
    Settings,
}

#[derive(Clone)]
pub(crate) struct ChangeObserver(Arc<dyn Fn(DurableChange) + Send + Sync>);

impl ChangeObserver {
    pub(crate) fn new(observer: impl Fn(DurableChange) + Send + Sync + 'static) -> Self {
        Self(Arc::new(observer))
    }

    fn notify(&self, change: DurableChange) {
        (self.0)(change);
    }
}

impl fmt::Debug for ChangeObserver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ChangeObserver")
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    database: Database,
    changes: Arc<OnceLock<ChangeObserver>>,
    submission_admission: Arc<tokio::sync::RwLock<()>>,
}

impl Store {
    #[must_use]
    pub fn new(database: Database) -> Self {
        Self {
            database,
            changes: Arc::new(OnceLock::new()),
            submission_admission: Arc::new(tokio::sync::RwLock::new(())),
        }
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    pub(crate) fn observe_changes(&self, observer: ChangeObserver) {
        let _ = self.changes.set(observer);
    }

    pub(crate) fn notify_change(&self, change: DurableChange) {
        if let Some(observer) = self.changes.get() {
            observer.notify(change);
        }
    }

    pub(crate) fn submission_admission(&self) -> Arc<tokio::sync::RwLock<()>> {
        Arc::clone(&self.submission_admission)
    }
}
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::domain::{TaskId, WorkerId};
