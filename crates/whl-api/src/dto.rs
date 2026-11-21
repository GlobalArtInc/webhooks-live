use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use whl_core::header::MAX_HEADERS;
use whl_core::response::{MAX_BODY_BYTES as RESPONSE_BODY_BYTES, MAX_DELAY_MS};
use whl_store::EndpointRecord;

use crate::settings::ApiSettings;

const MAX_FORWARD_RULES: u32 = 5;

#[derive(Serialize)]
pub struct InstanceDto {
    version: &'static str,
    auth_mode: &'static str,
    user: Option<serde_json::Value>,
    public_url: String,
    auth: AuthDto,
    limits: LimitsDto,
    features: FeaturesDto,
}

#[derive(Serialize)]
struct AuthDto {
    provider_name: Option<String>,
}

#[derive(Serialize)]
struct LimitsDto {
    max_captures: u32,
    max_body_bytes: u64,
    retention_seconds: u64,
    max_delay_ms: i64,
    max_forward_rules: u32,
    max_response_body_bytes: usize,
    max_response_headers: usize,
}

#[derive(Serialize)]
struct FeaturesDto {
    ring_mode_allowed: bool,
    forwarding: bool,
    private_forward_targets_allowed: bool,
}

impl InstanceDto {
    pub fn new(settings: &ApiSettings) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            auth_mode: "open",
            user: None,
            public_url: settings.public_url.clone(),
            auth: AuthDto {
                provider_name: None,
            },
            limits: LimitsDto {
                max_captures: settings.max_captures,
                max_body_bytes: settings.max_body_bytes,
                retention_seconds: settings.retention.as_secs(),
                max_delay_ms: MAX_DELAY_MS,
                max_forward_rules: MAX_FORWARD_RULES,
                max_response_body_bytes: RESPONSE_BODY_BYTES,
                max_response_headers: MAX_HEADERS,
            },
            features: FeaturesDto {
                ring_mode_allowed: settings.ring_mode_allowed,
                forwarding: settings.forwarding,
                private_forward_targets_allowed: settings.private_forward_targets_allowed,
            },
        }
    }
}

#[derive(Serialize)]
pub struct EndpointDto {
    id: String,
    url: String,
    title: Option<String>,
    response: ResponseDto,
    cors: bool,
    overflow: &'static str,
    forward_rules: Vec<serde_json::Value>,
    max_captures: u32,
    capture_count: i32,
    bytes_used: i64,
    last_seq: i64,
    created_at: String,
    last_active_at: String,
    expires_at: String,
    owner: Option<serde_json::Value>,
    my_role: Option<String>,
}

#[derive(Serialize)]
struct ResponseDto {
    status: u16,
    content_type: String,
    headers: Vec<NameValueDto>,
    body: BodyDto,
    delay_ms: u64,
}

#[derive(Serialize)]
struct NameValueDto {
    name: String,
    value: String,
}

#[derive(Serialize)]
pub(crate) struct BodyDto {
    encoding: &'static str,
    data: String,
    truncated: bool,
}

impl BodyDto {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_prefix(bytes, false)
    }

    pub(crate) fn from_prefix(bytes: &[u8], truncated: bool) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self {
                encoding: "utf8",
                data: text.to_owned(),
                truncated,
            },
            Err(_) => Self {
                encoding: "base64",
                data: STANDARD.encode(bytes),
                truncated,
            },
        }
    }
}

impl EndpointDto {
    pub fn from_record(record: &EndpointRecord, settings: &ApiSettings) -> Self {
        let stored = &record.settings;
        Self {
            id: record.id.unpack().to_owned(),
            url: format!("{}/{}", settings.public_url, record.id.unpack()),
            title: stored.title.clone(),
            response: ResponseDto {
                status: stored.response.status().unpack(),
                content_type: stored.response.content_type().unpack().to_owned(),
                headers: stored
                    .response
                    .headers()
                    .iter()
                    .map(|header| NameValueDto {
                        name: header.name.unpack().to_owned(),
                        value: header.value.unpack().to_owned(),
                    })
                    .collect(),
                body: BodyDto::from_bytes(stored.response.body()),
                delay_ms: u64::try_from(stored.response.delay().as_millis()).unwrap_or(u64::MAX),
            },
            cors: stored.cors,
            overflow: stored.overflow.as_str(),
            forward_rules: Vec::new(),
            max_captures: settings.max_captures,
            capture_count: record.capture_count,
            bytes_used: record.bytes_used,
            last_seq: record.last_seq,
            created_at: timestamp(record.created_at),
            last_active_at: timestamp(record.last_active_at),
            expires_at: timestamp(record.expires_at),
            owner: None,
            my_role: None,
        }
    }
}

const TIMESTAMP: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

fn timestamp(value: OffsetDateTime) -> String {
    let millis = value.nanosecond() / 1_000_000 * 1_000_000;
    let truncated = value.replace_nanosecond(millis).unwrap_or(value);
    truncated
        .to_offset(UtcOffset::UTC)
        .format(TIMESTAMP)
        .unwrap_or_default()
}
