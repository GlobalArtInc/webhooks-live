use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use whl_auth::AccessControl;
use whl_core::header::MAX_HEADERS;
use whl_core::response::{MAX_BODY_BYTES as RESPONSE_BODY_BYTES, MAX_DELAY_MS};
use whl_store::forward::ForwardRule;
use whl_store::{CaptureAuthRecord, EndpointRecord, EndpointRole, StoredVerifier, UserRecord};

use crate::forward::RuleDto;
use crate::settings::{ApiSettings, MEMBER_ALIASES};

const MAX_FORWARD_RULES: u32 = 5;

#[derive(Serialize)]
pub struct InstanceDto {
    version: &'static str,
    auth_mode: &'static str,
    user: Option<UserDto>,
    public_url: String,
    auth: AuthDto,
    limits: LimitsDto,
    features: FeaturesDto,
}

#[derive(Serialize)]
pub struct UserDto {
    id: String,
    email: String,
    name: Option<String>,
}

impl From<&UserRecord> for UserDto {
    fn from(user: &UserRecord) -> Self {
        Self {
            id: user.id.hyphenated().to_string(),
            email: user.email.clone(),
            name: user.display_name.clone(),
        }
    }
}

#[derive(Serialize)]
struct AuthDto {
    provider_name: Option<String>,
    sign_in_available: bool,
}

#[derive(Serialize)]
struct LimitsDto {
    max_captures: u32,
    max_body_bytes: u64,
    retention_seconds: u64,
    max_captures_settable: u32,
    max_retention_seconds: i64,
    never_expire_endpoints: u32,
    aliases: u32,
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
    secrets: bool,
}

impl InstanceDto {
    pub fn new(
        settings: &ApiSettings,
        access: &AccessControl,
        user: Option<&UserRecord>,
        secrets: bool,
    ) -> Self {
        let ceilings = settings.lifecycle_limits(user.is_some());
        Self {
            version: env!("CARGO_PKG_VERSION"),
            auth_mode: access.mode().as_str(),
            user: user.map(UserDto::from),
            public_url: settings.public_url.clone(),
            auth: AuthDto {
                provider_name: access.provider_name().map(str::to_owned),
                sign_in_available: access.sign_in_available(),
            },
            limits: LimitsDto {
                max_captures: settings.max_captures,
                max_body_bytes: settings.max_body_bytes,
                retention_seconds: settings.retention.as_secs(),
                max_captures_settable: ceilings.max_captures,
                max_retention_seconds: ceilings.max_retention_seconds,
                never_expire_endpoints: ceilings.never_expire_endpoints,
                aliases: if user.is_some() { MEMBER_ALIASES } else { 0 },
                max_delay_ms: MAX_DELAY_MS,
                max_forward_rules: MAX_FORWARD_RULES,
                max_response_body_bytes: RESPONSE_BODY_BYTES,
                max_response_headers: MAX_HEADERS,
            },
            features: FeaturesDto {
                ring_mode_allowed: settings.ring_mode_allowed,
                forwarding: settings.forwarding,
                private_forward_targets_allowed: settings.private_forward_targets_allowed,
                secrets,
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
    tags: Vec<String>,
    forward_rules: Vec<RuleDto>,
    verifier: Option<serde_json::Value>,
    max_captures: u32,
    capture_count: i32,
    bytes_used: i64,
    last_seq: i64,
    created_at: String,
    last_active_at: String,
    expires_at: Option<String>,
    retention: RetentionDto,
    capture_ttl_seconds: Option<i32>,
    sender_allow_cidrs: Vec<String>,
    capture_auth: Option<CaptureAuthDto>,
    owner: Option<UserDto>,
    my_role: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CaptureAuthDto {
    Basic { username: String },
    Header { name: String },
}

impl From<&CaptureAuthRecord> for CaptureAuthDto {
    fn from(record: &CaptureAuthRecord) -> Self {
        match record {
            CaptureAuthRecord::Basic { username, .. } => Self::Basic {
                username: username.unpack().to_owned(),
            },
            CaptureAuthRecord::Header { name, .. } => Self::Header {
                name: name.unpack().to_owned(),
            },
        }
    }
}

#[derive(Serialize)]
struct RetentionDto {
    mode: &'static str,
    seconds: Option<i32>,
}

#[derive(Serialize)]
struct ResponseDto {
    status: u16,
    templated: bool,
    status_template: Option<String>,
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
    pub fn from_record(
        record: &EndpointRecord,
        settings: &ApiSettings,
        rules: &[ForwardRule],
    ) -> Self {
        let stored = &record.settings;
        Self {
            id: record.id.unpack().to_owned(),
            url: format!("{}/{}", settings.public_url, record.id.unpack()),
            title: stored.title.clone(),
            response: ResponseDto {
                status: stored.response.status().unpack(),
                templated: stored.templating.enabled,
                status_template: stored.templating.status.clone(),
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
            tags: stored
                .tags
                .iter()
                .map(|tag| tag.unpack().to_owned())
                .collect(),
            forward_rules: rules.iter().map(RuleDto::from).collect(),
            verifier: stored.verifier.as_ref().map(verifier_value),
            max_captures: record
                .lifecycle
                .effective_max_captures(settings.max_captures),
            capture_count: record.capture_count,
            bytes_used: record.bytes_used,
            last_seq: record.last_seq,
            created_at: timestamp(record.created_at),
            last_active_at: timestamp(record.last_active_at),
            expires_at: record.expires_at.map(timestamp),
            retention: RetentionDto {
                mode: if record.lifecycle.never_expires {
                    "never"
                } else {
                    "sliding"
                },
                seconds: record.lifecycle.retention_seconds,
            },
            capture_ttl_seconds: record.lifecycle.capture_ttl_seconds,
            sender_allow_cidrs: record
                .access
                .sender_allow
                .ranges()
                .iter()
                .map(ToString::to_string)
                .collect(),
            capture_auth: record
                .access
                .capture_auth
                .as_ref()
                .map(CaptureAuthDto::from),
            owner: None,
            my_role: None,
        }
    }

    pub fn with_identity(mut self, role: Option<EndpointRole>, owner: Option<&UserRecord>) -> Self {
        self.my_role = role.map(EndpointRole::as_str);
        self.owner = owner.map(UserDto::from);
        self
    }
}

fn verifier_value(stored: &StoredVerifier) -> serde_json::Value {
    match stored {
        StoredVerifier::Configured(verifier) => {
            serde_json::to_value(verifier.document()).unwrap_or(serde_json::Value::Null)
        }
        StoredVerifier::Unreadable(raw) => {
            serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
        }
    }
}

const TIMESTAMP: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

pub(crate) fn timestamp(value: OffsetDateTime) -> String {
    let millis = value.nanosecond() / 1_000_000 * 1_000_000;
    let truncated = value.replace_nanosecond(millis).unwrap_or(value);
    truncated
        .to_offset(UtcOffset::UTC)
        .format(TIMESTAMP)
        .unwrap_or_default()
}
