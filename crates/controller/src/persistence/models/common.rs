use crate::domain::{AttemptId, TaskId};

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AuthDigest([u8; 32]);

impl AuthDigest {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputIdentity([u8; 16]);

impl InputIdentity {
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasOutcome {
    Applied { new_version: u64 },
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationOutcome {
    Reserved(AttemptId),
    Conflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskIngressOutcome {
    Inserted,
    Replay(TaskId),
    Conflict,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageResult<T> {
    pub items: Vec<T>,
    pub total: u64,
}
