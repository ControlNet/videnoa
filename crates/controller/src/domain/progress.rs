use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProgress {
    pub percent: f32,
    pub processed_frames: Option<u64>,
    pub total_frames: Option<u64>,
    pub frames_per_second: Option<f32>,
    pub eta_seconds: Option<u64>,
    pub bytes_transferred: Option<u64>,
    pub bytes_total: Option<u64>,
}
