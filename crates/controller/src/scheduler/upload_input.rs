use cap_std::fs::File;
use chrono::{DateTime, Utc};

use crate::domain::FailureCode;
use crate::paths::PathCapabilities;
use crate::persistence::{InputContentIdentity, InputIdentity, TaskRecord};

// One full hash, then cheap revalidation and ownership transfer of that same descriptor.
pub(crate) fn open_verified(
    paths: &PathCapabilities,
    task: &TaskRecord,
) -> Result<File, FailureCode> {
    let rooted = paths
        .open_input(task.request.input_path.as_str())
        .map_err(|_| FailureCode::InputUnavailable)?;
    let snapshot = rooted.snapshot();
    if snapshot.length != task.input_size
        || task.input_identity != Some(InputIdentity::new(snapshot.platform_identity()))
        || task.input_content_identity.is_some_and(|expected| {
            expected != InputContentIdentity::new(snapshot.content_identity())
        })
        || DateTime::<Utc>::from(snapshot.modified).timestamp_millis()
            != task.input_mtime.timestamp_millis()
    {
        return Err(FailureCode::InputChanged);
    }
    rooted
        .into_verified_file()
        .map_err(|_| FailureCode::InputChanged)
}
