use cap_std::fs::File;

use crate::domain::FailureCode;
use crate::paths::PathCapabilities;
use crate::persistence::TaskRecord;

pub(crate) fn open_current(
    paths: &PathCapabilities,
    task: &TaskRecord,
) -> Result<(File, u64), FailureCode> {
    paths
        .open_current_input(std::path::Path::new(task.request.input_path.as_str()))
        .map_err(|_| FailureCode::InputUnavailable)
}
