use std::net::IpAddr;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_core::capture_id::CaptureId;
use whl_core::client_ip::MAX_HOST_BYTES;
use whl_core::header::is_token;
use whl_core::status::HttpStatus;
use whl_store::CaptureKind;
use whl_store::captures::{Form, FormFile, NameValue};
use whl_store::import::ImportCapture;

use super::ImportLimits;
use super::error::{ImportError, Place, Problem};
use super::scan::MAX_DEPTH;

pub const MAX_METHOD_BYTES: usize = 32;
pub const MAX_PATH_BYTES: usize = 16 * 1024;
pub const MAX_URL_BYTES: usize = 32 * 1024;
pub const MAX_CONTENT_TYPE_BYTES: usize = 1024;
pub const MAX_PAIRS: usize = 256;
pub const MAX_PAIR_NAME_BYTES: usize = 256;
pub const MAX_HEADER_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_PAIR_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_PAIRS_BYTES: usize = 256 * 1024;
pub const MAX_FORM_FILES: usize = 64;
pub const MAX_META_BYTES: usize = 64 * 1024;
const MAX_TIMESTAMP_BYTES: usize = 64;
const MAX_KEY_ECHO_CHARS: usize = 64;

const KNOWN: &[&str] = &[
    "id",
    "seq",
    "kind",
    "received_at",
    "method",
    "path",
    "query_string",
    "url",
    "host",
    "client_ip",
    "content_type",
    "body_size",
    "status_override",
    "response_status",
    "headers",
    "query",
    "form",
    "meta",
    "body",
];
const PAIR_KEYS: &[&str] = &["name", "value"];
const BODY_KEYS: &[&str] = &["encoding", "data", "truncated"];
const FORM_KEYS: &[&str] = &["fields", "files"];
const FILE_KEYS: &[&str] = &["name", "filename", "content_type", "size"];

struct Scope<'a> {
    place: Place,
    limits: &'a ImportLimits,
}

impl Scope<'_> {
    fn fail(&self, path: &str, problem: Problem, text: &str) -> ImportError {
        ImportError::invalid(self.place, path, problem, text)
    }

    fn unknown_keys(
        &self,
        map: &Map<String, Value>,
        prefix: &str,
        known: &[&str],
    ) -> Result<(), ImportError> {
        match map.keys().find(|key| !known.contains(&key.as_str())) {
            None => Ok(()),
            Some(key) => {
                let shown: String = key.chars().take(MAX_KEY_ECHO_CHARS).collect();
                Err(self.fail(
                    &join(prefix, &shown),
                    Problem::NotAllowed,
                    "is not a known field",
                ))
            }
        }
    }

    fn object(&self, value: Value, path: &str) -> Result<Map<String, Value>, ImportError> {
        match value {
            Value::Object(map) => Ok(map),
            _ => Err(self.fail(path, Problem::InvalidFormat, "must be an object")),
        }
    }

    fn array(&self, value: Value, path: &str, max: usize) -> Result<Vec<Value>, ImportError> {
        match value {
            Value::Array(items) if items.len() > max => Err(self.fail(
                path,
                Problem::TooMany,
                &format!("must have at most {max} entries"),
            )),
            Value::Array(items) => Ok(items),
            _ => Err(self.fail(path, Problem::InvalidFormat, "must be an array")),
        }
    }

    fn text(&self, value: Value, path: &str, max: usize) -> Result<String, ImportError> {
        let Value::String(text) = value else {
            return Err(self.fail(path, Problem::InvalidFormat, "must be a string"));
        };
        if text.len() > max {
            return Err(self.fail(
                path,
                Problem::TooLong,
                &format!("must be at most {max} bytes"),
            ));
        }
        if text.contains('\0') {
            return Err(self.fail(path, Problem::InvalidFormat, "must not contain NUL"));
        }
        Ok(text)
    }

    fn plain_text(&self, value: Value, path: &str, max: usize) -> Result<String, ImportError> {
        let text = self.text(value, path, max)?;
        if text.chars().any(char::is_control) {
            return Err(self.fail(
                path,
                Problem::InvalidFormat,
                "must not contain control characters",
            ));
        }
        Ok(text)
    }

    fn optional_text(
        &self,
        value: Option<Value>,
        path: &str,
        max: usize,
    ) -> Result<Option<String>, ImportError> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => self.text(value, path, max).map(Some),
        }
    }

    fn required(&self, value: Option<Value>, path: &str) -> Result<Value, ImportError> {
        value.ok_or_else(|| self.fail(path, Problem::Required, "is required"))
    }

    fn integer(&self, value: &Value, path: &str) -> Result<i64, ImportError> {
        value
            .as_i64()
            .ok_or_else(|| self.fail(path, Problem::InvalidFormat, "must be an integer"))
    }

    fn status(&self, value: &Value, path: &str) -> Result<HttpStatus, ImportError> {
        let code = self.integer(value, path)?;
        HttpStatus::new(code)
            .ok_or_else(|| self.fail(path, Problem::OutOfRange, "must be between 100 and 599"))
    }

    fn timestamp(&self, value: Value, path: &str) -> Result<OffsetDateTime, ImportError> {
        let text = self.text(value, path, MAX_TIMESTAMP_BYTES)?;
        let moment = OffsetDateTime::parse(&text, &Rfc3339).map_err(|_| {
            self.fail(
                path,
                Problem::InvalidFormat,
                "must be an RFC 3339 timestamp",
            )
        })?;
        if self.limits.enforce_time_range
            && (moment < self.limits.earliest || moment > self.limits.latest)
        {
            return Err(self.fail(
                path,
                Problem::OutOfRange,
                "is outside the accepted time range",
            ));
        }
        Ok(moment)
    }

    fn pairs(
        &self,
        value: Value,
        path: &str,
        header_rules: bool,
    ) -> Result<Vec<NameValue>, ImportError> {
        let items = self.array(value, path, MAX_PAIRS)?;
        let mut pairs = Vec::with_capacity(items.len());
        let mut total = 0_usize;
        for (index, item) in items.into_iter().enumerate() {
            let item_path = format!("{path}.{index}");
            let mut map = self.object(item, &item_path)?;
            self.unknown_keys(&map, &item_path, PAIR_KEYS)?;
            let name_path = format!("{item_path}.name");
            let value_path = format!("{item_path}.value");
            let name = self.required(map.remove("name"), &name_path)?;
            let mut name = self.text(name, &name_path, MAX_PAIR_NAME_BYTES)?;
            let value = self.required(map.remove("value"), &value_path)?;
            let max_value = if header_rules {
                MAX_HEADER_VALUE_BYTES
            } else {
                MAX_PAIR_VALUE_BYTES
            };
            let value = self.text(value, &value_path, max_value)?;
            if header_rules {
                if !is_token(&name) {
                    return Err(self.fail(
                        &name_path,
                        Problem::InvalidFormat,
                        "must be a valid HTTP token",
                    ));
                }
                name.make_ascii_lowercase();
                if value.bytes().any(is_control_byte) {
                    return Err(self.fail(
                        &value_path,
                        Problem::InvalidFormat,
                        "must not contain control characters",
                    ));
                }
            }
            total = total.saturating_add(name.len()).saturating_add(value.len());
            if total > MAX_PAIRS_BYTES {
                return Err(self.fail(
                    path,
                    Problem::TooLong,
                    &format!("must be at most {MAX_PAIRS_BYTES} bytes in total"),
                ));
            }
            pairs.push(NameValue { name, value });
        }
        Ok(pairs)
    }

    fn form(&self, value: Value) -> Result<Form, ImportError> {
        let mut map = self.object(value, "form")?;
        self.unknown_keys(&map, "form", FORM_KEYS)?;
        let fields = match map.remove("fields") {
            Some(fields) => self.pairs(fields, "form.fields", false)?,
            None => Vec::new(),
        };
        let files = match map.remove("files") {
            Some(files) => self.array(files, "form.files", MAX_FORM_FILES)?,
            None => Vec::new(),
        };
        let mut parsed = Vec::with_capacity(files.len());
        for (index, item) in files.into_iter().enumerate() {
            let path = format!("form.files.{index}");
            let mut file = self.object(item, &path)?;
            self.unknown_keys(&file, &path, FILE_KEYS)?;
            let name_path = format!("{path}.name");
            let name = self.required(file.remove("name"), &name_path)?;
            let name = self.text(name, &name_path, MAX_PAIR_VALUE_BYTES)?;
            let filename = self.optional_text(
                file.remove("filename"),
                &format!("{path}.filename"),
                MAX_PAIR_VALUE_BYTES,
            )?;
            let content_type = self.optional_text(
                file.remove("content_type"),
                &format!("{path}.content_type"),
                MAX_CONTENT_TYPE_BYTES,
            )?;
            let size_path = format!("{path}.size");
            let size = self.required(file.remove("size"), &size_path)?;
            let size = size.as_u64().ok_or_else(|| {
                self.fail(
                    &size_path,
                    Problem::OutOfRange,
                    "must be a non-negative integer",
                )
            })?;
            parsed.push(FormFile {
                name,
                filename,
                content_type,
                size,
            });
        }
        Ok(Form {
            fields,
            files: parsed,
        })
    }

    fn meta(&self, value: Value) -> Result<Value, ImportError> {
        if !value.is_object() {
            return Err(self.fail("meta", Problem::InvalidFormat, "must be an object"));
        }
        if depth_of(&value) > MAX_DEPTH {
            return Err(self.fail(
                "meta",
                Problem::OutOfRange,
                &format!("must be nested at most {MAX_DEPTH} levels"),
            ));
        }
        let size = serde_json::to_vec(&value).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_META_BYTES {
            return Err(self.fail(
                "meta",
                Problem::TooLong,
                &format!("must be at most {MAX_META_BYTES} bytes"),
            ));
        }
        Ok(value)
    }

    fn body(&self, value: Value) -> Result<Vec<u8>, ImportError> {
        let mut map = self.object(value, "body")?;
        self.unknown_keys(&map, "body", BODY_KEYS)?;
        match map.remove("truncated") {
            None | Some(Value::Bool(false)) => {}
            Some(Value::Bool(true)) => {
                return Err(self.fail(
                    "body.truncated",
                    Problem::NotAllowed,
                    "must be false: a truncated body cannot be imported",
                ));
            }
            Some(_) => {
                return Err(self.fail(
                    "body.truncated",
                    Problem::InvalidFormat,
                    "must be a boolean",
                ));
            }
        }
        let max = usize::try_from(self.limits.max_body_bytes).unwrap_or(usize::MAX);
        let encoding = self.required(map.remove("encoding"), "body.encoding")?;
        let encoding = self.text(encoding, "body.encoding", 16)?;
        let data = self.required(map.remove("data"), "body.data")?;
        let Value::String(data) = data else {
            return Err(self.fail("body.data", Problem::InvalidFormat, "must be a string"));
        };
        let body = match encoding.as_str() {
            "utf8" => data.into_bytes(),
            "base64" => {
                if data.len() / 4 * 3 > max.saturating_add(3) {
                    return Err(self.too_long_body(max));
                }
                STANDARD.decode(data.as_bytes()).map_err(|_| {
                    self.fail("body.data", Problem::InvalidFormat, "must be valid base64")
                })?
            }
            _ => {
                return Err(self.fail(
                    "body.encoding",
                    Problem::InvalidFormat,
                    "must be utf8 or base64",
                ));
            }
        };
        if body.len() > max {
            return Err(self.too_long_body(max));
        }
        Ok(body)
    }

    fn too_long_body(&self, max: usize) -> ImportError {
        self.fail(
            "body.data",
            Problem::TooLong,
            &format!("must decode to at most {max} bytes"),
        )
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}.{name}")
    }
}

fn is_control_byte(byte: u8) -> bool {
    (byte < 0x20 && byte != b'\t') || byte == 0x7f
}

fn depth_of(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth_of).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth_of).max().unwrap_or(0),
        _ => 0,
    }
}

pub fn parse_capture(
    value: Value,
    place: Place,
    limits: &ImportLimits,
) -> Result<ImportCapture, ImportError> {
    let scope = Scope { place, limits };
    let mut map = scope.object(value, "")?;
    scope.unknown_keys(&map, "", KNOWN)?;

    let id = scope.required(map.remove("id"), "id")?;
    let id = scope.text(id, "id", 36)?;
    let original_id = CaptureId::parse(&id).map_err(|_| {
        scope.fail(
            "id",
            Problem::InvalidFormat,
            "must be a lowercase UUID version 7",
        )
    })?;
    if let Some(seq) = map.remove("seq") {
        let seq = scope.integer(&seq, "seq")?;
        if seq < 0 {
            return Err(scope.fail("seq", Problem::OutOfRange, "must not be negative"));
        }
    }
    let kind = scope.required(map.remove("kind"), "kind")?;
    let kind = scope.text(kind, "kind", 16)?;
    let kind = CaptureKind::parse(&kind).ok_or_else(|| {
        scope.fail(
            "kind",
            Problem::InvalidFormat,
            "must be http, email, dns or schedule",
        )
    })?;
    let received_at = scope.required(map.remove("received_at"), "received_at")?;
    let received_at = scope.timestamp(received_at, "received_at")?;

    let method = scope.required(map.remove("method"), "method")?;
    let method = scope.text(method, "method", MAX_METHOD_BYTES)?;
    if !is_token(&method) {
        return Err(scope.fail(
            "method",
            Problem::InvalidFormat,
            "must be a valid HTTP token",
        ));
    }
    let path = scope.required(map.remove("path"), "path")?;
    let path = scope.plain_text(path, "path", MAX_PATH_BYTES)?;
    let query_string = match map.remove("query_string") {
        None | Some(Value::Null) => String::new(),
        Some(value) => scope.plain_text(value, "query_string", MAX_PATH_BYTES)?,
    };
    let url = scope.required(map.remove("url"), "url")?;
    let url = scope.plain_text(url, "url", MAX_URL_BYTES)?;
    let host = scope.required(map.remove("host"), "host")?;
    let host = scope.plain_text(host, "host", MAX_HOST_BYTES)?;
    let client_ip = scope.required(map.remove("client_ip"), "client_ip")?;
    let client_ip = scope.text(client_ip, "client_ip", 64)?;
    let client_ip = IpAddr::from_str(&client_ip)
        .map_err(|_| {
            scope.fail(
                "client_ip",
                Problem::InvalidFormat,
                "must be an IPv4 or IPv6 address",
            )
        })?
        .to_canonical();
    let content_type = scope.optional_text(
        map.remove("content_type"),
        "content_type",
        MAX_CONTENT_TYPE_BYTES,
    )?;
    if content_type
        .as_deref()
        .is_some_and(|text| text.chars().any(char::is_control))
    {
        return Err(scope.fail(
            "content_type",
            Problem::InvalidFormat,
            "must not contain control characters",
        ));
    }
    let status_override = match map.remove("status_override") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.status(&value, "status_override")?),
    };
    let response_status = scope.required(map.remove("response_status"), "response_status")?;
    let response_status = scope.status(&response_status, "response_status")?;

    let headers = scope.required(map.remove("headers"), "headers")?;
    let headers = scope.pairs(headers, "headers", true)?;
    let query = match map.remove("query") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => scope.pairs(value, "query", false)?,
    };
    let form = match map.remove("form") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.form(value)?),
    };
    let meta = match map.remove("meta") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(value) => scope.meta(value)?,
    };
    let declared = map.remove("body_size");
    let body = scope.required(map.remove("body"), "body")?;
    let body = scope.body(body)?;
    if let Some(declared) = declared {
        let declared = scope.integer(&declared, "body_size")?;
        if usize::try_from(declared).ok() != Some(body.len()) {
            return Err(scope.fail(
                "body_size",
                Problem::InvalidFormat,
                "must equal the length of the body",
            ));
        }
    }
    Ok(ImportCapture {
        original_id,
        received_at,
        kind,
        method,
        url,
        path,
        query_string,
        host,
        client_ip,
        headers,
        query,
        form,
        content_type,
        body,
        status_override,
        response_status,
        meta,
    })
}
