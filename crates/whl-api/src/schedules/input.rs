use serde_json::{Map, Value};
use whl_store::schedules::{ScheduleHeader, ScheduleRequest};

use crate::error::{ApiError, FieldCode, FieldViolation};

pub const MAX_NAME_CHARS: usize = 100;

const SCHEDULE_KEYS: &[&str] = &["name", "cron", "timezone", "request", "enabled"];
const REQUEST_KEYS: &[&str] = &["method", "url", "headers", "body"];
const HEADER_KEYS: &[&str] = &["name", "value"];

#[derive(Debug, Default)]
pub struct ParsedSchedule {
    pub name: Option<String>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub request: Option<ScheduleRequest>,
    pub enabled: Option<bool>,
    pub violations: Vec<FieldViolation>,
}

pub fn parse_schedule(bytes: &[u8], creating: bool) -> Result<ParsedSchedule, ApiError> {
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
    walk.unknown_keys(&object, SCHEDULE_KEYS, "");
    let name = object.get("name").and_then(|value| walk.name(value));
    let cron = object
        .get("cron")
        .and_then(|value| walk.string("cron", value));
    let timezone = object
        .get("timezone")
        .and_then(|value| walk.string("timezone", value));
    let request = object.get("request").and_then(|value| walk.request(value));
    let enabled = object.get("enabled").and_then(|value| walk.boolean(value));
    if creating {
        for (key, present) in [
            ("name", name.is_some()),
            ("cron", cron.is_some()),
            ("request", request.is_some()),
        ] {
            if !present && !object.contains_key(key) {
                walk.push(key, FieldCode::Required, "is required");
            }
        }
    }
    Ok(ParsedSchedule {
        name,
        cron,
        timezone,
        request,
        enabled,
        violations: walk.violations,
    })
}

#[derive(Default)]
struct Walk {
    violations: Vec<FieldViolation>,
}

impl Walk {
    fn push(&mut self, field: impl Into<String>, code: FieldCode, message: impl Into<String>) {
        self.violations
            .push(FieldViolation::new(field, code, message));
    }

    fn unknown_keys(&mut self, object: &Map<String, Value>, known: &[&str], prefix: &str) {
        for key in object.keys().filter(|key| !known.contains(&key.as_str())) {
            self.push(
                format!("{prefix}{key}"),
                FieldCode::NotAllowed,
                "is not a known field",
            );
        }
    }

    fn string(&mut self, field: &str, value: &Value) -> Option<String> {
        match value {
            Value::String(text) => Some(text.clone()),
            _ => {
                self.push(field, FieldCode::InvalidFormat, "must be a string");
                None
            }
        }
    }

    fn boolean(&mut self, value: &Value) -> Option<bool> {
        match value {
            Value::Bool(flag) => Some(*flag),
            _ => {
                self.push("enabled", FieldCode::InvalidFormat, "must be a boolean");
                None
            }
        }
    }

    fn name(&mut self, value: &Value) -> Option<String> {
        let text = self.string("name", value)?;
        let length = text.chars().count();
        if length == 0 {
            self.push("name", FieldCode::Required, "must not be empty");
            return None;
        }
        if length > MAX_NAME_CHARS {
            self.push(
                "name",
                FieldCode::TooLong,
                format!("must be at most {MAX_NAME_CHARS} characters"),
            );
            return None;
        }
        if text.chars().any(char::is_control) {
            self.push(
                "name",
                FieldCode::InvalidFormat,
                "must not contain control characters",
            );
            return None;
        }
        Some(text)
    }

    fn request(&mut self, value: &Value) -> Option<ScheduleRequest> {
        let Value::Object(object) = value else {
            self.push("request", FieldCode::InvalidFormat, "must be an object");
            return None;
        };
        self.unknown_keys(object, REQUEST_KEYS, "request.");
        let method = match object.get("method") {
            Some(value) => self.string("request.method", value),
            None => {
                self.push("request.method", FieldCode::Required, "is required");
                None
            }
        };
        let url = match object.get("url") {
            Some(value) => self.string("request.url", value),
            None => {
                self.push("request.url", FieldCode::Required, "is required");
                None
            }
        };
        let headers = match object.get("headers") {
            None | Some(Value::Null) => Some(Vec::new()),
            Some(value) => self.headers(value),
        };
        let body = match object.get("body") {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(text)) => Some(Some(text.clone())),
            Some(_) => {
                self.push(
                    "request.body",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                None
            }
        };
        Some(ScheduleRequest {
            method: method?,
            url: url?,
            headers: headers?,
            body: body?,
        })
    }

    fn headers(&mut self, value: &Value) -> Option<Vec<ScheduleHeader>> {
        let Value::Array(items) = value else {
            self.push(
                "request.headers",
                FieldCode::InvalidFormat,
                "must be an array",
            );
            return None;
        };
        if items.len() > crate::schedules::MAX_HEADERS_INPUT {
            self.push(
                "request.headers",
                FieldCode::TooMany,
                format!(
                    "must have at most {} entries",
                    crate::schedules::MAX_HEADERS_INPUT
                ),
            );
            return None;
        }
        let mut parsed = Vec::with_capacity(items.len());
        let mut sound = true;
        for (index, item) in items.iter().enumerate() {
            let Value::Object(object) = item else {
                self.push(
                    format!("request.headers.{index}"),
                    FieldCode::InvalidFormat,
                    "must be an object",
                );
                sound = false;
                continue;
            };
            self.unknown_keys(object, HEADER_KEYS, &format!("request.headers.{index}."));
            let name = self.header_part(object, index, "name");
            let value = self.header_part(object, index, "value");
            match (name, value) {
                (Some(name), Some(value)) => parsed.push(ScheduleHeader { name, value }),
                _ => sound = false,
            }
        }
        sound.then_some(parsed)
    }

    fn header_part(
        &mut self,
        object: &Map<String, Value>,
        index: usize,
        key: &str,
    ) -> Option<String> {
        let field = format!("request.headers.{index}.{key}");
        match object.get(key) {
            Some(value) => self.string(&field, value),
            None => {
                self.push(field, FieldCode::Required, "is required");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(parsed: &ParsedSchedule) -> Vec<(String, FieldCode)> {
        parsed
            .violations
            .iter()
            .map(|violation| (violation.field().to_owned(), violation.code()))
            .collect()
    }

    #[test]
    fn a_complete_schedule_parses_and_a_missing_field_is_required() {
        let parsed = parse_schedule(
            br#"{"name":"n","cron":"*/5 * * * *","request":{"method":"GET","url":"https://e.example/"}}"#,
            true,
        )
        .expect("an object");
        assert!(parsed.violations.is_empty());
        assert_eq!(parsed.request.expect("request").headers, []);
        let missing = parse_schedule(br#"{"name":"n"}"#, true).expect("an object");
        assert_eq!(
            codes(&missing),
            [
                ("cron".to_owned(), FieldCode::Required),
                ("request".to_owned(), FieldCode::Required)
            ]
        );
    }

    #[test]
    fn a_partial_patch_needs_nothing_and_unknown_or_mistyped_fields_are_flagged() {
        let empty = parse_schedule(b"{}", false).expect("an object");
        assert!(empty.violations.is_empty());
        let bad =
            parse_schedule(br#"{"enabled":"yes","extra":1,"name":""}"#, false).expect("an object");
        assert_eq!(
            codes(&bad),
            [
                ("extra".to_owned(), FieldCode::NotAllowed),
                ("name".to_owned(), FieldCode::Required),
                ("enabled".to_owned(), FieldCode::InvalidFormat)
            ]
        );
    }
}
