use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use thiserror::Error;
use whl_template::{NameValue, StepData};

pub const MAX_STEP_RESULTS_BYTES: usize = 64 * 1024;
pub const MAX_STEP_BODY_BYTES: usize = 16 * 1024;
pub const MAX_STEP_HEADERS: usize = 16;

#[derive(Debug, Error)]
#[error("the stored step results have the wrong shape")]
pub struct StepResultsError;

fn encode(steps: &BTreeMap<String, StepData>) -> Value {
    let mut map = Map::new();
    for (name, data) in steps {
        let headers: Vec<Value> = data
            .headers
            .iter()
            .take(MAX_STEP_HEADERS)
            .map(|header| json!({"name": header.name, "value": header.value}))
            .collect();
        map.insert(
            name.clone(),
            json!({
                "status": data.status,
                "headers": headers,
                "body": data.body,
                "duration_ms": data.duration_ms,
                "error": data.error,
            }),
        );
    }
    Value::Object(map)
}

pub fn steps_to_value(steps: &BTreeMap<String, StepData>) -> Value {
    let encoded = encode(steps);
    let size = serde_json::to_vec(&encoded).map_or(usize::MAX, |bytes| bytes.len());
    if size <= MAX_STEP_RESULTS_BYTES {
        return encoded;
    }
    let mut trimmed = steps.clone();
    for data in trimmed.values_mut() {
        data.body.clear();
        data.headers.clear();
    }
    encode(&trimmed)
}

pub fn steps_from_value(value: &Value) -> Result<BTreeMap<String, StepData>, StepResultsError> {
    let map = value.as_object().ok_or(StepResultsError)?;
    let mut steps = BTreeMap::new();
    for (name, item) in map {
        let item = item.as_object().ok_or(StepResultsError)?;
        let headers = match item.get("headers") {
            Some(Value::Array(entries)) => entries
                .iter()
                .map(|entry| {
                    let name = entry.get("name").and_then(Value::as_str)?;
                    let value = entry.get("value").and_then(Value::as_str)?;
                    Some(NameValue::new(name, value))
                })
                .collect::<Option<Vec<_>>>()
                .ok_or(StepResultsError)?,
            _ => Vec::new(),
        };
        steps.insert(
            name.clone(),
            StepData {
                status: item
                    .get("status")
                    .and_then(Value::as_u64)
                    .and_then(|status| u16::try_from(status).ok()),
                headers,
                body: item
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                duration_ms: item
                    .get("duration_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                error: item.get("error").and_then(Value::as_str).map(str::to_owned),
            },
        );
    }
    Ok(steps)
}
