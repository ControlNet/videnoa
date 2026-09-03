use std::path::Path;

use super::TestResult;

pub(super) fn require_status(
    actual: reqwest::StatusCode,
    expected: reqwest::StatusCode,
    operation: &str,
) -> TestResult {
    if actual == expected {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{operation} returned HTTP {actual}, expected {expected}"
        ))
        .into())
    }
}

pub(super) fn path_string(path: &Path) -> TestResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("test path is not UTF-8").into())
}
