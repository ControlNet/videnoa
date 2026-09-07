use std::path::{Path, PathBuf};

use videnoa_controller::domain::TaskId;
use videnoa_controller::lifecycle::JitterSample;

use super::TestResult;

pub fn zero_jitter() -> TestResult<JitterSample> {
    Ok(JitterSample::try_from(0)?)
}

pub fn verified_path(root: &Path, task_id: TaskId) -> PathBuf {
    root.join(task_id.to_string()).join("output.mp4.verified")
}

pub fn part_path(root: &Path, task_id: TaskId) -> PathBuf {
    root.join(task_id.to_string()).join("output.mp4.part")
}

pub fn evidence_path(root: &Path, task_id: TaskId) -> PathBuf {
    root.join(task_id.to_string())
        .join("output.mp4.verified.evidence")
}
