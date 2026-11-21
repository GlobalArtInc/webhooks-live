use serde_json::Value;
use whl_secrets::{MAX_VALUE_BYTES, Secret, ValueError};

use crate::error::{ApiError, FieldCode, FieldViolation};

const VALUE_KEY: &str = "value";

pub fn parse_put(bytes: &[u8]) -> Result<Secret, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(mut object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut violations: Vec<FieldViolation> = object
        .keys()
        .filter(|key| key.as_str() != VALUE_KEY)
        .map(|key| FieldViolation::new(key.as_str(), FieldCode::NotAllowed, "is not a known field"))
        .collect();
    match object.remove(VALUE_KEY) {
        None => violations.push(FieldViolation::new(
            VALUE_KEY,
            FieldCode::Required,
            "is required",
        )),
        Some(Value::String(text)) => match Secret::new(text) {
            Ok(secret) if violations.is_empty() => return Ok(secret),
            Ok(_) => {}
            Err(ValueError::Empty) => violations.push(FieldViolation::new(
                VALUE_KEY,
                FieldCode::Required,
                "must not be empty",
            )),
            Err(ValueError::TooLong) => violations.push(FieldViolation::new(
                VALUE_KEY,
                FieldCode::TooLong,
                format!("must be at most {MAX_VALUE_BYTES} bytes"),
            )),
        },
        Some(_) => violations.push(FieldViolation::new(
            VALUE_KEY,
            FieldCode::InvalidFormat,
            "must be a string",
        )),
    }
    Err(ApiError::Validation(violations))
}
