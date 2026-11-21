use serde_json::{Map, Value};
use whl_rules::{MAX_RULES_PER_ENDPOINT, RuleDiagnostic, RuleErrorCode};
use whl_store::Uuid;

use crate::error::{ApiError, FieldCode, FieldViolation};

pub fn parse_object(bytes: &[u8]) -> Result<Map<String, Value>, ApiError> {
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

pub fn parse_order(bytes: &[u8]) -> Result<Vec<Uuid>, ApiError> {
    let object = parse_object(bytes)?;
    let mut violations = Vec::new();
    for key in object.keys().filter(|key| key.as_str() != "ids") {
        violations.push(FieldViolation::new(
            key,
            FieldCode::NotAllowed,
            "is not a known field",
        ));
    }
    let items = match object.get("ids") {
        Some(Value::Array(items)) => items,
        None | Some(Value::Null) => {
            return Err(ApiError::Validation(vec![FieldViolation::new(
                "ids",
                FieldCode::Required,
                "is required",
            )]));
        }
        Some(_) => {
            return Err(ApiError::Validation(vec![FieldViolation::new(
                "ids",
                FieldCode::InvalidFormat,
                "must be an array",
            )]));
        }
    };
    if items.len() > MAX_RULES_PER_ENDPOINT {
        violations.push(FieldViolation::new(
            "ids",
            FieldCode::TooMany,
            format!("must have at most {MAX_RULES_PER_ENDPOINT} entries"),
        ));
        return Err(ApiError::Validation(violations));
    }
    let mut ids: Vec<Uuid> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let parsed = item.as_str().and_then(|text| {
            Uuid::try_parse(text)
                .ok()
                .filter(|uuid| uuid.hyphenated().to_string() == text)
        });
        match parsed {
            Some(id) if ids.contains(&id) => violations.push(FieldViolation::new(
                format!("ids.{index}"),
                FieldCode::Duplicate,
                "is listed twice",
            )),
            Some(id) => ids.push(id),
            None => violations.push(FieldViolation::new(
                format!("ids.{index}"),
                FieldCode::InvalidFormat,
                "must be a lowercase hyphenated UUID",
            )),
        }
    }
    if violations.is_empty() {
        Ok(ids)
    } else {
        Err(ApiError::Validation(violations))
    }
}

pub fn order_violation() -> ApiError {
    ApiError::Validation(vec![FieldViolation::new(
        "ids",
        FieldCode::InvalidFormat,
        "must list every rule of the endpoint exactly once",
    )])
}

pub fn violations(
    diagnostics: Vec<RuleDiagnostic>,
    candidate: Option<usize>,
) -> Vec<FieldViolation> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let message = match diagnostic.position {
                Some(position) => format!(
                    "{} (line {}, column {})",
                    diagnostic.message, position.line, position.column
                ),
                None => diagnostic.message,
            };
            FieldViolation::new(
                relative(&diagnostic.field, candidate),
                field_code(diagnostic.code),
                message,
            )
        })
        .collect()
}

fn relative(field: &str, candidate: Option<usize>) -> String {
    match candidate {
        Some(index) => field
            .strip_prefix(&format!("rules.{index}."))
            .map_or_else(|| field.to_owned(), str::to_owned),
        None => field.to_owned(),
    }
}

fn field_code(code: RuleErrorCode) -> FieldCode {
    match code {
        RuleErrorCode::Required => FieldCode::Required,
        RuleErrorCode::InvalidFormat => FieldCode::InvalidFormat,
        RuleErrorCode::TooLong => FieldCode::TooLong,
        RuleErrorCode::OutOfRange => FieldCode::OutOfRange,
        RuleErrorCode::TooMany => FieldCode::TooMany,
        RuleErrorCode::Duplicate => FieldCode::Duplicate,
        RuleErrorCode::NotAllowed => FieldCode::NotAllowed,
    }
}
