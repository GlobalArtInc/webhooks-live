use std::time::Duration;

use serde_json::{Map, Value};
use whl_core::endpoint_id::EndpointId;
use whl_store::{MAX_TOKEN_ENDPOINTS, MAX_TOKEN_NAME_CHARS, TokenScope};

use crate::error::{ApiError, FieldCode, FieldViolation};

const KEYS: &[&str] = &["name", "scopes", "endpoint_ids", "expires_in_days"];
const DEFAULT_DAYS: i64 = 90;
const MIN_DAYS: i64 = 1;
const MAX_DAYS: i64 = 365;
const SECONDS_PER_DAY: u64 = 24 * 3600;
const MAX_VIOLATIONS: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub struct NewTokenInput {
    pub name: String,
    pub scopes: Vec<TokenScope>,
    pub endpoint_ids: Option<Vec<EndpointId>>,
    pub lifetime: Duration,
}

pub fn parse_token_input(bytes: &[u8]) -> Result<NewTokenInput, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut walk = Walk::default();
    let name = walk.name(&object);
    let scopes = walk.scopes(&object);
    let endpoint_ids = walk.endpoint_ids(&object);
    let days = walk.days(&object);
    walk.unknown_keys(&object);
    match (walk.violations.is_empty(), name, scopes, endpoint_ids, days) {
        (true, Some(name), Some(scopes), Some(endpoint_ids), Some(days)) => Ok(NewTokenInput {
            name,
            scopes,
            endpoint_ids,
            lifetime: Duration::from_secs(days.unsigned_abs() * SECONDS_PER_DAY),
        }),
        _ => Err(ApiError::Validation(walk.violations)),
    }
}

#[derive(Default)]
struct Walk {
    violations: Vec<FieldViolation>,
}

impl Walk {
    fn push(&mut self, field: &str, code: FieldCode, message: impl Into<String>) {
        if self.violations.len() < MAX_VIOLATIONS {
            self.violations
                .push(FieldViolation::new(field, code, message));
        }
    }

    fn unknown_keys(&mut self, object: &Map<String, Value>) {
        let unknown: Vec<&String> = object
            .keys()
            .filter(|key| !KEYS.contains(&key.as_str()))
            .collect();
        for key in unknown {
            self.push(key, FieldCode::NotAllowed, "is not a known field");
        }
    }

    fn name(&mut self, object: &Map<String, Value>) -> Option<String> {
        let Some(value) = object.get("name") else {
            self.push("name", FieldCode::Required, "is required");
            return None;
        };
        let Some(text) = value.as_str() else {
            self.push("name", FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        let trimmed = text.trim();
        let length = trimmed.chars().count();
        if length == 0 {
            self.push("name", FieldCode::Required, "must not be empty");
            None
        } else if length > MAX_TOKEN_NAME_CHARS {
            self.push(
                "name",
                FieldCode::TooLong,
                format!("must be at most {MAX_TOKEN_NAME_CHARS} characters"),
            );
            None
        } else if trimmed.chars().any(char::is_control) {
            self.push(
                "name",
                FieldCode::InvalidFormat,
                "must not contain control characters",
            );
            None
        } else {
            Some(trimmed.to_owned())
        }
    }

    fn scopes(&mut self, object: &Map<String, Value>) -> Option<Vec<TokenScope>> {
        let Some(value) = object.get("scopes") else {
            self.push("scopes", FieldCode::Required, "is required");
            return None;
        };
        let Some(items) = value.as_array() else {
            self.push("scopes", FieldCode::InvalidFormat, "must be an array");
            return None;
        };
        if items.is_empty() {
            self.push(
                "scopes",
                FieldCode::Required,
                "must name at least one scope",
            );
            return None;
        }
        if items.len() > TokenScope::ALL.len() {
            self.push(
                "scopes",
                FieldCode::OutOfRange,
                format!("must hold at most {} scopes", TokenScope::ALL.len()),
            );
            return None;
        }
        let mut scopes = Vec::with_capacity(items.len());
        let mut valid = true;
        for item in items {
            match item.as_str().and_then(TokenScope::parse) {
                Some(scope) if !scopes.contains(&scope) => scopes.push(scope),
                Some(_) => {}
                None => {
                    self.push(
                        "scopes",
                        FieldCode::InvalidFormat,
                        "must be read, endpoints:write, captures:write, rules:write or send",
                    );
                    valid = false;
                }
            }
        }
        valid.then_some(scopes)
    }

    fn endpoint_ids(&mut self, object: &Map<String, Value>) -> Option<Option<Vec<EndpointId>>> {
        let Some(value) = object.get("endpoint_ids") else {
            return Some(None);
        };
        if value.is_null() {
            return Some(None);
        }
        let Some(items) = value.as_array() else {
            self.push(
                "endpoint_ids",
                FieldCode::InvalidFormat,
                "must be an array or null",
            );
            return None;
        };
        if items.is_empty() || items.len() > MAX_TOKEN_ENDPOINTS {
            self.push(
                "endpoint_ids",
                FieldCode::OutOfRange,
                format!("must hold 1 to {MAX_TOKEN_ENDPOINTS} endpoint ids"),
            );
            return None;
        }
        let mut ids: Vec<EndpointId> = Vec::with_capacity(items.len());
        let mut valid = true;
        for item in items {
            match item.as_str().map(EndpointId::parse) {
                Some(Ok(id)) if !ids.contains(&id) => ids.push(id),
                Some(Ok(_)) => {}
                _ => {
                    self.push(
                        "endpoint_ids",
                        FieldCode::InvalidFormat,
                        "must hold endpoint ids",
                    );
                    valid = false;
                }
            }
        }
        valid.then_some(Some(ids))
    }

    fn days(&mut self, object: &Map<String, Value>) -> Option<i64> {
        let Some(value) = object.get("expires_in_days") else {
            return Some(DEFAULT_DAYS);
        };
        match value.as_i64() {
            Some(days) if (MIN_DAYS..=MAX_DAYS).contains(&days) => Some(days),
            Some(_) => {
                self.push(
                    "expires_in_days",
                    FieldCode::OutOfRange,
                    format!("must be between {MIN_DAYS} and {MAX_DAYS}"),
                );
                None
            }
            None => {
                self.push(
                    "expires_in_days",
                    FieldCode::InvalidFormat,
                    "must be an integer",
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdefghjkmnpq";

    fn parse(text: &str) -> Result<NewTokenInput, ApiError> {
        parse_token_input(text.as_bytes())
    }

    fn fields(result: Result<NewTokenInput, ApiError>) -> Vec<(String, FieldCode)> {
        match result {
            Err(ApiError::Validation(violations)) => violations
                .iter()
                .map(|violation| (violation.field().to_owned(), violation.code()))
                .collect(),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_minimal_input_takes_the_default_expiry_and_no_endpoint_limit() {
        let parsed =
            parse(r#"{"name":" laptop ","scopes":["read","read","send"]}"#).expect("a valid input");
        assert_eq!(parsed.name, "laptop");
        assert_eq!(parsed.scopes, [TokenScope::Read, TokenScope::Send]);
        assert_eq!(parsed.endpoint_ids, None);
        assert_eq!(parsed.lifetime, Duration::from_secs(90 * SECONDS_PER_DAY));
    }

    #[test]
    fn the_expiry_bounds_are_inclusive() {
        for days in [1, 365] {
            let input = format!(r#"{{"name":"n","scopes":["read"],"expires_in_days":{days}}}"#);
            assert!(parse(&input).is_ok(), "{days}");
        }
        for days in ["0", "366", "-1", "1.5", "\"7\""] {
            let input = format!(r#"{{"name":"n","scopes":["read"],"expires_in_days":{days}}}"#);
            assert_eq!(
                fields(parse(&input)).first().map(|field| field.0.as_str()),
                Some("expires_in_days"),
                "{days}"
            );
        }
    }

    #[test]
    fn the_name_scopes_and_endpoint_limit_are_validated() {
        let long = "x".repeat(65);
        let ids: Vec<String> = (0..51).map(|_| format!("\"{ID}\"")).collect();
        let cases: [(String, &str); 9] = [
            (r#"{"scopes":["read"]}"#.to_owned(), "name"),
            (r#"{"name":"  ","scopes":["read"]}"#.to_owned(), "name"),
            (format!(r#"{{"name":"{long}","scopes":["read"]}}"#), "name"),
            (r#"{"name":"n"}"#.to_owned(), "scopes"),
            (r#"{"name":"n","scopes":[]}"#.to_owned(), "scopes"),
            (r#"{"name":"n","scopes":["admin"]}"#.to_owned(), "scopes"),
            (
                r#"{"name":"n","scopes":["read"],"endpoint_ids":[]}"#.to_owned(),
                "endpoint_ids",
            ),
            (
                r#"{"name":"n","scopes":["read"],"endpoint_ids":["nope"]}"#.to_owned(),
                "endpoint_ids",
            ),
            (
                format!(
                    r#"{{"name":"n","scopes":["read"],"endpoint_ids":[{}]}}"#,
                    ids.join(",")
                ),
                "endpoint_ids",
            ),
        ];
        for (input, field) in cases {
            let found = fields(parse(&input));
            assert_eq!(found.first().map(|f| f.0.as_str()), Some(field), "{input}");
        }
    }

    #[test]
    fn an_unknown_key_and_a_bad_body_are_refused() {
        let found = fields(parse(r#"{"name":"n","scopes":["read"],"admin":true}"#));
        assert_eq!(found, [("admin".to_owned(), FieldCode::NotAllowed)]);
        assert!(matches!(parse(""), Err(ApiError::InvalidJson(_))));
        assert!(matches!(parse("[]"), Err(ApiError::InvalidJson(_))));
        assert!(matches!(parse("{"), Err(ApiError::InvalidJson(_))));
    }

    #[test]
    fn a_flood_of_bad_items_yields_one_scope_violation_and_a_capped_list() {
        let scopes = vec!["1"; 5000].join(",");
        let found = fields(parse(&format!(r#"{{"name":"n","scopes":[{scopes}]}}"#)));
        assert_eq!(found, [("scopes".to_owned(), FieldCode::OutOfRange)]);
        let bad = ["1"; 5].join(",");
        let keys: Vec<String> = (0..100).map(|index| format!("\"k{index}\":1")).collect();
        let body = format!(r#"{{"name":"n","scopes":[{bad}],{}}}"#, keys.join(","));
        assert_eq!(fields(parse(&body)).len(), MAX_VIOLATIONS);
    }

    #[test]
    fn an_endpoint_limit_keeps_distinct_ids_in_order() {
        let other = "0123456789abcdefghjkmnpr";
        let input =
            format!(r#"{{"name":"n","scopes":["read"],"endpoint_ids":["{ID}","{other}","{ID}"]}}"#);
        let parsed = parse(&input).expect("a valid input");
        let ids = parsed.endpoint_ids.expect("a limit");
        assert_eq!(
            ids.iter().map(EndpointId::unpack).collect::<Vec<_>>(),
            [ID, other]
        );
    }
}
