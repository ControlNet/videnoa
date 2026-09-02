use std::collections::BTreeMap;

use serde_json::Value;

use crate::domain::RemotePath;
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
