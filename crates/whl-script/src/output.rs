use std::time::Duration;

use rhai::{Array, Dynamic, ImmutableString, Map};
use serde_json::Value;

use crate::convert::{BoundedWriter, ConvertLimits, dynamic_to_json};
use crate::error::ScriptError;
use crate::limits::{EngineLimits, RunLimits};

pub const MAX_RESPONSE_HEADERS: usize = 20;
pub const MAX_HEADER_NAME_BYTES: usize = 128;
pub const MAX_HEADER_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_CONTENT_TYPE_BYTES: usize = 256;

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

fn forbidden_in_header(text: &str) -> bool {
    text.chars()
        .any(|character| character == '\r' || character == '\n' || character == '\0')
}

fn header(name: String, value: String) -> Result<ResponseHeader, ScriptError> {
    if name.is_empty()
        || name.len() > MAX_HEADER_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b':')
    {
        return Err(invalid("response header name is not a valid token"));
    }
    if value.len() > MAX_HEADER_VALUE_BYTES || forbidden_in_header(&value) {
        return Err(invalid("response header value is invalid"));
    }
    Ok(ResponseHeader { name, value })
}

fn extract_headers(value: &Dynamic) -> Result<Vec<ResponseHeader>, ScriptError> {
    let mut headers = Vec::new();
    if let Some(map) = value.read_lock::<Map>() {
        for (name, item) in map.iter() {
            let text = as_text(item, "response header value", MAX_HEADER_VALUE_BYTES)?;
            headers.push(header(name.to_string(), text)?);
            if headers.len() > MAX_RESPONSE_HEADERS {
                break;
            }
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
            headers.push(header(
                as_text(name, "response header name", MAX_HEADER_NAME_BYTES)?,
                as_text(text, "response header value", MAX_HEADER_VALUE_BYTES)?,
            )?);
            if headers.len() > MAX_RESPONSE_HEADERS {
                break;
            }
        }
    } else {
        return Err(invalid("response headers must be a map or an array"));
    }
    if headers.len() > MAX_RESPONSE_HEADERS {
        return Err(invalid(format!(
            "response has more than {MAX_RESPONSE_HEADERS} headers"
        )));
    }
    Ok(headers)
}

fn extract_response(
    value: &Dynamic,
    run: &RunLimits,
) -> Result<Option<ScriptResponse>, ScriptError> {
    if value.is_unit() {
        return Ok(None);
    }
    let map = as_map(value, "response")?;
    let mut response = ScriptResponse::default();
    for (key, item) in &map {
        match key.as_str() {
            "status" => {
                let status = item
                    .as_int()
                    .map_err(|_| invalid("response status must be an integer"))?;
                if !(100..=599).contains(&status) {
                    return Err(invalid("response status must be between 100 and 599"));
                }
                response.status = u16::try_from(status).ok();
            }
            "headers" => response.headers = extract_headers(item)?,
            "body" => {
                response.body = Some(as_text(item, "response body", run.max_response_body_bytes)?);
            }
            "content_type" => {
                let text = as_text(item, "response content_type", MAX_CONTENT_TYPE_BYTES)?;
                if forbidden_in_header(&text) {
                    return Err(invalid("response content_type is invalid"));
                }
                response.content_type = Some(text);
            }
            other => {
                return Err(invalid(format!("unknown response field {other}")));
            }
        }
    }
    Ok(Some(response))
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
