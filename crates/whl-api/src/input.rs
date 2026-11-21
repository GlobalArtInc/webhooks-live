use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use whl_core::header::HeaderInput;
use whl_core::tag::{MAX_TAGS, Tag};
use whl_store::Overflow;
use whl_verify::VerifierInput;

use crate::error::{ApiError, FieldCode, FieldViolation};

const TITLE_MAX_CHARS: usize = 100;
const ENDPOINT_KEYS: &[&str] = &[
    "title",
    "response",
    "cors",
    "overflow",
    "tags",
    "retention",
    "max_captures",
    "capture_ttl_seconds",
    "verifier",
];
const RETENTION_KEYS: &[&str] = &["mode", "seconds"];
const VERIFIER_KEYS: &[&str] = &[
    "scheme",
    "secret_name",
    "header",
    "algorithm",
    "encoding",
    "prefix",
    "timestamp_header",
];
const RESPONSE_KEYS: &[&str] = &[
    "status",
    "content_type",
    "headers",
    "body",
    "delay_ms",
    "templated",
    "status_template",
];
const STATUS_TEMPLATE_MAX_CHARS: usize = 1024;
const HEADER_KEYS: &[&str] = &["name", "value"];
const BODY_KEYS: &[&str] = &["encoding", "data", "truncated"];

#[derive(Debug, Default, PartialEq)]
pub struct EndpointPatch {
    pub title: Option<Option<String>>,
    pub response: ResponsePatch,
    pub cors: Option<bool>,
    pub overflow: Option<Overflow>,
    pub tags: Option<Vec<Tag>>,
    pub lifecycle: LifecyclePatch,
    pub verifier: Option<Option<VerifierInput>>,
}

#[derive(Debug, Default, PartialEq)]
pub struct LifecyclePatch {
    pub retention: Option<RetentionPatch>,
    pub max_captures: Option<Option<i64>>,
    pub capture_ttl_seconds: Option<Option<i64>>,
}

impl LifecyclePatch {
    pub fn is_empty(&self) -> bool {
        self.retention.is_none()
            && self.max_captures.is_none()
            && self.capture_ttl_seconds.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPatch {
    Sliding { seconds: Option<i64> },
    Never,
}

#[derive(Debug, Default, PartialEq)]
pub struct ResponsePatch {
    pub status: Option<i64>,
    pub content_type: Option<String>,
    pub headers: Option<Vec<HeaderInput>>,
    pub body: Option<Vec<u8>>,
    pub delay_ms: Option<i64>,
    pub templated: Option<bool>,
    pub status_template: Option<Option<String>>,
}

#[derive(Debug, Default)]
pub struct ParsedInput {
    pub patch: EndpointPatch,
    pub violations: Vec<FieldViolation>,
}

pub fn parse_endpoint_input(bytes: &[u8], allow_empty: bool) -> Result<ParsedInput, ApiError> {
    if bytes.is_empty() {
        return if allow_empty {
            Ok(ParsedInput::default())
        } else {
            Err(ApiError::InvalidJson("the request body is empty"))
        };
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut walk = Walk::default();
    let patch = walk.endpoint(&object);
    Ok(ParsedInput {
        patch,
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

    fn unknown_keys(&mut self, object: &Map<String, Value>, path: &str, known: &[&str]) {
        let unknown: Vec<String> = object
            .keys()
            .filter(|key| !known.contains(&key.as_str()))
            .map(|key| join(path, key))
            .collect();
        for field in unknown {
            self.push(field, FieldCode::NotAllowed, "is not a known field");
        }
    }

    fn endpoint(&mut self, object: &Map<String, Value>) -> EndpointPatch {
        self.unknown_keys(object, "", ENDPOINT_KEYS);
        let mut patch = EndpointPatch::default();
        if let Some(value) = object.get("title") {
            patch.title = self.title(value);
        }
        if let Some(value) = object.get("response")
            && let Some(response) = self.object(value, "response")
        {
            patch.response = self.response(response);
        }
        if let Some(value) = object.get("cors") {
            patch.cors = self.boolean(value, "cors");
        }
        if let Some(value) = object.get("overflow") {
            patch.overflow = self.overflow(value);
        }
        if let Some(value) = object.get("tags") {
            patch.tags = self.tags(value);
        }
        if let Some(value) = object.get("retention") {
            patch.lifecycle.retention = self.retention(value);
        }
        if let Some(value) = object.get("max_captures") {
            patch.lifecycle.max_captures = self.nullable_integer(value, "max_captures");
        }
        if let Some(value) = object.get("capture_ttl_seconds") {
            patch.lifecycle.capture_ttl_seconds =
                self.nullable_integer(value, "capture_ttl_seconds");
        }
        if let Some(value) = object.get("verifier") {
            patch.verifier = self.verifier(value);
        }
        patch
    }

    fn tags(&mut self, value: &Value) -> Option<Vec<Tag>> {
        let Value::Array(items) = value else {
            self.push(
                "tags",
                FieldCode::InvalidFormat,
                "must be an array of strings",
            );
            return None;
        };
        if items.len() > MAX_TAGS {
            self.push(
                "tags",
                FieldCode::TooMany,
                format!("must have at most {MAX_TAGS} entries"),
            );
            return None;
        }
        let mut tags: Vec<Tag> = Vec::with_capacity(items.len());
        for item in items {
            let Some(tag) = item.as_str().and_then(|text| Tag::parse(text).ok()) else {
                self.push(
                    "tags",
                    FieldCode::InvalidFormat,
                    "each tag must be 1 to 32 characters of a to z, 0 to 9 and hyphen, not starting with a hyphen",
                );
                return None;
            };
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        Some(tags)
    }

    fn retention(&mut self, value: &Value) -> Option<RetentionPatch> {
        let object = self.object(value, "retention")?;
        self.unknown_keys(object, "retention", RETENTION_KEYS);
        let seconds = match object.get("seconds") {
            None | Some(Value::Null) => None,
            Some(other) => Some(self.integer(other, "retention.seconds")?),
        };
        match object.get("mode").and_then(Value::as_str) {
            Some("sliding") => Some(RetentionPatch::Sliding { seconds }),
            Some("never") if seconds.is_none() => Some(RetentionPatch::Never),
            Some("never") => {
                self.push(
                    "retention.seconds",
                    FieldCode::NotAllowed,
                    "must be null when the mode is never",
                );
                None
            }
            _ => {
                self.push(
                    "retention.mode",
                    FieldCode::InvalidFormat,
                    "must be sliding or never",
                );
                None
            }
        }
    }

    fn nullable_integer(&mut self, value: &Value, field: &str) -> Option<Option<i64>> {
        if value.is_null() {
            return Some(None);
        }
        self.integer(value, field).map(Some)
    }

    fn verifier(&mut self, value: &Value) -> Option<Option<VerifierInput>> {
        if value.is_null() {
            return Some(None);
        }
        let object = self.object(value, "verifier")?;
        self.unknown_keys(object, "verifier", VERIFIER_KEYS);
        let scheme = self.required_text(object, "verifier", "scheme");
        let secret_name = self.required_text(object, "verifier", "secret_name");
        let header = self.optional_text(object, "header");
        let algorithm = self.optional_text(object, "algorithm");
        let encoding = self.optional_text(object, "encoding");
        let prefix = self.optional_text(object, "prefix");
        let timestamp_header = self.optional_text(object, "timestamp_header");
        Some(Some(VerifierInput {
            scheme: scheme?,
            secret_name: secret_name?,
            header: header?,
            algorithm: algorithm?,
            encoding: encoding?,
            prefix: prefix?,
            timestamp_header: timestamp_header?,
        }))
    }

    fn optional_text(&mut self, object: &Map<String, Value>, key: &str) -> Option<Option<String>> {
        match object.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(text)) => Some(Some(text.clone())),
            Some(_) => {
                self.push(
                    join("verifier", key),
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                None
            }
        }
    }

    fn title(&mut self, value: &Value) -> Option<Option<String>> {
        match value {
            Value::Null => Some(None),
            Value::String(text) if text.chars().count() <= TITLE_MAX_CHARS => {
                Some(Some(text.clone()))
            }
            Value::String(_) => {
                self.push(
                    "title",
                    FieldCode::TooLong,
                    format!("must be at most {TITLE_MAX_CHARS} characters"),
                );
                None
            }
            _ => {
                self.push(
                    "title",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                None
            }
        }
    }

    fn overflow(&mut self, value: &Value) -> Option<Overflow> {
        match value.as_str() {
            Some("refuse") => Some(Overflow::Refuse),
            Some("ring") => Some(Overflow::Ring),
            _ => {
                self.push(
                    "overflow",
                    FieldCode::InvalidFormat,
                    "must be refuse or ring",
                );
                None
            }
        }
    }

    fn response(&mut self, object: &Map<String, Value>) -> ResponsePatch {
        self.unknown_keys(object, "response", RESPONSE_KEYS);
        let mut patch = ResponsePatch::default();
        if let Some(value) = object.get("status") {
            patch.status = self.integer(value, "response.status");
        }
        if let Some(value) = object.get("content_type") {
            patch.content_type = self.text(value, "response.content_type").map(str::to_owned);
        }
        if let Some(value) = object.get("headers") {
            patch.headers = self.headers(value);
        }
        if let Some(value) = object.get("body") {
            patch.body = self.body(value);
        }
        if let Some(value) = object.get("delay_ms") {
            patch.delay_ms = self.integer(value, "response.delay_ms");
        }
        if let Some(value) = object.get("templated") {
            patch.templated = self.boolean(value, "response.templated");
        }
        if let Some(value) = object.get("status_template") {
            patch.status_template = self.status_template(value);
        }
        patch
    }

    fn status_template(&mut self, value: &Value) -> Option<Option<String>> {
        match value {
            Value::Null => Some(None),
            Value::String(text) if text.contains('\0') => {
                self.push(
                    "response.status_template",
                    FieldCode::InvalidFormat,
                    "must not contain NUL characters",
                );
                None
            }
            Value::String(text) if text.chars().count() <= STATUS_TEMPLATE_MAX_CHARS => {
                Some(Some(text.clone()))
            }
            Value::String(_) => {
                self.push(
                    "response.status_template",
                    FieldCode::TooLong,
                    format!("must be at most {STATUS_TEMPLATE_MAX_CHARS} characters"),
                );
                None
            }
            _ => {
                self.push(
                    "response.status_template",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                None
            }
        }
    }

    fn headers(&mut self, value: &Value) -> Option<Vec<HeaderInput>> {
        let Some(items) = value.as_array() else {
            self.push(
                "response.headers",
                FieldCode::InvalidFormat,
                "must be an array",
            );
            return None;
        };
        let mut headers = Vec::with_capacity(items.len());
        let mut valid = true;
        for (index, item) in items.iter().enumerate() {
            let path = format!("response.headers.{index}");
            let Some(object) = item.as_object() else {
                self.push(path, FieldCode::InvalidFormat, "must be an object");
                valid = false;
                continue;
            };
            self.unknown_keys(object, &path, HEADER_KEYS);
            let name = self.required_text(object, &path, "name");
            let value = self.required_text(object, &path, "value");
            match (name, value) {
                (Some(name), Some(value)) => headers.push(HeaderInput { name, value }),
                _ => valid = false,
            }
        }
        valid.then_some(headers)
    }

    fn body(&mut self, value: &Value) -> Option<Vec<u8>> {
        let object = self.object(value, "response.body")?;
        self.unknown_keys(object, "response.body", BODY_KEYS);
        let encoding = self.required_text(object, "response.body", "encoding");
        let data = self.required_text(object, "response.body", "data");
        if let Some(truncated) = object.get("truncated") {
            self.boolean(truncated, "response.body.truncated");
        }
        let (encoding, data) = (encoding?, data?);
        match encoding.as_str() {
            "utf8" => Some(data.into_bytes()),
            "base64" => match STANDARD.decode(&data) {
                Ok(bytes) => Some(bytes),
                Err(_) => {
                    self.push(
                        "response.body.data",
                        FieldCode::InvalidFormat,
                        "must be valid base64",
                    );
                    None
                }
            },
            _ => {
                self.push(
                    "response.body.encoding",
                    FieldCode::InvalidFormat,
                    "must be utf8 or base64",
                );
                None
            }
        }
    }

    fn required_text(
        &mut self,
        object: &Map<String, Value>,
        path: &str,
        key: &str,
    ) -> Option<String> {
        let field = join(path, key);
        match object.get(key) {
            None => {
                self.push(field, FieldCode::Required, "is required");
                None
            }
            Some(value) => self.text(value, &field).map(str::to_owned),
        }
    }

    fn object<'a>(&mut self, value: &'a Value, field: &str) -> Option<&'a Map<String, Value>> {
        let object = value.as_object();
        if object.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be an object");
        }
        object
    }

    fn text<'a>(&mut self, value: &'a Value, field: &str) -> Option<&'a str> {
        let text = value.as_str();
        if text.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be a string");
        }
        text
    }

    fn integer(&mut self, value: &Value, field: &str) -> Option<i64> {
        let number = value.as_i64();
        if number.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be an integer");
        }
        number
    }

    fn boolean(&mut self, value: &Value, field: &str) -> Option<bool> {
        let flag = value.as_bool();
        if flag.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be a boolean");
        }
        flag
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}
