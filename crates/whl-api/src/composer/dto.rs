use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use whl_forward::ResponseHeader;
use whl_store::Uuid;
use whl_store::composer::{MAX_STORED_RESPONSE_BODY_BYTES, SentRecord};

use super::input::SendInput;
use super::outbound::Executed;
use super::redact::{PathPolicy, redact_body, redact_header_value, redact_url};
use crate::dto::{BodyDto, BodyInputDoc, NameValueDto};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderSnapshot {
    pub name: String,
    pub value: String,
}

impl From<&ResponseHeader> for HeaderSnapshot {
    fn from(header: &ResponseHeader) -> Self {
        Self {
            name: header.name.as_str().to_owned(),
            value: header.value.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResultSnapshot {
    pub status: Option<u16>,
    pub headers: Vec<HeaderSnapshot>,
    pub duration_ms: u64,
    pub final_url: Option<String>,
    pub error: Option<String>,
    pub capture_id: Option<String>,
    pub body_size: usize,
    pub body_truncated: bool,
}

#[derive(Serialize, ToSchema)]
#[schema(as = SendResult)]
pub struct SendResultDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = 200)]
    status: Option<u16>,
    #[schema(value_type = Vec<NameValueDto>)]
    headers: Vec<HeaderSnapshot>,
    body: Option<BodyDto>,
    #[schema(example = 42)]
    duration_ms: u64,
    #[schema(example = "https://hooks.example.com/incoming")]
    final_url: Option<String>,
    #[schema(example = "connect_timeout: the connection took too long")]
    error: Option<String>,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d71"
    )]
    capture_id: Option<String>,
}

impl SendResultDto {
    pub fn new(id: Uuid, executed: &Executed) -> Self {
        let body = executed
            .status
            .map(|_| BodyDto::from_prefix(&executed.body, executed.truncated));
        Self {
            id: id.hyphenated().to_string(),
            status: executed.status,
            headers: executed.headers.iter().map(HeaderSnapshot::from).collect(),
            body,
            duration_ms: executed.duration_ms(),
            final_url: executed.final_url.clone(),
            error: executed.error.clone(),
            capture_id: executed.capture_id.map(|id| id.hyphenated().to_string()),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = SentRequest)]
pub struct SentRequestDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = 200)]
    status: Option<u16>,
    #[schema(value_type = Vec<NameValueDto>)]
    headers: Vec<HeaderSnapshot>,
    body: Option<BodyDto>,
    #[schema(example = 42)]
    duration_ms: u64,
    #[schema(example = "https://hooks.example.com/incoming")]
    final_url: Option<String>,
    #[schema(example = "connect_timeout: the connection took too long")]
    error: Option<String>,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d71"
    )]
    capture_id: Option<String>,
    #[schema(value_type = RequestSnapshot)]
    request: Value,
    #[schema(format = DateTime)]
    created_at: String,
}

impl SentRequestDto {
    pub fn from_record(record: &SentRecord) -> Self {
        let result: Option<ResultSnapshot> = serde_json::from_value(record.result.clone()).ok();
        let (status, headers, duration_ms, final_url, error, capture_id, truncated) = match result {
            Some(result) => (
                result.status,
                result.headers,
                result.duration_ms,
                result.final_url,
                result.error,
                result.capture_id,
                result.body_truncated,
            ),
            None => (None, Vec::new(), 0, None, None, None, false),
        };
        let body = record
            .response_body
            .as_deref()
            .map(|bytes| BodyDto::from_prefix(bytes, truncated));
        Self {
            id: record.id.hyphenated().to_string(),
            status,
            headers,
            body,
            duration_ms,
            final_url,
            error,
            capture_id,
            request: record.request.clone(),
            created_at: record.created_at.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = SentRequestList)]
pub struct HistoryDto {
    pub items: Vec<SentRequestDto>,
    #[schema(example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70")]
    pub next_cursor: Option<String>,
}

#[derive(ToSchema)]
#[schema(as = SendRequest)]
#[allow(dead_code)]
pub struct SendRequestDoc {
    #[schema(example = "POST")]
    method: String,
    #[schema(max_length = 2048, example = "https://hooks.example.com/incoming")]
    url: String,
    #[schema(max_items = 50)]
    headers: Option<Vec<NameValueDto>>,
    body: Option<BodyInputDoc>,
    #[schema(minimum = 1, maximum = 15000, default = 15000)]
    timeout_ms: Option<u64>,
    #[schema(default = false)]
    follow_redirects: Option<bool>,
    #[schema(min_length = 24, max_length = 24, pattern = "^[0-9a-hjkmnp-tv-z]{24}$")]
    endpoint_id: Option<String>,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    source_capture_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = SentRequestSnapshot)]
struct RequestSnapshot {
    #[schema(example = "POST")]
    method: String,
    #[schema(example = "https://hooks.example.com/incoming")]
    url: String,
    #[schema(value_type = Vec<NameValueDto>)]
    headers: Vec<HeaderSnapshot>,
    body: Option<BodyDto>,
    #[schema(example = 12)]
    body_size: usize,
    #[schema(example = 15000)]
    timeout_ms: u64,
    follow_redirects: bool,
    #[schema(min_length = 24, max_length = 24, pattern = "^[0-9a-hjkmnp-tv-z]{24}$")]
    endpoint_id: Option<String>,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    source_capture_id: Option<String>,
}

pub struct StoredEntry {
    pub request: Value,
    pub result: Value,
    pub response_body: Option<Vec<u8>>,
}

impl StoredEntry {
    pub fn build(
        input: &SendInput,
        executed: &Executed,
        policy: PathPolicy,
    ) -> Result<Self, serde_json::Error> {
        let request_headers: Vec<HeaderSnapshot> = input
            .headers
            .iter()
            .map(|header| HeaderSnapshot {
                name: header.name.as_str().to_owned(),
                value: redact_header_value(
                    header.name.as_str(),
                    &String::from_utf8_lossy(header.value.as_bytes()),
                    policy,
                ),
            })
            .collect();
        let request_body = redact_body(
            input.header_text("content-type"),
            &input.body,
            MAX_STORED_RESPONSE_BODY_BYTES,
            policy,
        )
        .map(|redacted| BodyDto::from_prefix(&redacted.bytes, redacted.capped));
        let request = RequestSnapshot {
            method: input.method.as_str().to_owned(),
            url: redact_url(input.url.as_str(), policy),
            headers: request_headers,
            body: request_body,
            body_size: input.body.len(),
            timeout_ms: u64::try_from(input.timeout.as_millis()).unwrap_or(u64::MAX),
            follow_redirects: input.follow_redirects,
            endpoint_id: input.endpoint.as_ref().map(|id| id.unpack().to_owned()),
            source_capture_id: input.source_capture.map(|id| id.hyphenated().to_string()),
        };
        let content_type = executed
            .headers
            .iter()
            .find(|header| header.name == axum::http::header::CONTENT_TYPE)
            .map(|header| header.value.as_str());
        let redacted_response = redact_body(
            content_type,
            &executed.body,
            MAX_STORED_RESPONSE_BODY_BYTES,
            policy,
        );
        let truncated = executed.truncated
            || redacted_response
                .as_ref()
                .is_some_and(|redacted| redacted.capped);
        let response_body = redacted_response.map(|redacted| redacted.bytes);
        let result = ResultSnapshot {
            status: executed.status,
            headers: executed
                .headers
                .iter()
                .map(|header| HeaderSnapshot {
                    name: header.name.as_str().to_owned(),
                    value: redact_header_value(header.name.as_str(), &header.value, policy),
                })
                .collect(),
            duration_ms: executed.duration_ms(),
            final_url: executed
                .final_url
                .as_deref()
                .map(|url| redact_url(url, policy)),
            error: executed.error.clone(),
            capture_id: executed.capture_id.map(|id| id.hyphenated().to_string()),
            body_size: executed.body.len(),
            body_truncated: truncated,
        };
        Ok(Self {
            request: serde_json::to_value(request)?,
            result: serde_json::to_value(result)?,
            response_body,
        })
    }
}
