use serde_json::{Map, Value};
use whl_core::content_type::ContentType;
use whl_forward::{RefusalReason, TargetCheckError};
use whl_store::Uuid;
use whl_store::forward::ForwardRulePatch;

use crate::error::{ApiError, FieldCode, FieldViolation, core_code};

pub const MAX_METHOD_BYTES: usize = 16;
pub const MAX_ALLOWLIST_ENTRIES: usize = 50;
pub const MAX_ALLOWLIST_NAME_BYTES: usize = 64;

const RULE_KEYS: &[&str] = &[
    "enabled",
    "target_url",
    "method",
    "content_type",
    "header_allowlist",
    "append_path",
];
const REQUEST_KEYS: &[&str] = &[
    "rule_id",
    "target_url",
    "method",
    "content_type",
    "header_allowlist",
];
const REQUEST_RULE_CONFLICTS: &[&str] =
    &["target_url", "method", "content_type", "header_allowlist"];

#[derive(Debug, Default)]
pub struct ParsedRule {
    pub fields: ForwardRulePatch,
    pub violations: Vec<FieldViolation>,
}

#[derive(Debug, Default)]
pub struct ParsedRequest {
    pub rule_id: Option<Uuid>,
    pub fields: ForwardRulePatch,
    pub violations: Vec<FieldViolation>,
}

pub fn parse_rule(bytes: &[u8], creating: bool) -> Result<ParsedRule, ApiError> {
    let object = object_of(bytes, creating)?.unwrap_or_default();
    let mut walk = Walk::default();
    walk.unknown_keys(&object, RULE_KEYS);
    let fields = walk.fields(&object, true);
    if creating && !object.contains_key("target_url") {
        walk.push("target_url", FieldCode::Required, "is required");
    }
    Ok(ParsedRule {
        fields,
        violations: walk.violations,
    })
}

pub fn parse_request(bytes: &[u8]) -> Result<ParsedRequest, ApiError> {
    let object = object_of(bytes, true)?.unwrap_or_default();
    let mut walk = Walk::default();
    walk.unknown_keys(&object, REQUEST_KEYS);
    let rule_id = object.get("rule_id").and_then(|value| walk.rule_id(value));
    let mut fields = walk.fields(&object, false);
    if rule_id.is_some() {
        for key in REQUEST_RULE_CONFLICTS {
            if object.get(*key).is_some_and(|value| !value.is_null()) {
                walk.push(
                    *key,
                    FieldCode::NotAllowed,
                    "must be absent when rule_id is set",
                );
            }
        }
        fields = ForwardRulePatch::default();
    } else if object.get("rule_id").is_none_or(Value::is_null) {
        let target_missing = object.get("target_url").is_none_or(Value::is_null);
        if target_missing {
            walk.push("target_url", FieldCode::Required, "is required");
        }
    }
    Ok(ParsedRequest {
        rule_id,
        fields,
        violations: walk.violations,
    })
}

fn object_of(bytes: &[u8], allow_empty: bool) -> Result<Option<Map<String, Value>>, ApiError> {
    if bytes.is_empty() {
        return if allow_empty {
            Ok(None)
        } else {
            Err(ApiError::InvalidJson("the request body is empty"))
        };
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    match value {
        Value::Object(object) => Ok(Some(object)),
        _ => Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        )),
    }
}

pub fn target_violation(error: &TargetCheckError) -> FieldViolation {
    target_violation_for("target_url", error)
}

pub fn target_violation_for(field: &str, error: &TargetCheckError) -> FieldViolation {
    match error {
        TargetCheckError::Refused(reason) => {
            FieldViolation::new(field, refusal_code(*reason), reason.to_string())
        }
        TargetCheckError::Unresolvable(detail) => {
            FieldViolation::new(field, FieldCode::TargetUnresolvable, detail.clone())
        }
    }
}

fn refusal_code(reason: RefusalReason) -> FieldCode {
    match reason {
        RefusalReason::InvalidUrl
        | RefusalReason::UrlTooLong
        | RefusalReason::Scheme
        | RefusalReason::MissingHost => FieldCode::InvalidFormat,
        RefusalReason::Userinfo
        | RefusalReason::ForbiddenHost
        | RefusalReason::ForbiddenPort
        | RefusalReason::RefusedAddress
        | RefusalReason::HttpsDowngrade => FieldCode::TargetRefused,
    }
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

    fn unknown_keys(&mut self, object: &Map<String, Value>, known: &[&str]) {
        let unknown: Vec<String> = object
            .keys()
            .filter(|key| !known.contains(&key.as_str()))
            .cloned()
            .collect();
        for field in unknown {
            self.push(field, FieldCode::NotAllowed, "is not a known field");
        }
    }

    fn fields(&mut self, object: &Map<String, Value>, with_flags: bool) -> ForwardRulePatch {
        let mut fields = ForwardRulePatch::default();
        if with_flags {
            if let Some(value) = object.get("enabled") {
                fields.enabled = self.boolean(value, "enabled");
            }
            if let Some(value) = object.get("append_path") {
                fields.append_path = self.boolean(value, "append_path");
            }
        }
        let request = !with_flags;
        if let Some(value) = object
            .get("target_url")
            .filter(|value| !(request && value.is_null()))
        {
            fields.target_url = self.target_text(value);
        }
        if let Some(value) = object.get("method") {
            fields.method = self.method(value);
        }
        if let Some(value) = object.get("content_type") {
            fields.content_type = self.content_type(value);
        }
        if let Some(value) = object
            .get("header_allowlist")
            .filter(|value| !(request && value.is_null()))
        {
            fields.header_allowlist = self.allowlist(value);
        }
        fields
    }

    fn boolean(&mut self, value: &Value, field: &str) -> Option<bool> {
        let flag = value.as_bool();
        if flag.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be a boolean");
        }
        flag
    }

    fn rule_id(&mut self, value: &Value) -> Option<Uuid> {
        if value.is_null() {
            return None;
        }
        let parsed = value.as_str().and_then(|text| {
            Uuid::try_parse(text)
                .ok()
                .filter(|uuid| uuid.hyphenated().to_string() == text)
        });
        if parsed.is_none() {
            self.push(
                "rule_id",
                FieldCode::InvalidFormat,
                "must be a lowercase hyphenated UUID or null",
            );
        }
        parsed
    }

    fn target_text(&mut self, value: &Value) -> Option<String> {
        match value {
            Value::String(text) if !text.is_empty() => Some(text.clone()),
            Value::String(_) | Value::Null => {
                self.push("target_url", FieldCode::Required, "must not be empty");
                None
            }
            _ => {
                self.push("target_url", FieldCode::InvalidFormat, "must be a string");
                None
            }
        }
    }

    fn method(&mut self, value: &Value) -> Option<Option<String>> {
        let text = match value {
            Value::Null => return Some(None),
            Value::String(text) => text,
            _ => {
                self.push(
                    "method",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                return None;
            }
        };
        if text.is_empty() {
            self.push("method", FieldCode::Required, "must not be empty");
            return None;
        }
        if text.len() > MAX_METHOD_BYTES {
            self.push(
                "method",
                FieldCode::TooLong,
                format!("must be at most {MAX_METHOD_BYTES} bytes"),
            );
            return None;
        }
        if !text.bytes().all(is_token_byte) {
            self.push(
                "method",
                FieldCode::InvalidFormat,
                "must be a valid HTTP token",
            );
            return None;
        }
        Some(Some(text.to_ascii_uppercase()))
    }

    fn content_type(&mut self, value: &Value) -> Option<Option<String>> {
        let text = match value {
            Value::Null => return Some(None),
            Value::String(text) => text,
            _ => {
                self.push(
                    "content_type",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                return None;
            }
        };
        match ContentType::parse(text) {
            Ok(parsed) => Some(Some(parsed.unpack().to_owned())),
            Err(error) => {
                for code in error.codes() {
                    self.push("content_type", core_code(*code), code.to_string());
                }
                None
            }
        }
    }

    fn allowlist(&mut self, value: &Value) -> Option<Vec<String>> {
        let items = match value {
            Value::Array(items) => items,
            _ => {
                self.push(
                    "header_allowlist",
                    FieldCode::InvalidFormat,
                    "must be an array",
                );
                return None;
            }
        };
        if items.len() > MAX_ALLOWLIST_ENTRIES {
            self.push(
                "header_allowlist",
                FieldCode::TooMany,
                format!("must have at most {MAX_ALLOWLIST_ENTRIES} entries"),
            );
            return None;
        }
        let mut names: Vec<String> = Vec::with_capacity(items.len());
        let mut valid = true;
        for (index, item) in items.iter().enumerate() {
            let field = format!("header_allowlist.{index}");
            match self.allowlist_name(item, &field) {
                Some(name) => {
                    if !names.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
                        names.push(name);
                    }
                }
                None => valid = false,
            }
        }
        valid.then_some(names)
    }

    fn allowlist_name(&mut self, item: &Value, field: &str) -> Option<String> {
        let Some(text) = item.as_str() else {
            self.push(field, FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        if text.is_empty() {
            self.push(field, FieldCode::Required, "must not be empty");
            return None;
        }
        if text.len() > MAX_ALLOWLIST_NAME_BYTES {
            self.push(
                field,
                FieldCode::TooLong,
                format!("must be at most {MAX_ALLOWLIST_NAME_BYTES} bytes"),
            );
            return None;
        }
        if !text.bytes().all(is_token_byte) {
            self.push(
                field,
                FieldCode::InvalidFormat,
                "must be a valid HTTP token",
            );
            return None;
        }
        Some(text.to_owned())
    }
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}
