use serde_json::{Map, Value};
use whl_core::capture_id::CaptureId;

use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::input::{ResponsePatch, parse_response_object};

const REQUEST_KEYS: [&str; 2] = ["capture_id", "response"];
const RESPONSE_KEYS: [&str; 5] = [
    "status",
    "content_type",
    "headers",
    "body",
    "status_template",
];

#[derive(Debug)]
pub(super) struct PreviewRequest {
    pub capture: Option<CaptureId>,
    pub response: ResponsePatch,
}

pub(super) fn parse_capture_id(raw: &str) -> Result<CaptureId, FieldViolation> {
    CaptureId::parse(raw).map_err(|_| {
        FieldViolation::new(
            "capture_id",
            FieldCode::InvalidFormat,
            "must be a capture id",
        )
    })
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

fn unknown_keys(object: &Map<String, Value>, prefix: &str, known: &[&str]) -> Vec<FieldViolation> {
    object
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .map(|key| {
            let field = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            FieldViolation::new(field, FieldCode::NotAllowed, "is not a known field")
        })
        .collect()
}

pub(super) fn parse_preview(bytes: &[u8]) -> Result<PreviewRequest, ApiError> {
    let object = parse_object(bytes)?;
    let mut violations = unknown_keys(&object, "", &REQUEST_KEYS);
    let capture = match object.get("capture_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => match parse_capture_id(text) {
            Ok(id) => Some(id),
            Err(violation) => {
                violations.push(violation);
                None
            }
        },
        Some(_) => {
            violations.push(FieldViolation::new(
                "capture_id",
                FieldCode::InvalidFormat,
                "must be a string or null",
            ));
            None
        }
    };
    let response = match object.get("response") {
        Some(Value::Object(response)) => {
            violations.extend(unknown_keys(response, "response", &RESPONSE_KEYS));
            let parsed = parse_response_object(response);
            violations.extend(parsed.violations);
            parsed.patch
        }
        None | Some(Value::Null) => {
            violations.push(FieldViolation::new(
                "response",
                FieldCode::Required,
                "is required",
            ));
            ResponsePatch::default()
        }
        Some(_) => {
            violations.push(FieldViolation::new(
                "response",
                FieldCode::InvalidFormat,
                "must be an object",
            ));
            ResponsePatch::default()
        }
    };
    if violations.is_empty() {
        Ok(PreviewRequest { capture, response })
    } else {
        Err(ApiError::Validation(violations))
    }
}
