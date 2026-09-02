mod attempt;
mod codec;
mod database;
mod error;
mod idempotency;
mod lifecycle_evidence;
mod lifecycle_retry;
mod lifecycle_transition;
mod models;
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
mod worker_registry;

pub use database::{Database, DatabaseOptions};
pub use error::PersistenceError;
pub use models::{
    AttemptFailureUpdate, AttemptProgressUpdate, AttemptRecord, AttemptRemoteUpdate, AuthDigest,
    CasOutcome, IdempotencyRecord, InputIdentity, NewSession, NewTask, NewWorker, PageResult,
    PublicationUpdate, Reservation, ReservationOutcome, SchedulerCandidate, SessionRecord,
    SettingsRecord, SettingsUpdate, Sha256Digest, TaskFailureUpdate, TaskIngressOutcome,
    TaskProgressUpdate, TaskRecord, TaskRetryUpdate, UploadCandidateRecord, WorkerDeleteOutcome,
    WorkerHealthUpdate, WorkerIdentityConflict, WorkerRecord, WorkerUpdate, WorkerUpdateOutcome,
};

#[derive(Clone, Debug)]
pub struct Store {
    database: Database,
}

impl Store {
    #[must_use]
    pub const fn new(database: Database) -> Self {
        Self { database }
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }
}
