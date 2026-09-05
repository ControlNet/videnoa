use chrono::{DateTime, Utc};

use super::codec::timestamp;
use super::{PersistenceError, Store};

#[derive(Clone, Eq, PartialEq)]
pub struct PasswordHashRecord(String);

impl PasswordHashRecord {
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Store {
    /// Reads the singleton administrator credential when setup has completed.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot read the credential row.
    pub async fn administrator_credential(
        &self,
    ) -> Result<Option<PasswordHashRecord>, PersistenceError> {
        sqlx::query_scalar("SELECT password_hash FROM administrator_credential WHERE id = 1")
            .fetch_optional(self.database.pool())
            .await
            .map(|hash| hash.map(PasswordHashRecord))
            .map_err(PersistenceError::from)
    }

    /// Inserts the first administrator credential without replacing an existing row.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot execute the singleton insert.
    pub async fn insert_administrator_credential(
        &self,
        password_hash: &str,
        created_at: DateTime<Utc>,
    ) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO administrator_credential (id, password_hash, created_at_ms)
             VALUES (1, ?, ?)",
        )
        .bind(password_hash)
        .bind(timestamp(created_at))
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }
}
