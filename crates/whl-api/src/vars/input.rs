use serde_json::Value;
use whl_store::vars::{MAX_VAR_NAME_BYTES, MAX_VAR_VALUE_BYTES};

use crate::error::{ApiError, FieldCode, FieldViolation};

const VALUE_KEY: &str = "value";
const NAME_FIELD: &str = "name";

pub fn is_valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    name.len() <= MAX_VAR_NAME_BYTES
        && (first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn parse_name(raw: &str) -> Result<&str, ApiError> {
    if is_valid_name(raw) {
        Ok(raw)
    } else {
        Err(ApiError::Validation(vec![FieldViolation::new(
            NAME_FIELD,
            FieldCode::InvalidFormat,
            "must match ^[a-z_][a-z0-9_]{0,63}$",
        )]))
    }
}

pub fn parse_put(bytes: &[u8]) -> Result<Value, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(mut object) = document else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut violations: Vec<FieldViolation> = object
        .keys()
        .filter(|key| key.as_str() != VALUE_KEY)
        .map(|key| FieldViolation::new(key.as_str(), FieldCode::NotAllowed, "is not a known field"))
        .collect();
    let Some(value) = object.remove(VALUE_KEY) else {
        violations.push(FieldViolation::new(
            VALUE_KEY,
            FieldCode::Required,
            "is required",
        ));
        return Err(ApiError::Validation(violations));
    };
    if contains_nul(&value) {
        violations.push(FieldViolation::new(
            VALUE_KEY,
            FieldCode::InvalidFormat,
            "must not contain NUL characters",
        ));
    }
    let serialized = serde_json::to_vec(&value)
        .map_err(|_| ApiError::InvalidJson("the value cannot be serialized"))?;
    if serialized.len() > MAX_VAR_VALUE_BYTES {
        violations.push(FieldViolation::new(
            VALUE_KEY,
            FieldCode::TooLong,
            format!("must be at most {MAX_VAR_VALUE_BYTES} bytes serialized"),
        ));
    }
    if violations.is_empty() {
        Ok(value)
    } else {
        Err(ApiError::Validation(violations))
    }
}

pub fn contains_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(contains_nul),
        Value::Object(map) => map
            .iter()
            .any(|(key, item)| key.contains('\0') || contains_nul(item)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_documented_pattern() {
        let longest = "a".repeat(64);
        let too_long = "a".repeat(65);
        for valid in ["a", "_", "orders_2", longest.as_str()] {
            assert!(is_valid_name(valid), "{valid}");
        }
        for invalid in ["", "A", "1a", "a-b", "a b", too_long.as_str()] {
            assert!(!is_valid_name(invalid), "{invalid}");
        }
    }

    #[test]
    fn nul_characters_are_refused_anywhere_in_a_value() {
        for body in [
            r#"{"value":"a\u0000b"}"#,
            r#"{"value":["x",{"k":"\u0000"}]}"#,
            r#"{"value":{"k\u0000":1}}"#,
        ] {
            assert!(
                matches!(parse_put(body.as_bytes()), Err(ApiError::Validation(_))),
                "{body}"
            );
        }
    }

    #[test]
    fn a_large_array_within_the_cap_fits_the_database_bound() {
        let items = "1,".repeat(MAX_VAR_VALUE_BYTES / 2 - 2);
        let body = format!("{{\"value\":[{}1]}}", items);
        assert!(parse_put(body.as_bytes()).is_ok());
    }

    #[test]
    fn a_value_of_exactly_the_cap_is_accepted_and_one_more_byte_is_refused() {
        let at_cap = "a".repeat(MAX_VAR_VALUE_BYTES - 2);
        let body = serde_json::json!({ "value": at_cap }).to_string();
        assert!(parse_put(body.as_bytes()).is_ok());
        let over = "a".repeat(MAX_VAR_VALUE_BYTES - 1);
        let body = serde_json::json!({ "value": over }).to_string();
        assert!(matches!(
            parse_put(body.as_bytes()),
            Err(ApiError::Validation(_))
        ));
    }
}
