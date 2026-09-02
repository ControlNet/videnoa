use chrono::{DateTime, Utc};

use crate::domain::{IdempotencyKey, SessionId, TaskId};

use super::AuthDigest;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct NewSession {
    pub id: SessionId,
    pub token_digest: AuthDigest,
    pub csrf_digest: AuthDigest,
    pub password_hash_fingerprint: AuthDigest,
    pub absolute_expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub token_digest: AuthDigest,
    pub csrf_digest: AuthDigest,
    pub password_hash_fingerprint: AuthDigest,
    pub absolute_expires_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdempotencyRecord {
    pub key: IdempotencyKey,
    pub request_fingerprint: [u8; 32],
    pub task_id: TaskId,
    pub created_at: DateTime<Utc>,
}
