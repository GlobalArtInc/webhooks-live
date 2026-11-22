use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use utoipa::ToSchema;
use whl_auth::AccessControl;
use whl_core::header::MAX_HEADERS;
use whl_core::response::{MAX_BODY_BYTES as RESPONSE_BODY_BYTES, MAX_DELAY_MS};
use whl_core::tier::Tier;
use whl_store::forward::ForwardRule;
use whl_store::{CaptureAuthRecord, EndpointRecord, EndpointRole, StoredVerifier, UserRecord};

use crate::forward::RuleDto;
use crate::settings::ApiSettings;
use crate::view_password::GrantDto;

const MAX_FORWARD_RULES: u32 = 5;

#[derive(Serialize, ToSchema)]
#[schema(as = Instance)]
pub struct InstanceDto {
    #[schema(example = "1.0.0")]
    version: &'static str,
    #[schema(pattern = "^(open|sso|mixed)$", example = "sso")]
    auth_mode: &'static str,
    #[schema(example = "member")]
    tier: String,
    user: Option<UserDto>,
    #[schema(example = "https://webhooks.globalart.hu")]
    public_url: String,
    auth: AuthDto,
    limits: LimitsDto,
    features: FeaturesDto,
}

#[derive(Serialize, ToSchema)]
#[schema(as = User)]
pub struct UserDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21"
    )]
    id: String,
    #[schema(example = "dev@example.com")]
    email: String,
    #[schema(example = "Dev")]
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

#[derive(Serialize, ToSchema)]
#[schema(as = InstanceAuth)]
struct AuthDto {
    #[schema(example = "GlobalArt SSO")]
    provider_name: Option<String>,
    sign_in_available: bool,
}

#[derive(Serialize, ToSchema)]
#[schema(as = InstanceLimits)]
struct LimitsDto {
    max_captures: u32,
    max_body_bytes: u64,
    retention_seconds: u64,
    max_captures_settable: u32,
    max_retention_seconds: i64,
    never_expire_endpoints: u32,
    max_delay_ms: i64,
    max_forward_rules: u32,
    max_response_body_bytes: usize,
    max_response_headers: usize,
    endpoints: u32,
    storage_bytes: Option<u64>,
    rules_per_endpoint: u32,
    steps_per_rule: u32,
    sync_http_steps: u32,
    aliases: u32,
    tokens: u32,
    folders: u32,
    schedules: u32,
    schedule_min_interval_seconds: u64,
    exports_per_hour: u32,
    export_max_bytes: u64,
    imports_per_day: u32,
    import_max_bytes: u64,
    outbound_per_hour: Option<u32>,
    notifications_per_hour: Option<u32>,
    email_notifications_per_day: u32,
}

#[derive(Serialize, ToSchema)]
#[schema(as = InstanceFeatures)]
struct FeaturesDto {
    ring_mode_allowed: bool,
    forwarding: bool,
    private_forward_targets_allowed: bool,
    secrets: bool,
    push_public_key: Option<String>,
    schedules: bool,
    notifications: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Availability {
    pub secrets: bool,
    pub schedules: bool,
    pub notifications: bool,
}

impl InstanceDto {
    pub fn new(
        settings: &ApiSettings,
        access: &AccessControl,
        user: Option<&UserRecord>,
        tier: Tier<'_>,
        available: Availability,
        push_public_key: Option<&str>,
    ) -> Self {
        let limits = tier.limits();
        let ceilings = settings.lifecycle_limits(tier);
        let (outbound, notifications) = if user.is_some() {
            (limits.outbound_per_user_hour, limits.notify_per_user_hour)
        } else {
            (
                limits.outbound_per_endpoint_hour,
                limits.notify_per_endpoint_hour,
            )
        };
        Self {
            version: env!("CARGO_PKG_VERSION"),
            auth_mode: access.mode().as_str(),
            tier: tier.name().to_owned(),
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
                max_delay_ms: MAX_DELAY_MS,
                max_forward_rules: MAX_FORWARD_RULES,
                max_response_body_bytes: RESPONSE_BODY_BYTES,
                max_response_headers: MAX_HEADERS,
                endpoints: limits.endpoints,
                storage_bytes: limits.storage_bytes,
                rules_per_endpoint: limits.rules_per_endpoint,
                steps_per_rule: limits.steps_per_rule,
                sync_http_steps: limits.sync_http_steps,
                aliases: limits.aliases,
                tokens: limits.tokens,
                folders: limits.folders,
                schedules: limits.schedules,
                schedule_min_interval_seconds: limits.schedule_min_interval.as_secs(),
                exports_per_hour: limits.exports_per_hour,
                export_max_bytes: limits.export_max_bytes,
                imports_per_day: limits.imports_per_day,
                import_max_bytes: limits.import_max_bytes,
                outbound_per_hour: outbound,
                notifications_per_hour: notifications,
                email_notifications_per_day: limits.email_notifications_per_day,
            },
            features: FeaturesDto {
                ring_mode_allowed: settings.ring_mode_allowed,
                forwarding: settings.forwarding,
                private_forward_targets_allowed: settings.private_forward_targets_allowed,
                secrets: available.secrets,
                push_public_key: push_public_key.map(str::to_owned),
                schedules: available.schedules && limits.schedules > 0,
                notifications: if available.notifications {
                    limits
                        .notification_kinds
                        .iter()
                        .filter(|kind| crate::notify::kind_is_available(kind))
                        .cloned()
                        .collect()
                } else {
                    Vec::new()
                },
            },
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = Endpoint)]
pub struct EndpointDto {
    #[schema(
        pattern = "^[0-9a-hjkmnp-tv-z]{24}$",
        example = "7k2m9q4xw8c1d5f3h6j0n2p4"
    )]
    id: String,
    url: String,
    title: Option<String>,
    response: ResponseDto,
    cors: bool,
    #[schema(pattern = "^(refuse|ring)$", example = "refuse")]
    overflow: &'static str,
    tags: Vec<String>,
    forward_rules: Vec<RuleDto>,
    #[schema(value_type = Option<VerifierDoc>)]
    verifier: Option<serde_json::Value>,
    max_captures: u32,
    capture_count: i32,
    bytes_used: i64,
    last_seq: i64,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
    last_active_at: String,
    #[schema(format = DateTime)]
    expires_at: Option<String>,
    retention: RetentionDto,
    capture_ttl_seconds: Option<i32>,
    sender_allow_cidrs: Vec<String>,
    capture_auth: Option<CaptureAuthDto>,
    owner: Option<UserDto>,
    #[schema(pattern = "^(owner|editor|viewer)$", example = "owner")]
    my_role: Option<&'static str>,
    view_protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    view_grant: Option<GrantDto>,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schema(as = CaptureAuth)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = Retention)]
struct RetentionDto {
    #[schema(pattern = "^(sliding|never)$", example = "sliding")]
    mode: &'static str,
    seconds: Option<i32>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = EndpointResponse)]
struct ResponseDto {
    #[schema(minimum = 100, maximum = 599, default = 200)]
    status: u16,
    templated: bool,
    status_template: Option<String>,
    #[schema(example = "text/plain")]
    content_type: String,
    headers: Vec<NameValueDto>,
    body: BodyDto,
    #[schema(maximum = 10000)]
    delay_ms: u64,
}

#[derive(Serialize, ToSchema)]
#[schema(as = NameValue)]
pub(crate) struct NameValueDto {
    #[schema(example = "content-type")]
    name: String,
    #[schema(example = "application/json")]
    value: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = Body)]
pub(crate) struct BodyDto {
    #[schema(pattern = "^(utf8|base64)$", example = "utf8")]
    encoding: &'static str,
    data: String,
    truncated: bool,
}

#[derive(ToSchema)]
#[schema(as = BodyInput)]
#[allow(dead_code)]
pub(crate) struct BodyInputDoc {
    #[schema(pattern = "^(utf8|base64)$", example = "utf8")]
    encoding: String,
    #[schema(example = "{\"ok\":true}")]
    data: String,
    truncated: Option<bool>,
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
            view_protected: false,
            view_grant: None,
        }
    }

    pub fn with_view_grant(mut self, grant: Option<GrantDto>) -> Self {
        self.view_grant = grant;
        self
    }

    pub fn with_view_protected(mut self, protected: bool) -> Self {
        self.view_protected = protected;
        self
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

#[derive(ToSchema)]
#[schema(as = EndpointInput)]
#[allow(dead_code)]
pub struct EndpointInputDoc {
    #[schema(max_length = 100)]
    title: Option<String>,
    response: Option<EndpointResponseInputDoc>,
    cors: Option<bool>,
    #[schema(pattern = "^(refuse|ring)$")]
    overflow: Option<String>,
    #[schema(max_items = 10)]
    tags: Option<Vec<String>>,
    retention: Option<RetentionInputDoc>,
    #[schema(minimum = 1)]
    max_captures: Option<u32>,
    #[schema(minimum = 60)]
    capture_ttl_seconds: Option<u32>,
    #[schema(max_items = 20)]
    sender_allow_cidrs: Option<Vec<String>>,
    capture_auth: Option<CaptureAuthInputDoc>,
    #[schema(min_length = 8, max_length = 128)]
    view_password: Option<String>,
    verifier: Option<VerifierDoc>,
    #[schema(min_length = 1)]
    template_id: Option<String>,
}

#[derive(ToSchema)]
#[schema(as = EndpointResponseInput)]
#[allow(dead_code)]
pub struct EndpointResponseInputDoc {
    #[schema(minimum = 100, maximum = 599)]
    status: Option<u16>,
    content_type: Option<String>,
    headers: Option<Vec<NameValueDto>>,
    body: Option<BodyInputDoc>,
    #[schema(maximum = 10000)]
    delay_ms: Option<u64>,
    templated: Option<bool>,
    #[schema(max_length = 1024)]
    status_template: Option<String>,
}

#[derive(ToSchema)]
#[schema(as = RetentionInput)]
#[allow(dead_code)]
pub struct RetentionInputDoc {
    #[schema(pattern = "^(sliding|never)$")]
    mode: Option<String>,
    #[schema(minimum = 3600)]
    seconds: Option<u32>,
}

#[derive(ToSchema)]
#[schema(as = CaptureAuthInput)]
#[allow(dead_code)]
pub struct CaptureAuthInputDoc {
    #[schema(pattern = "^(basic|header)$")]
    kind: String,
    #[schema(min_length = 1, max_length = 256)]
    username: Option<String>,
    #[schema(min_length = 1, max_length = 256)]
    password: Option<String>,
    #[schema(min_length = 1, max_length = 256)]
    name: Option<String>,
    #[schema(min_length = 1, max_length = 256)]
    value: Option<String>,
}

#[derive(ToSchema)]
#[schema(as = Verifier)]
#[allow(dead_code)]
pub struct VerifierDoc {
    #[schema(pattern = "^(hmac|github|stripe|slack|shopify|standard_webhooks)$")]
    scheme: String,
    secret_name: String,
    header: Option<String>,
    #[schema(pattern = "^(sha1|sha256|sha512)$")]
    algorithm: Option<String>,
    #[schema(pattern = "^(hex|base64)$")]
    encoding: Option<String>,
    prefix: Option<String>,
    timestamp_header: Option<String>,
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
