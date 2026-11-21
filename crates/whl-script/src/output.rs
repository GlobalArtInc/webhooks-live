use std::time::Duration;

use rhai::{Array, Dynamic, ImmutableString, Map};
use serde_json::Value;

use whl_core::content_type::DEFAULT_CONTENT_TYPE;
pub use whl_core::content_type::MAX_CONTENT_TYPE_BYTES;
use whl_core::header::HeaderInput;
pub use whl_core::header::MAX_HEADERS as MAX_RESPONSE_HEADERS;
pub use whl_core::header::{MAX_HEADER_NAME_BYTES, MAX_HEADER_VALUE_BYTES};
use whl_core::response::{ResponseConfig, ResponseConfigInput};

use crate::convert::{BoundedWriter, ConvertLimits, dynamic_to_json};
use crate::error::{ResponseViolation, ScriptError};
use crate::limits::{EngineLimits, RunLimits};

const DEFAULT_STATUS: i64 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptResponse {
    pub status: Option<u16>,
    pub headers: Vec<ResponseHeader>,
    pub body: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScriptLog {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    pub operations: u64,
    pub peak_memory_bytes: usize,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptOutput {
    pub response: Option<ScriptResponse>,
    pub vars: serde_json::Map<String, Value>,
    pub log: ScriptLog,
    pub stats: RunStats,
}

#[derive(Debug)]
pub(crate) struct LogBuffer {
    text: String,
    limit: usize,
    truncated: bool,
}

impl LogBuffer {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    pub(crate) fn push_line(&mut self, line: &str) {
        if self.truncated {
            return;
        }
        let room = self.limit.saturating_sub(self.text.len());
        let needed = line.len().saturating_add(1);
        if needed <= room {
            self.text.push_str(line);
            self.text.push('\n');
            return;
        }
        let mut end = room.min(line.len());
        while end > 0 && !line.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(prefix) = line.get(..end) {
            self.text.push_str(prefix);
        }
        self.truncated = true;
    }

    pub(crate) fn finish(&mut self) -> ScriptLog {
        ScriptLog {
            text: std::mem::take(&mut self.text),
            truncated: self.truncated,
        }
    }
}

fn invalid(message: impl Into<String>) -> ScriptError {
    ScriptError::InvalidResult(message.into())
}

fn json_limits(engine: &EngineLimits, run: &RunLimits) -> ConvertLimits {
    ConvertLimits {
        max_depth: 32,
        max_nodes: 4096,
        max_bytes: run.max_vars_bytes.saturating_mul(2),
        max_string_bytes: engine.max_string_bytes.max(run.max_response_body_bytes),
        max_array_len: engine.max_array_size,
        max_map_len: engine.max_map_size,
        max_key_bytes: engine.max_map_key_bytes,
    }
}

fn as_map(value: &Dynamic, what: &str) -> Result<Map, ScriptError> {
    value
        .read_lock::<Map>()
        .map(|map| map.clone())
        .ok_or_else(|| invalid(format!("{what} must be a map")))
}

fn as_text(value: &Dynamic, what: &str, max: usize) -> Result<String, ScriptError> {
    let text = value
        .read_lock::<ImmutableString>()
        .map(|text| text.as_str().to_owned())
        .ok_or_else(|| invalid(format!("{what} must be a string")))?;
    if text.len() > max {
        return Err(invalid(format!("{what} is longer than {max} bytes")));
    }
    Ok(text)
}

struct RawResponse {
    status: Option<i64>,
    headers: Vec<ResponseHeader>,
    body: Option<String>,
    content_type: Option<String>,
}

fn extract_headers(value: &Dynamic) -> Result<Vec<ResponseHeader>, ScriptError> {
    let mut headers = Vec::new();
    if let Some(map) = value.read_lock::<Map>() {
        for (name, item) in map.iter() {
            headers.push(ResponseHeader {
                name: name.to_string(),
                value: as_text(item, "response header value", usize::MAX)?,
            });
        }
    } else if let Some(array) = value.read_lock::<Array>() {
        for item in array.iter() {
            let entry = as_map(item, "response header")?;
            let name = entry
                .get("name")
                .ok_or_else(|| invalid("response header needs a name"))?;
            let text = entry
                .get("value")
                .ok_or_else(|| invalid("response header needs a value"))?;
            headers.push(ResponseHeader {
                name: as_text(name, "response header name", usize::MAX)?,
                value: as_text(text, "response header value", usize::MAX)?,
            });
        }
    } else {
        return Err(invalid("response headers must be a map or an array"));
    }
    Ok(headers)
}

fn violations_of(error: &whl_core::validation::ValidationError) -> Vec<ResponseViolation> {
    error
        .violations()
        .iter()
        .map(|violation| ResponseViolation {
            field: violation.field.to_string(),
            code: violation.code.as_str(),
            message: violation.code.to_string(),
        })
        .collect()
}

fn validate_response(raw: &RawResponse) -> Result<(), ScriptError> {
    let input = ResponseConfigInput {
        status: raw.status.unwrap_or(DEFAULT_STATUS),
        content_type: raw
            .content_type
            .clone()
            .unwrap_or_else(|| DEFAULT_CONTENT_TYPE.to_owned()),
        headers: raw
            .headers
            .iter()
            .map(|header| HeaderInput {
                name: header.name.clone(),
                value: header.value.clone(),
            })
            .collect(),
        body: raw.body.clone().unwrap_or_default().into_bytes(),
        delay_ms: 0,
    };
    ResponseConfig::validate(input)
        .map(|_| ())
        .map_err(|error| ScriptError::InvalidResponse(violations_of(&error)))
}

fn extract_response(
    value: &Dynamic,
    run: &RunLimits,
) -> Result<Option<ScriptResponse>, ScriptError> {
    if value.is_unit() {
        return Ok(None);
    }
    let map = as_map(value, "response")?;
    let mut raw = RawResponse {
        status: None,
        headers: Vec::new(),
        body: None,
        content_type: None,
    };
    for (key, item) in &map {
        match key.as_str() {
            "status" => {
                raw.status = Some(
                    item.as_int()
                        .map_err(|_| invalid("response status must be an integer"))?,
                );
            }
            "headers" => raw.headers = extract_headers(item)?,
            "body" => {
                raw.body = Some(as_text(item, "response body", run.max_response_body_bytes)?);
            }
            "content_type" => {
                raw.content_type = Some(as_text(item, "response content_type", usize::MAX)?);
            }
            other => {
                return Err(invalid(format!("unknown response field {other}")));
            }
        }
    }
    validate_response(&raw)?;
    Ok(Some(ScriptResponse {
        status: raw.status.and_then(|status| u16::try_from(status).ok()),
        headers: raw.headers,
        body: raw.body,
        content_type: raw.content_type,
    }))
}

fn extract_vars(
    value: &Dynamic,
    engine: &EngineLimits,
    run: &RunLimits,
) -> Result<serde_json::Map<String, Value>, ScriptError> {
    let json = dynamic_to_json(value, json_limits(engine, run))
        .map_err(|error| invalid(format!("vars cannot be returned: {error}")))?;
    let Value::Object(entries) = json else {
        return Err(invalid("vars must be a map"));
    };
    let mut writer = BoundedWriter::new(run.max_vars_bytes);
    serde_json::to_writer(&mut writer, &entries)
        .map_err(|_| invalid(format!("vars are larger than {} bytes", run.max_vars_bytes)))?;
    Ok(entries)
}

pub(crate) struct Extracted {
    pub response: Option<ScriptResponse>,
    pub vars: serde_json::Map<String, Value>,
}

pub(crate) fn extract(
    returned: &Dynamic,
    scope_vars: Option<&Dynamic>,
    engine: &EngineLimits,
    run: &RunLimits,
) -> Result<Extracted, ScriptError> {
    let mut response = None;
    let mut returned_vars: Option<&Dynamic> = None;
    let map_holder;
    if !returned.is_unit() {
        map_holder = as_map(returned, "the script result")?;
        for (key, item) in &map_holder {
            match key.as_str() {
                "response" => response = extract_response(item, run)?,
                "vars" => returned_vars = Some(item),
                other => {
                    return Err(invalid(format!("unknown result field {other}")));
                }
            }
        }
    }
    let source = returned_vars.or(scope_vars);
    let vars = match source {
        Some(value) => extract_vars(value, engine, run)?,
        None => serde_json::Map::new(),
    };
    Ok(Extracted { response, vars })
}
