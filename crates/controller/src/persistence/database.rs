use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use super::PersistenceError;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
// Pool acquisition waits for a free connection, not for a SQLite lock, so it is
// bounded independently of the busy timeout and tolerates loaded hosts.
const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_CONNECTIONS: u32 = 8;

#[derive(Clone, Debug)]
pub struct DatabaseOptions {
    path: PathBuf,
    busy_timeout: Duration,
    acquire_timeout: Duration,
    max_connections: u32,
}

impl DatabaseOptions {
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
        }
    }

    #[must_use]
    pub const fn with_busy_timeout(mut self, busy_timeout: Duration) -> Self {
        self.busy_timeout = busy_timeout;
        self
    }

    /// Bounds how long a caller waits for a free pooled connection.
    #[must_use]
    pub const fn with_acquire_timeout(mut self, acquire_timeout: Duration) -> Self {
        self.acquire_timeout = acquire_timeout;
        self
    }

    #[must_use]
    pub const fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }

    #[must_use]
    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
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
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut component = options.path.as_os_str().to_os_string();
            component.push(suffix);
            match std::fs::symlink_metadata(Path::new(&component)) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(sqlx::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "symbolic links are not allowed for SQLite files",
                    ))
                    .into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(sqlx::Error::Io(error).into()),
            }
        }
        let connect = SqliteConnectOptions::new()
            .filename(options.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(options.busy_timeout);
        let pool = SqlitePoolOptions::new()
            .max_connections(options.max_connections)
            .acquire_timeout(options.acquire_timeout)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_timeout_does_not_shorten_pool_acquisition() {
        let options = DatabaseOptions::new("controller.sqlite3")
            .with_busy_timeout(Duration::from_millis(100));
        assert_eq!(options.busy_timeout(), Duration::from_millis(100));
        assert_eq!(options.acquire_timeout(), DEFAULT_ACQUIRE_TIMEOUT);
        assert_eq!(DEFAULT_ACQUIRE_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn acquire_timeout_is_configured_independently() {
        let options = DatabaseOptions::new("controller.sqlite3")
            .with_acquire_timeout(Duration::from_millis(100));
        assert_eq!(options.acquire_timeout(), Duration::from_millis(100));
        assert_eq!(options.busy_timeout(), DEFAULT_BUSY_TIMEOUT);
    }
}
