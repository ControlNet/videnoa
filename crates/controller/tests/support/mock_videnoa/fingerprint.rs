use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn run_fingerprint(
    workflow_name: &str,
    params: Option<&BTreeMap<String, Value>>,
) -> Result<String, serde_json::Error> {
    let mut canonical = String::from("{\"params\":");
    match params {
        Some(values) => write_object(values.iter(), &mut canonical)?,
        None => canonical.push_str("null"),
    }
    canonical.push_str(",\"workflow_name\":");
    canonical.push_str(&serde_json::to_string(workflow_name)?);
    canonical.push('}');
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
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
        Value::Object(values) => write_object(values.iter(), output)?,
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

fn write_object<'a>(
    values: impl Iterator<Item = (&'a String, &'a Value)>,
    output: &mut String,
) -> Result<(), serde_json::Error> {
    output.push('{');
    for (index, (key, value)) in values.enumerate() {
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
