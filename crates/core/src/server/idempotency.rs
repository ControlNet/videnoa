use std::collections::HashMap;

use axum::http::HeaderMap;
use serde_json::Value;
use sha2::{Digest, Sha256};

const HEADER_NAME: &str = "idempotency-key";
const MAX_KEY_BYTES: usize = 255;

#[derive(Debug)]
pub(super) struct IdempotencyKey(Box<str>);

impl IdempotencyKey {
    pub(super) fn from_headers(headers: &HeaderMap) -> Result<Option<Self>, InvalidKey> {
        let mut values = headers.get_all(HEADER_NAME).iter();
        let Some(value) = values.next() else {
            return Ok(None);
        };
        if values.next().is_some() {
            return Err(InvalidKey);
        }
        let raw = value.to_str().map_err(|_| InvalidKey)?;
        let bytes = raw.as_bytes();
        if bytes.is_empty()
            || bytes.len() > MAX_KEY_BYTES
            || !bytes.iter().all(u8::is_ascii_graphic)
        {
            return Err(InvalidKey);
        }
        Ok(Some(Self(raw.into())))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub(super) struct InvalidKey;

#[derive(Debug)]
pub(super) struct RequestFingerprint(String);

impl RequestFingerprint {
    pub(super) fn for_run(
        workflow_name: &str,
        params: Option<&HashMap<String, Value>>,
    ) -> serde_json::Result<Self> {
        let mut canonical = String::from("{\"params\":");
        match params {
            Some(params) => write_object(params.iter(), &mut canonical)?,
            None => canonical.push_str("null"),
        }
        canonical.push_str(",\"workflow_name\":");
        canonical.push_str(&serde_json::to_string(workflow_name)?);
        canonical.push('}');
        Ok(Self(format!("{:x}", Sha256::digest(canonical.as_bytes()))))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

fn write_value(value: &Value, output: &mut String) -> serde_json::Result<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => write_number(value, output)?,
        Value::String(value) => output.push_str(&serde_json::to_string(value)?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_value(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => write_object(values.iter(), output)?,
    }
    Ok(())
}

fn write_number(value: &serde_json::Number, output: &mut String) -> serde_json::Result<()> {
    if let Some(value) = value.as_i64() {
        output.push_str(&value.to_string());
    } else if let Some(value) = value.as_u64() {
        output.push_str(&value.to_string());
    } else if let Some(value) = value.as_f64() {
        if value == 0.0 {
            output.push('0');
        } else {
            output.push_str(&value.to_string());
        }
    } else {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON number has no canonical representation",
        )));
    }
    Ok(())
}

fn write_object<'a>(
    values: impl Iterator<Item = (&'a String, &'a Value)>,
    output: &mut String,
) -> serde_json::Result<()> {
    let mut entries: Vec<_> = values.collect();
    entries.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    output.push('{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&serde_json::to_string(key)?);
        output.push(':');
        write_value(value, output)?;
    }
    output.push('}');
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_preserves_array_order_and_scalar_types() {
        let first = HashMap::from([("value".to_string(), serde_json::json!([1, "1", null]))]);
        let reordered = HashMap::from([("value".to_string(), serde_json::json!(["1", 1, null]))]);

        let first = RequestFingerprint::for_run("workflow", Some(&first)).expect("fingerprint");
        let reordered =
            RequestFingerprint::for_run("workflow", Some(&reordered)).expect("fingerprint");

        assert_ne!(first.as_str(), reordered.as_str());
    }
}
