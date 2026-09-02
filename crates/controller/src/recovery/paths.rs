use std::collections::BTreeMap;

use serde_json::Value;

use crate::persistence::TaskRecord;
use crate::remote::FileApiPath;

use super::RecoveryError;

pub(super) fn input_path(task: &TaskRecord) -> Result<FileApiPath, RecoveryError> {
    FileApiPath::parse(&format!(
        "{}/input.{}",
        task.id,
        task.input_extension.as_str()
    ))
    .map_err(Into::into)
}

pub(super) fn output_path(task: &TaskRecord) -> Result<FileApiPath, RecoveryError> {
    FileApiPath::parse(&format!(
        "{}/output.{}",
        task.id,
        task.output_extension.as_str()
    ))
    .map_err(Into::into)
}

pub(super) fn submission_params(
    input: &FileApiPath,
    output: &FileApiPath,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("input".to_owned(), Value::String(input.as_str().to_owned())),
        (
            "output".to_owned(),
            Value::String(output.as_str().to_owned()),
        ),
    ])
}
