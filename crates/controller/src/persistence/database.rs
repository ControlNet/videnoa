use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use super::PersistenceError;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_CONNECTIONS: u32 = 8;

#[derive(Clone, Debug)]
pub struct DatabaseOptions {
    path: PathBuf,
    busy_timeout: Duration,
    max_connections: u32,
}

impl DatabaseOptions {
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
        }
    }

    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    #[must_use]
    pub const fn with_max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// # Errors
    /// Returns an error when `SQLite` cannot open or migrate the database.
    pub async fn open(options: DatabaseOptions) -> Result<Self, PersistenceError> {
        let connect = SqliteConnectOptions::new()
            .filename(options.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(options.busy_timeout);
        let pool = SqlitePoolOptions::new()
            .max_connections(options.max_connections)
            .acquire_timeout(options.busy_timeout)
            .connect_with(connect)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub const fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}
