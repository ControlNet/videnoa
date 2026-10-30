use serde::{Deserialize, Serialize};

use crate::LEGACY_PROTOCOL_VERSION;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
    /// A status this build does not know, sent by a newer Worker.
    #[serde(other)]
    Unknown,
}

/// `GET /api/health`. Workers up to v0.1.10 send only `status`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    /// Jobs or previews that can hold the GPU at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_slots: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<usize>,
    /// Queued or running jobs that were asked to stop and have not yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelling: Option<usize>,
    /// The Worker's [`crate::PROTOCOL_VERSION`]; see [`Self::protocol`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
}

impl HealthResponse {
    /// The protocol the Worker speaks; a Worker that does not say speaks
    /// [`LEGACY_PROTOCOL_VERSION`].
    #[must_use]
    pub fn protocol(&self) -> u32 {
        self.protocol.unwrap_or(LEGACY_PROTOCOL_VERSION)
    }
}
