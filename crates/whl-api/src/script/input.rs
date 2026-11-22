use serde_json::{Map, Value};
use whl_script::MAX_SOURCE_BYTES;

use crate::error::{ApiError, FieldCode, FieldViolation};

const KEYS: [&str; 2] = ["source", "phase"];
const PHASES: [&str; 2] = ["before", "after"];
const ENVELOPE_BYTES: usize = 1024;
const WORST_ESCAPE_FACTOR: usize = 6;

pub(crate) const MAX_VALIDATE_BODY_BYTES: usize =
    MAX_SOURCE_BYTES * WORST_ESCAPE_FACTOR + ENVELOPE_BYTES;

#[derive(Debug)]
pub(super) struct ValidateRequest {
    pub source: String,
}

fn parse_object(bytes: &[u8]) -> Result<Map<String, Value>, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        )),
    }
}

pub(super) fn parse_validate(bytes: &[u8]) -> Result<ValidateRequest, ApiError> {
    let object = parse_object(bytes)?;
    let mut violations: Vec<FieldViolation> = object
        .keys()
        .filter(|key| !KEYS.contains(&key.as_str()))
        .map(|key| FieldViolation::new(key.clone(), FieldCode::NotAllowed, "is not a known field"))
        .collect();
    let source = match object.get("source") {
        Some(Value::String(text)) if text.len() <= MAX_SOURCE_BYTES => Some(text.clone()),
        Some(Value::String(_)) => {
            violations.push(FieldViolation::new(
                "source",
                FieldCode::TooLong,
                format!("must be at most {MAX_SOURCE_BYTES} bytes"),
            ));
            None
        }
        None | Some(Value::Null) => {
            violations.push(FieldViolation::new(
                "source",
                FieldCode::Required,
                "is required",
            ));
            None
        }
        Some(_) => {
            violations.push(FieldViolation::new(
                "source",
                FieldCode::InvalidFormat,
                "must be a string",
            ));
            None
        }
    };
    match object.get("phase") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) if PHASES.contains(&text.as_str()) => {}
        Some(_) => violations.push(FieldViolation::new(
            "phase",
            FieldCode::InvalidFormat,
            "must be before or after",
        )),
    }
    match source {
        Some(source) if violations.is_empty() => Ok(ValidateRequest { source }),
        _ => Err(ApiError::Validation(violations)),
    }
}
