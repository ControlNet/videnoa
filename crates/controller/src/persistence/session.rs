use sqlx::Row;

use crate::domain::SessionId;

use super::codec::{parse_brand, parse_optional_timestamp, parse_timestamp, timestamp};
use super::models::{AuthDigest, NewSession, SessionRecord};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` cannot persist the session digests.
    pub async fn insert_session(&self, session: &NewSession) -> Result<(), PersistenceError> {
        sqlx::query(
            "INSERT INTO auth_sessions (
                id, token_digest, csrf_digest, password_hash_fingerprint,
                absolute_expires_at_ms, idle_expires_at_ms, last_used_at_ms, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(session.token_digest.as_bytes().as_slice())
        .bind(session.csrf_digest.as_bytes().as_slice())
        .bind(session.password_hash_fingerprint.as_bytes().as_slice())
        .bind(timestamp(session.absolute_expires_at))
        .bind(timestamp(session.idle_expires_at))
        .bind(timestamp(session.created_at))
        .bind(timestamp(session.created_at))
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn session_by_token_digest(
        &self,
        digest: AuthDigest,
    ) -> Result<Option<SessionRecord>, PersistenceError> {
        sqlx::query(SESSION_SELECT)
            .bind(digest.as_bytes().as_slice())
            .fetch_optional(self.database.pool())
            .await?
            .as_ref()
            .map(map_session)
            .transpose()
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot update the session expiry.
    pub async fn touch_session(
        &self,
        id: SessionId,
        last_used_at: chrono::DateTime<chrono::Utc>,
        idle_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            "UPDATE auth_sessions SET last_used_at_ms = ?, idle_expires_at_ms = ?
             WHERE id = ? AND revoked_at_ms IS NULL AND absolute_expires_at_ms > ?",
        )
        .bind(timestamp(last_used_at))
        .bind(timestamp(idle_expires_at))
        .bind(id.to_string())
        .bind(timestamp(last_used_at))
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot revoke the session.
    pub async fn revoke_session(
        &self,
        id: SessionId,
        revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            "UPDATE auth_sessions SET revoked_at_ms = ? WHERE id = ? AND revoked_at_ms IS NULL",
        )
        .bind(timestamp(revoked_at))
        .bind(id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot rotate the session CSRF digest.
    pub async fn update_session_csrf(
        &self,
        id: SessionId,
        csrf_digest: AuthDigest,
    ) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            "UPDATE auth_sessions SET csrf_digest = ? WHERE id = ? AND revoked_at_ms IS NULL",
        )
        .bind(csrf_digest.as_bytes().as_slice())
        .bind(id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot purge expired sessions.
    pub async fn purge_expired_sessions(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, PersistenceError> {
        Ok(sqlx::query(
            "DELETE FROM auth_sessions
             WHERE revoked_at_ms IS NOT NULL OR absolute_expires_at_ms <= ? OR idle_expires_at_ms <= ?",
        )
        .bind(timestamp(now))
        .bind(timestamp(now))
        .execute(self.database.pool())
        .await?
        .rows_affected())
    }
}

const SESSION_SELECT: &str = "SELECT id, token_digest, csrf_digest, password_hash_fingerprint,
    absolute_expires_at_ms, idle_expires_at_ms, last_used_at_ms, created_at_ms, revoked_at_ms
    FROM auth_sessions WHERE token_digest = ?";

fn map_session(row: &sqlx::sqlite::SqliteRow) -> Result<SessionRecord, PersistenceError> {
    Ok(SessionRecord {
        id: parse_brand::<SessionId>("id", row.try_get("id")?)?,
        token_digest: digest(row, "token_digest")?,
        csrf_digest: digest(row, "csrf_digest")?,
        password_hash_fingerprint: digest(row, "password_hash_fingerprint")?,
        absolute_expires_at: parse_timestamp(
            "absolute_expires_at_ms",
            row.try_get("absolute_expires_at_ms")?,
        )?,
        idle_expires_at: parse_timestamp("idle_expires_at_ms", row.try_get("idle_expires_at_ms")?)?,
        last_used_at: parse_timestamp("last_used_at_ms", row.try_get("last_used_at_ms")?)?,
        created_at: parse_timestamp("created_at_ms", row.try_get("created_at_ms")?)?,
        revoked_at: parse_optional_timestamp("revoked_at_ms", row.try_get("revoked_at_ms")?)?,
    })
}

fn digest(
    row: &sqlx::sqlite::SqliteRow,
    field: &'static str,
) -> Result<AuthDigest, PersistenceError> {
    let bytes: Vec<u8> = row.try_get(field)?;
    <[u8; 32]>::try_from(bytes)
        .map(AuthDigest::new)
        .map_err(|bytes| super::codec::corrupt(field, bytes.len()))
}
