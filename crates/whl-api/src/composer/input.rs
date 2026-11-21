use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderName, HeaderValue, Method};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use url::Url;
use whl_core::endpoint_id::EndpointId;
use whl_core::header::{MAX_HEADER_NAME_BYTES, is_token};
use whl_store::Uuid;

use super::settings::{MAX_REQUEST_BODY_BYTES, MAX_TIMEOUT_MS};
use super::target::parse_url;
use crate::error::{ApiError, FieldCode, FieldViolation};

pub const MAX_HEADERS: usize = 50;
pub const MAX_HEADER_VALUE_BYTES: usize = 4096;
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const RESERVED_PREFIX: &str = "x-webhooks-live-";

const KEYS: &[&str] = &[
    "method",
    "url",
    "headers",
    "body",
    "timeout_ms",
    "follow_redirects",
    "endpoint_id",
    "source_capture_id",
];
const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
const DENIED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "trailer",
    "te",
    "expect",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-forwarded-port",
    "x-real-ip",
    "x-client-ip",
    "true-client-ip",
    "cf-connecting-ip",
    "x-original-forwarded-for",
];
const DENIED_PREFIXES: &[&str] = &["proxy-", RESERVED_PREFIX];

#[derive(Debug)]
pub struct InputHeader {
    pub name: HeaderName,
    pub value: HeaderValue,
}

#[derive(Debug)]
pub struct SendInput {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<InputHeader>,
    pub body: Bytes,
    pub timeout: Duration,
    pub follow_redirects: bool,
    pub endpoint: Option<EndpointId>,
    pub source_capture: Option<Uuid>,
}

impl SendInput {
    pub fn header_text(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.as_str() == name)
            .and_then(|header| header.value.to_str().ok())
    }
}

pub fn parse_send(bytes: &[u8]) -> Result<SendInput, ApiError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut walk = Walk::default();
    for key in object.keys() {
        if !KEYS.contains(&key.as_str()) {
            walk.push(key, FieldCode::NotAllowed, "is not a known field");
        }
    }
    let method = walk.method(&object);
    let url = walk.url(&object);
    let headers = walk.headers(&object);
    let body = walk.body(&object);
    let timeout = walk.timeout(&object);
    let follow_redirects = walk.follow(&object);
    let endpoint = walk.endpoint(&object);
    let source_capture = walk.source_capture(&object);
    if !walk.violations.is_empty() {
        return Err(ApiError::Validation(walk.violations));
    }
    match (method, url) {
        (Some(method), Some(url)) => Ok(SendInput {
            method,
            url,
            headers,
            body,
            timeout,
            follow_redirects,
            endpoint,
            source_capture,
        }),
        _ => Err(ApiError::Validation(walk.violations)),
    }
}

#[derive(Default)]
struct Walk {
    violations: Vec<FieldViolation>,
}

impl Walk {
    fn push(&mut self, field: &str, code: FieldCode, message: &str) {
        self.violations
            .push(FieldViolation::new(field, code, message));
    }

    fn present<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
        object.get(key).filter(|value| !value.is_null())
    }

    fn method(&mut self, object: &Map<String, Value>) -> Option<Method> {
        let Some(value) = Self::present(object, "method") else {
            self.push("method", FieldCode::Required, "is required");
            return None;
        };
        let Some(text) = value.as_str() else {
            self.push("method", FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        let upper = text.to_ascii_uppercase();
        if !METHODS.contains(&upper.as_str()) {
            self.push(
                "method",
                FieldCode::NotAllowed,
                "must be GET, HEAD, POST, PUT, PATCH, DELETE or OPTIONS",
            );
            return None;
        }
        Method::from_bytes(upper.as_bytes()).ok()
    }

    fn url(&mut self, object: &Map<String, Value>) -> Option<Url> {
        let Some(value) = Self::present(object, "url") else {
            self.push("url", FieldCode::Required, "is required");
            return None;
        };
        let Some(text) = value.as_str() else {
            self.push("url", FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        match parse_url(text) {
            Ok(url) => Some(url),
            Err(violation) => {
                self.violations.push(violation);
                None
            }
        }
    }

    fn headers(&mut self, object: &Map<String, Value>) -> Vec<InputHeader> {
        let Some(value) = Self::present(object, "headers") else {
            return Vec::new();
        };
        let Some(items) = value.as_array() else {
            self.push("headers", FieldCode::InvalidFormat, "must be an array");
            return Vec::new();
        };
        if items.len() > MAX_HEADERS {
            self.push("headers", FieldCode::TooMany, "has more than 50 entries");
            return Vec::new();
        }
        let mut parsed = Vec::with_capacity(items.len());
        let mut total = 0_usize;
        for (index, item) in items.iter().enumerate() {
            if let Some(header) = self.header(index, item) {
                total += header.name.as_str().len() + header.value.len();
                parsed.push(header);
            }
        }
        if total > MAX_HEADER_BYTES {
            self.push("headers", FieldCode::TooLong, "are longer than 16 KiB");
        }
        parsed
    }

    fn header(&mut self, index: usize, item: &Value) -> Option<InputHeader> {
        let name_field = format!("headers.{index}.name");
        let value_field = format!("headers.{index}.value");
        let Some(entry) = item.as_object() else {
            self.push(
                &format!("headers.{index}"),
                FieldCode::InvalidFormat,
                "must be an object with name and value",
            );
            return None;
        };
        let name = entry.get("name").and_then(Value::as_str);
        let value = entry.get("value").and_then(Value::as_str);
        let (Some(name), Some(value)) = (name, value) else {
            self.push(
                &format!("headers.{index}"),
                FieldCode::Required,
                "needs a string name and a string value",
            );
            return None;
        };
        let lowered = name.to_ascii_lowercase();
        if name.len() > MAX_HEADER_NAME_BYTES || !is_token(name) {
            self.push(
                &name_field,
                FieldCode::InvalidFormat,
                "must be an HTTP token of at most 64 bytes",
            );
            return None;
        }
        if DENIED_HEADERS.contains(&lowered.as_str())
            || DENIED_PREFIXES
                .iter()
                .any(|prefix| lowered.starts_with(prefix))
        {
            self.push(
                &name_field,
                FieldCode::NotAllowed,
                "is managed by the server",
            );
            return None;
        }
        if value.len() > MAX_HEADER_VALUE_BYTES
            || !value.is_ascii()
            || value.bytes().any(is_control)
        {
            self.push(
                &value_field,
                FieldCode::InvalidFormat,
                "must be visible ASCII, at most 4096 bytes, without control characters",
            );
            return None;
        }
        match (
            HeaderName::from_bytes(lowered.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            (Ok(name), Ok(value)) => Some(InputHeader { name, value }),
            _ => {
                self.push(
                    &name_field,
                    FieldCode::InvalidFormat,
                    "is not a valid header",
                );
                None
            }
        }
    }

    fn body(&mut self, object: &Map<String, Value>) -> Bytes {
        let Some(value) = Self::present(object, "body") else {
            return Bytes::new();
        };
        let Some(body) = value.as_object() else {
            self.push(
                "body",
                FieldCode::InvalidFormat,
                "must be a Body object or null",
            );
            return Bytes::new();
        };
        let encoding = body.get("encoding").and_then(Value::as_str);
        let data = body.get("data").and_then(Value::as_str);
        let (Some(encoding), Some(data)) = (encoding, data) else {
            self.push(
                "body",
                FieldCode::Required,
                "needs a string encoding and a string data",
            );
            return Bytes::new();
        };
        match encoding {
            "utf8" if data.len() <= MAX_REQUEST_BODY_BYTES => {
                Bytes::copy_from_slice(data.as_bytes())
            }
            "base64" if data.len() <= MAX_REQUEST_BODY_BYTES / 3 * 4 + 4 => {
                match STANDARD.decode(data) {
                    Ok(decoded) if decoded.len() <= MAX_REQUEST_BODY_BYTES => Bytes::from(decoded),
                    Ok(_) => self.too_large(),
                    Err(_) => {
                        self.push("body.data", FieldCode::InvalidFormat, "is not valid base64");
                        Bytes::new()
                    }
                }
            }
            "utf8" | "base64" => self.too_large(),
            _ => {
                self.push(
                    "body.encoding",
                    FieldCode::InvalidFormat,
                    "must be utf8 or base64",
                );
                Bytes::new()
            }
        }
    }

    fn too_large(&mut self) -> Bytes {
        self.push("body.data", FieldCode::TooLong, "is larger than 1 MiB");
        Bytes::new()
    }

    fn timeout(&mut self, object: &Map<String, Value>) -> Duration {
        let Some(value) = Self::present(object, "timeout_ms") else {
            return Duration::from_millis(MAX_TIMEOUT_MS);
        };
        match value.as_u64() {
            Some(millis) if (1..=MAX_TIMEOUT_MS).contains(&millis) => Duration::from_millis(millis),
            Some(_) => {
                self.push("timeout_ms", FieldCode::OutOfRange, "must be 1 to 15000");
                Duration::from_millis(MAX_TIMEOUT_MS)
            }
            None => {
                self.push("timeout_ms", FieldCode::InvalidFormat, "must be an integer");
                Duration::from_millis(MAX_TIMEOUT_MS)
            }
        }
    }

    fn follow(&mut self, object: &Map<String, Value>) -> bool {
        match Self::present(object, "follow_redirects") {
            None => false,
            Some(Value::Bool(flag)) => *flag,
            Some(_) => {
                self.push(
                    "follow_redirects",
                    FieldCode::InvalidFormat,
                    "must be a boolean",
                );
                false
            }
        }
    }

    fn endpoint(&mut self, object: &Map<String, Value>) -> Option<EndpointId> {
        let value = Self::present(object, "endpoint_id")?;
        match value.as_str().map(EndpointId::parse) {
            Some(Ok(id)) => Some(id),
            _ => {
                self.push(
                    "endpoint_id",
                    FieldCode::InvalidFormat,
                    "is not an endpoint id",
                );
                None
            }
        }
    }

    fn source_capture(&mut self, object: &Map<String, Value>) -> Option<Uuid> {
        let value = Self::present(object, "source_capture_id")?;
        let parsed = value
            .as_str()
            .and_then(|text| Uuid::try_parse(text).ok().map(|id| (id, text)))
            .filter(|(id, text)| id.hyphenated().to_string() == *text)
            .map(|(id, _)| id);
        if parsed.is_none() {
            self.push(
                "source_capture_id",
                FieldCode::InvalidFormat,
                "must be a lower-case hyphenated UUID",
            );
        }
        parsed
    }
}

fn is_control(byte: u8) -> bool {
    (byte < 0x20 && byte != b'\t') || byte == 0x7f
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: &Value) -> Result<SendInput, ApiError> {
        parse_send(value.to_string().as_bytes())
    }

    fn codes(error: ApiError) -> Vec<(String, FieldCode)> {
        match error {
            ApiError::Validation(violations) => violations
                .iter()
                .map(|violation| (violation.field().to_owned(), violation.code()))
                .collect(),
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_minimal_request_gets_the_defaults() {
        let input = parse(&json!({"method": "get", "url": "https://example.com/x"}))
            .expect("a minimal request is valid");
        assert_eq!(input.method, Method::GET);
        assert_eq!(input.timeout, Duration::from_millis(15_000));
        assert!(!input.follow_redirects);
        assert!(input.body.is_empty() && input.headers.is_empty());
    }

    #[test]
    fn managed_and_injected_headers_are_refused() {
        for (name, value) in [
            ("Host", "evil.test"),
            ("Content-Length", "5"),
            ("Transfer-Encoding", "chunked"),
            ("X-Forwarded-For", "10.0.0.1"),
            ("X-Webhooks-Live-Hop", "0"),
            ("X-Webhooks-Live-Composer", "1"),
            ("Proxy-Authorization", "x"),
            ("X-A", "a\r\nX-B: b"),
            ("X-A", "caf\u{e9}"),
            ("X A", "v"),
        ] {
            let error = parse(&json!({
                "method": "POST",
                "url": "https://example.com/x",
                "headers": [{"name": name, "value": value}],
            }))
            .expect_err(name);
            assert_eq!(codes(error).len(), 1, "{name}");
        }
    }

    #[test]
    fn bodies_are_decoded_and_capped() {
        let ok = parse(&json!({
            "method": "POST",
            "url": "https://example.com/x",
            "body": {"encoding": "base64", "data": "aGk="},
        }))
        .expect("base64 decodes");
        assert_eq!(ok.body.as_ref(), b"hi");
        let big = "a".repeat(MAX_REQUEST_BODY_BYTES + 1);
        for body in [
            json!({"encoding": "utf8", "data": big}),
            json!({"encoding": "base64", "data": "***"}),
            json!({"encoding": "latin1", "data": "x"}),
        ] {
            let error =
                parse(&json!({"method": "POST", "url": "https://example.com/x", "body": body}))
                    .expect_err("a bad body is refused");
            assert_eq!(codes(error).len(), 1);
        }
    }

    #[test]
    fn limits_and_unknown_fields_are_refused() {
        let error = parse(&json!({
            "method": "TRACE",
            "url": "not a url",
            "timeout_ms": 15001,
            "follow_redirects": "yes",
            "endpoint_id": "short",
            "source_capture_id": "NOT-A-UUID",
            "extra": 1,
        }))
        .expect_err("every field is wrong");
        let fields: Vec<String> = codes(error).into_iter().map(|(field, _)| field).collect();
        assert_eq!(
            fields,
            [
                "extra",
                "method",
                "url",
                "timeout_ms",
                "follow_redirects",
                "endpoint_id",
                "source_capture_id"
            ]
        );
    }
}
