use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::TaskCreateRequest;

pub(crate) fn fingerprint(request: &TaskCreateRequest) -> Result<[u8; 32], serde_json::Error> {
    let value = serde_json::to_value(request)?;
    let mut canonical = String::new();
    write_value(&value, &mut canonical)?;
    Ok(Sha256::digest(canonical.as_bytes()).into())
}

fn write_value(value: &Value, output: &mut String) -> Result<(), serde_json::Error> {
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
        Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
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
        }
    }
    Ok(())
}

fn write_number(value: &serde_json::Number, output: &mut String) -> Result<(), serde_json::Error> {
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
