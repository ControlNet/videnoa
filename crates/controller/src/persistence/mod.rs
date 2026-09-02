mod attempt;
mod codec;
mod database;
mod error;
mod idempotency;
mod models;
mod reservation;
mod session;
mod settings;
mod task;
mod task_mutation;
mod task_query;
mod task_row;
mod worker;

pub use database::{Database, DatabaseOptions};
pub use error::PersistenceError;
pub use models::{
    AttemptFailureUpdate, AttemptProgressUpdate, AttemptRecord, AttemptRemoteUpdate,
    AttemptTransition, AuthDigest, CasOutcome, IdempotencyRecord, NewSession, NewTask, NewWorker,
    PageResult, PublicationUpdate, Reservation, ReservationOutcome, SessionRecord, SettingsRecord,
    SettingsUpdate, Sha256Digest, TaskFailureUpdate, TaskIngressOutcome, TaskProgressUpdate,
    TaskRecord, TaskRetryUpdate, TaskTransition, WorkerHealthUpdate, WorkerRecord, WorkerUpdate,
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
