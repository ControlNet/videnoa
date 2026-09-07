use std::collections::BTreeMap;

use serde_json::Value;

use crate::domain::RemotePath;
use crate::persistence::AttemptRecord;

use super::RecoveryError;

pub(super) fn submission_params(
    input: &RemotePath,
    output: &RemotePath,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("input".to_owned(), Value::String(input.as_str().to_owned())),
        (
            "output".to_owned(),
            Value::String(output.as_str().to_owned()),
        ),
    ])
}

pub(super) fn remote_paths(
    attempt: &AttemptRecord,
) -> Result<(&RemotePath, &RemotePath), RecoveryError> {
    let input = attempt
        .attempt
        .remote_input_path
        .as_ref()
        .ok_or(RecoveryError::MissingRemoteEvidence)?;
    let output = attempt
        .attempt
        .remote_output_path
        .as_ref()
        .ok_or(RecoveryError::MissingRemoteEvidence)?;
    Ok((input, output))
}
