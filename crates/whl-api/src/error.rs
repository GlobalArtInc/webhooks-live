use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use time::OffsetDateTime;
use whl_auth::{AccessDenied, AuthRejection};
use whl_core::validation::{FieldPath, Violation, ViolationCode};
use whl_rules::MAX_RULES_PER_ENDPOINT;
use whl_script::LimitsError;
use whl_secrets::{MAX_SECRETS_PER_ENDPOINT, MacError, SealError};
use whl_store::MAX_FOLDERS_PER_USER;
use whl_store::composer::ComposerStoreError;
use whl_store::captures::CaptureStoreError;
use whl_store::forward::ForwardStoreError;
use whl_store::notify::{MAX_CHANNELS_PER_ENDPOINT, NotifyStoreError};
use whl_store::rules::RuleStoreError;
use whl_store::schedules::ScheduleStoreError;
use whl_store::secrets::SecretStoreError;
use whl_store::vars::{MAX_VARS_PER_ENDPOINT, VarStoreError};
use whl_store::{
    AliasStoreError, ApiTokenStoreError, EndpointError, FolderError, LifecycleError,
    MembershipError,
};

use crate::dto::timestamp;
use crate::settings::MEMBER_ALIASES;
use crate::tokens::{MAX_TOKENS_PER_HOUR, MAX_TOKENS_PER_USER};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldCode {
    Required,
    InvalidFormat,
    TooLong,
    OutOfRange,
    TooMany,
    Duplicate,
    NotAllowed,
    Reserved,
    TargetRefused,
    TargetUnresolvable,
    SecretUnknown,
    TemplateInvalid,
    JsonPathInvalid,
    SecretNotAllowed,
    StepUnknown,
    CronInvalid,
    TimezoneInvalid,
    IntervalTooShort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldViolation {
    field: String,
    code: FieldCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<u32>,
}

impl FieldViolation {
    pub fn new(field: impl Into<String>, code: FieldCode, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    pub fn at(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    #[cfg(test)]
    pub fn field(&self) -> &str {
        &self.field
    }

    #[cfg(test)]
    pub fn code(&self) -> FieldCode {
        self.code
    }

    pub fn prefixed(self, prefix: &str) -> Self {
        Self {
            field: format!("{prefix}.{}", self.field),
            ..self
        }
    }

    pub fn from_core(violation: &Violation) -> Self {
        Self::new(
            core_field(violation.field),
            core_code(violation.code),
            violation.code.to_string(),
        )
    }
}

fn core_field(field: FieldPath) -> String {
    match field {
        FieldPath::Status => "response.status".to_owned(),
        FieldPath::ContentType => "response.content_type".to_owned(),
        FieldPath::Headers => "response.headers".to_owned(),
        FieldPath::HeaderName(index) => format!("response.headers.{index}.name"),
        FieldPath::HeaderValue(index) => format!("response.headers.{index}.value"),
        FieldPath::Body => "response.body".to_owned(),
        FieldPath::DelayMs => "response.delay_ms".to_owned(),
    }
}

pub(crate) fn core_code(code: ViolationCode) -> FieldCode {
    match code {
        ViolationCode::OutOfRange { .. } => FieldCode::OutOfRange,
        ViolationCode::Empty => FieldCode::Required,
        ViolationCode::TooLong { .. } => FieldCode::TooLong,
        ViolationCode::TooMany { .. } => FieldCode::TooMany,
        ViolationCode::Denied => FieldCode::NotAllowed,
        ViolationCode::ControlCharacter
        | ViolationCode::NonAscii
        | ViolationCode::InvalidToken
        | ViolationCode::InvalidMediaType
        | ViolationCode::Duplicate => FieldCode::InvalidFormat,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    InvalidJson(&'static str),
    #[error("the request body must be application/json")]
    UnsupportedMediaType,
    #[error("the request body is too large")]
    PayloadTooLarge,
    #[error("the request failed validation")]
    Validation(Vec<FieldViolation>),
    #[error("the endpoint is unknown, expired or deleted")]
    EndpointGone,
    #[error("the resource was not found")]
    NotFound,
    #[error("the method is not allowed on this route")]
    MethodNotAllowed,
    #[error("the capture is not found in this endpoint")]
    CaptureNotFound,
    #[error("the part is not found in this capture")]
    PartNotFound,
    #[error("the forward rule is not found in this endpoint")]
    ForwardRuleNotFound,
    #[error("the endpoint already has the maximum number of forward rules")]
    ForwardRuleLimit,
    #[error("the rule is not found in this endpoint")]
    RuleNotFound,
    #[error("the endpoint already has the maximum number of rules")]
    RuleLimit,
    #[error("forwarding is disabled on this instance")]
    ForwardingDisabled,
    #[error("the cursor is malformed or does not match the order and filters")]
    InvalidCursor,
    #[error("this route needs sso or mixed mode")]
    NotAvailableInMode,
    #[error("the endpoint already has an owner")]
    AlreadyOwned,
    #[error("the endpoint cannot be claimed from here")]
    ClaimRefused,
    #[error("nobody with a verified email address has signed in with that email")]
    UserNotFound,
    #[error("more than one account has verified that email address, so it cannot name one user")]
    EmailAmbiguous,
    #[error("the user is not a member of this endpoint")]
    MemberNotFound,
    #[error(transparent)]
    Auth(AuthRejection),
    #[error("membership storage failed: {0}")]
    Membership(MembershipError),
    #[error("the database is unavailable")]
    StoreUnavailable,
    #[error("too many requests")]
    RateLimited { retry_after_secs: u64 },
    #[error("the client address cannot be determined")]
    ClientUnknown,
    #[error("endpoint storage failed: {0}")]
    Internal(EndpointError),
    #[error("forward storage failed: {0}")]
    ForwardStorage(ForwardStoreError),
    #[error("the folder is not found")]
    FolderNotFound,
    #[error("a folder with that name already exists")]
    FolderNameTaken,
    #[error("the user already has the maximum number of folders")]
    FolderLimit,
    #[error("folder storage failed: {0}")]
    FolderStorage(FolderError),
    #[error("the secret is not found in this endpoint")]
    SecretNotFound,
    #[error("the endpoint already has the maximum number of secrets")]
    SecretLimit,
    #[error("only the owner can change the retention settings of an endpoint")]
    OwnerRequired,
    #[error("the owner already has the maximum of {max} never-expire endpoints")]
    NeverExpireLimit { max: u32 },
    #[error("the secret is still used by the signature verifier of this endpoint")]
    SecretInUse { endpoint: String },
    #[error("the selection is larger than the export limit")]
    ExportTooLarge,
    #[error("the import is larger than the import limit")]
    ImportTooLarge,
    #[error("the import must be application/json or application/x-ndjson")]
    ImportMediaType,
    #[error("import storage failed: {0}")]
    ImportStorage(CaptureStoreError),
    #[error("secrets are not configured on this instance")]
    SecretsDisabled,
    #[error("the variable is not found in this endpoint")]
    VarNotFound,
    #[error("the endpoint already has the maximum number of variables")]
    VarLimit,
    #[error("variable storage failed: {0}")]
    VarStorage(VarStoreError),
    #[error("secret storage failed: {0}")]
    SecretStorage(SecretStoreError),
    #[error("composer storage failed: {0}")]
    ComposerStorage(ComposerStoreError),
    #[error("the composer is disabled on this instance")]
    ComposerDisabled,
    #[error("the composer could not be set up")]
    ComposerInternal,
    #[error("the composer request body is too large")]
    SendPayloadTooLarge,
    #[error("the composer request body was not received in time")]
    SendBodyTimeout,
    #[error("this request has been relayed too many times")]
    HopLimit,
    #[error("the composer quota {name} of {max} is used up")]
    ComposerQuota {
        name: &'static str,
        max: usize,
        retry_after_secs: u64,
    },
    #[error(transparent)]
    Access(AccessDenied),
    #[error("a secret could not be sealed: {0}")]
    SecretSeal(SealError),
    #[error("the capture authentication code could not be computed: {0}")]
    CaptureAuthMac(MacError),
    #[error("the API token is not found")]
    TokenNotFound,
    #[error("the maximum number of active API tokens is reached")]
    TokenLimit,
    #[error("too many API tokens were created recently")]
    TokenCreationRate { resets_at: OffsetDateTime },
    #[error("a token could not be generated")]
    EntropyUnavailable,
    #[error("API token storage failed: {0}")]
    TokenStorage(ApiTokenStoreError),
    #[error("rule storage failed: {0}")]
    RuleStorage(RuleStoreError),
    #[error("the script engine cannot be configured: {0}")]
    RuleEngine(LimitsError),
    #[error("the schedule is not found in this endpoint")]
    ScheduleNotFound,
    #[error("the owner already has the maximum number of schedules")]
    ScheduleLimit(usize),
    #[error("the schedule is paused or disabled, enable it before running it")]
    ScheduleDisabled,
    #[error("schedules are switched off on this instance")]
    SchedulesDisabled,
    #[error("only the owner can change a request that uses secrets")]
    ScheduleOwnerRequired,
    #[error("schedule storage failed: {0}")]
    ScheduleStorage(ScheduleStoreError),
    #[error("the notification channel is not found in this endpoint")]
    ChannelNotFound,
    #[error("the endpoint already has the maximum number of notification channels")]
    ChannelLimit,
    #[error("notification storage failed: {0}")]
    NotifyStorage(NotifyStoreError),
    #[error("{0}")]
    InternalFault(&'static str),
    #[error("the alias is not found on this endpoint")]
    AliasNotFound,
    #[error("the alias is in use or in quarantine")]
    AliasTaken,
    #[error("the user already has the maximum number of aliases")]
    AliasLimit,
    #[error("alias storage failed: {0}")]
    AliasStorage(AliasStoreError),
}

impl From<AliasStoreError> for ApiError {
    fn from(error: AliasStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::AliasStorage(error)
        }
    }
}

impl From<ApiTokenStoreError> for ApiError {
    fn from(error: ApiTokenStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::TokenStorage(error)
        }
    }
}

impl From<ComposerStoreError> for ApiError {
    fn from(error: ComposerStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::ComposerStorage(error)
        }
    }
}

impl From<ScheduleStoreError> for ApiError {
    fn from(error: ScheduleStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::ScheduleStorage(error)
        }
    }
}

impl From<NotifyStoreError> for ApiError {
    fn from(error: NotifyStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::NotifyStorage(error)
        }
    }
}

impl From<CaptureStoreError> for ApiError {
    fn from(error: CaptureStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::ImportStorage(error)
        }
    }
}

impl From<SecretStoreError> for ApiError {
    fn from(error: SecretStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::SecretStorage(error)
        }
    }
}

impl From<FolderError> for ApiError {
    fn from(error: FolderError) -> Self {
        match error {
            FolderError::Unavailable(_) => Self::StoreUnavailable,
            other => Self::FolderStorage(other),
        }
    }
}

impl From<VarStoreError> for ApiError {
    fn from(error: VarStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::VarStorage(error)
        }
    }
}

impl From<ForwardStoreError> for ApiError {
    fn from(error: ForwardStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::ForwardStorage(error)
        }
    }
}

impl From<RuleStoreError> for ApiError {
    fn from(error: RuleStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::RuleStorage(error)
        }
    }
}

impl From<MembershipError> for ApiError {
    fn from(error: MembershipError) -> Self {
        match error {
            MembershipError::Unavailable(_) => Self::StoreUnavailable,
            other => Self::Membership(other),
        }
    }
}

impl From<LifecycleError> for ApiError {
    fn from(error: LifecycleError) -> Self {
        match error {
            LifecycleError::Endpoint(error) => error.into(),
            LifecycleError::NeverExpireLimit { max } => Self::NeverExpireLimit { max },
            LifecycleError::NeverExpireNeedsOwner => Self::Auth(AuthRejection::SignInRequired),
        }
    }
}

impl From<AuthRejection> for ApiError {
    fn from(rejection: AuthRejection) -> Self {
        Self::Auth(rejection)
    }
}

impl From<EndpointError> for ApiError {
    fn from(error: EndpointError) -> Self {
        match error {
            EndpointError::Unavailable(_) => Self::StoreUnavailable,
            EndpointError::VerifierSecretUnknown => Self::Validation(vec![FieldViolation::new(
                "verifier.secret_name",
                FieldCode::SecretUnknown,
                "names a secret this endpoint does not hold",
            )]),
            other => Self::Internal(other),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<FieldViolation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<LimitDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    references: Option<Vec<ReferenceDetail>>,
}

#[derive(Serialize)]
struct ReferenceDetail {
    kind: &'static str,
    id: String,
}

#[derive(Serialize)]
struct LimitDetail {
    name: &'static str,
    max: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resets_at: Option<String>,
}

impl LimitDetail {
    fn count(name: &'static str, max: usize) -> Self {
        Self {
            name,
            max,
            window: None,
            resets_at: None,
        }
    }
}

fn seconds_until(moment: OffsetDateTime) -> u64 {
    let remaining = (moment - OffsetDateTime::now_utc()).whole_seconds();
    u64::try_from(remaining).unwrap_or(0).max(1)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Auth(rejection) = self {
            return rejection.into_response();
        }
        if let Self::Access(denied) = self {
            return denied.into_response();
        }
        let retry_after = match &self {
            Self::RateLimited { retry_after_secs }
            | Self::ComposerQuota {
                retry_after_secs, ..
            } => Some(*retry_after_secs),
            Self::TokenCreationRate { resets_at } => Some(seconds_until(*resets_at)),
            _ => None,
        };
        let limit = match &self {
            Self::SecretLimit => Some(LimitDetail::count("secrets", MAX_SECRETS_PER_ENDPOINT)),
            Self::TokenLimit => Some(LimitDetail::count("tokens", MAX_TOKENS_PER_USER as usize)),
            Self::TokenCreationRate { resets_at } => Some(LimitDetail {
                name: "token_creation",
                max: MAX_TOKENS_PER_HOUR as usize,
                window: Some("hour"),
                resets_at: Some(timestamp(*resets_at)),
            }),
            Self::FolderLimit => Some(LimitDetail::count("folders", MAX_FOLDERS_PER_USER)),
            Self::AliasLimit => Some(LimitDetail::count(
                "aliases",
                usize::try_from(MEMBER_ALIASES).unwrap_or(usize::MAX),
            )),
            Self::NeverExpireLimit { max } => Some(LimitDetail::count(
                "never_expire",
                usize::try_from(*max).unwrap_or(usize::MAX),
            )),
            Self::RuleLimit => Some(LimitDetail::count("rules", MAX_RULES_PER_ENDPOINT)),
            Self::VarLimit => Some(LimitDetail::count("vars", MAX_VARS_PER_ENDPOINT)),
            Self::ComposerQuota { name, max, .. } => Some(LimitDetail::count(name, *max)),
            Self::ScheduleLimit(max) => Some(LimitDetail::count("schedules", *max)),
            Self::ChannelLimit => Some(LimitDetail::count(
                "notification_channels",
                MAX_CHANNELS_PER_ENDPOINT,
            )),
            _ => None,
        };
        let references = match &self {
            Self::SecretInUse { endpoint } => Some(vec![ReferenceDetail {
                kind: "verifier",
                id: endpoint.clone(),
            }]),
            _ => None,
        };
        let (status, code, message, fields) = match self {
            Self::InvalidJson(detail) => (
                StatusCode::BAD_REQUEST,
                "invalid_json",
                detail.to_owned(),
                None,
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                self.to_string(),
                None,
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                format!(
                    "the request body is larger than {} bytes",
                    crate::body::MAX_BODY_BYTES
                ),
                None,
            ),
            Self::Validation(violations) => {
                let message = violations.first().map_or_else(
                    || "the request failed validation".to_owned(),
                    |first| format!("{} {}", first.field, first.message),
                );
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "validation_failed",
                    message,
                    Some(violations),
                )
            }
            Self::EndpointGone => (StatusCode::GONE, "endpoint_gone", self.to_string(), None),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", self.to_string(), None),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                self.to_string(),
                None,
            ),
            Self::CaptureNotFound => (
                StatusCode::NOT_FOUND,
                "capture_not_found",
                self.to_string(),
                None,
            ),
            Self::PartNotFound => (
                StatusCode::NOT_FOUND,
                "part_not_found",
                self.to_string(),
                None,
            ),
            Self::ForwardRuleNotFound => (
                StatusCode::NOT_FOUND,
                "forward_rule_not_found",
                self.to_string(),
                None,
            ),
            Self::ForwardRuleLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "forward_rule_limit",
                self.to_string(),
                None,
            ),
            Self::ForwardingDisabled => {
                (StatusCode::NOT_FOUND, "not_found", self.to_string(), None)
            }
            Self::RuleNotFound => (
                StatusCode::NOT_FOUND,
                "rule_not_found",
                self.to_string(),
                None,
            ),
            Self::RuleLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::RuleStorage(error) => {
                tracing::error!(%error, "rule storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::RuleEngine(error) => {
                tracing::error!(%error, "the script engine cannot be configured");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::InvalidCursor => (
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                self.to_string(),
                None,
            ),
            Self::StoreUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "store_unavailable",
                self.to_string(),
                None,
            ),
            Self::RateLimited { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                self.to_string(),
                None,
            ),
            Self::ClientUnknown => {
                tracing::error!("a request reached the rate limiter without a client address");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::NotAvailableInMode => (
                StatusCode::FORBIDDEN,
                "not_available_in_mode",
                self.to_string(),
                None,
            ),
            Self::AlreadyOwned => (
                StatusCode::CONFLICT,
                "already_owned",
                self.to_string(),
                None,
            ),
            Self::ClaimRefused => (
                StatusCode::FORBIDDEN,
                "claim_refused",
                self.to_string(),
                None,
            ),
            Self::UserNotFound => (
                StatusCode::NOT_FOUND,
                "user_not_found",
                self.to_string(),
                None,
            ),
            Self::EmailAmbiguous => (
                StatusCode::CONFLICT,
                "email_ambiguous",
                self.to_string(),
                None,
            ),
            Self::MemberNotFound => (
                StatusCode::NOT_FOUND,
                "member_not_found",
                self.to_string(),
                None,
            ),
            Self::Access(_) | Self::Auth(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "the request could not be completed".to_owned(),
                None,
            ),
            Self::Membership(error) => {
                tracing::error!(%error, "membership storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::Internal(error) => {
                tracing::error!(%error, "endpoint storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::FolderNotFound => (
                StatusCode::NOT_FOUND,
                "folder_not_found",
                self.to_string(),
                None,
            ),
            Self::FolderNameTaken => (StatusCode::CONFLICT, "name_taken", self.to_string(), None),
            Self::FolderLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::FolderStorage(error) => {
                tracing::error!(%error, "folder storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::ForwardStorage(error) => {
                tracing::error!(%error, "forward storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::ComposerDisabled => (StatusCode::NOT_FOUND, "not_found", self.to_string(), None),
            Self::ComposerInternal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "the request could not be completed".to_owned(),
                None,
            ),
            Self::SendPayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                format!(
                    "the request body is larger than {} bytes",
                    crate::composer::MAX_SEND_BODY_BYTES
                ),
                None,
            ),
            Self::SendBodyTimeout => (
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                self.to_string(),
                None,
            ),
            Self::HopLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "hop_limit",
                self.to_string(),
                None,
            ),
            Self::TokenNotFound => (
                StatusCode::NOT_FOUND,
                "token_not_found",
                self.to_string(),
                None,
            ),
            Self::TokenLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::TokenCreationRate { .. } | Self::ComposerQuota { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "quota_exceeded",
                self.to_string(),
                None,
            ),
            Self::ComposerStorage(error) => {
                tracing::error!(%error, "composer storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::EntropyUnavailable => {
                tracing::error!("the operating system random source failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::TokenStorage(error) => {
                tracing::error!(%error, "API token storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::AliasNotFound => (
                StatusCode::NOT_FOUND,
                "alias_not_found",
                self.to_string(),
                None,
            ),
            Self::AliasTaken => (StatusCode::CONFLICT, "alias_taken", self.to_string(), None),
            Self::AliasLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::AliasStorage(error) => {
                tracing::error!(%error, "alias storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::SecretNotFound => (
                StatusCode::NOT_FOUND,
                "secret_not_found",
                self.to_string(),
                None,
            ),
            Self::SecretInUse { .. } => (
                StatusCode::CONFLICT,
                "secret_in_use",
                self.to_string(),
                None,
            ),
            Self::SecretLimit | Self::VarLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::OwnerRequired => (StatusCode::FORBIDDEN, "forbidden", self.to_string(), None),
            Self::NeverExpireLimit { .. } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::ExportTooLarge => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "export_too_large",
                self.to_string(),
                None,
            ),
            Self::VarNotFound => (
                StatusCode::NOT_FOUND,
                "var_not_found",
                self.to_string(),
                None,
            ),
            Self::ImportTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                self.to_string(),
                None,
            ),
            Self::ImportMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                self.to_string(),
                None,
            ),
            Self::ImportStorage(error) => {
                tracing::error!(%error, "import storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::VarStorage(error) => {
                tracing::error!(%error, "variable storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::SecretsDisabled => (
                StatusCode::SERVICE_UNAVAILABLE,
                "feature_disabled",
                self.to_string(),
                None,
            ),
            Self::SecretStorage(error) => {
                tracing::error!(%error, "secret storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::ScheduleNotFound => (
                StatusCode::NOT_FOUND,
                "schedule_not_found",
                self.to_string(),
                None,
            ),
            Self::ScheduleLimit(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::ScheduleOwnerRequired => (
                StatusCode::FORBIDDEN,
                "owner_required",
                self.to_string(),
                None,
            ),
            Self::ScheduleDisabled => (
                StatusCode::CONFLICT,
                "schedule_disabled",
                self.to_string(),
                None,
            ),
            Self::SchedulesDisabled => (
                StatusCode::SERVICE_UNAVAILABLE,
                "feature_disabled",
                self.to_string(),
                None,
            ),
            Self::ScheduleStorage(error) => {
                tracing::error!(%error, "schedule storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::CaptureAuthMac(error) => {
                tracing::error!(%error, "the capture authentication code could not be computed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::SecretSeal(error) => {
                tracing::error!(%error, "a secret could not be sealed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                "channel_not_found",
                self.to_string(),
                None,
            ),
            Self::ChannelLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
            Self::NotifyStorage(error) => {
                tracing::error!(%error, "notification storage failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
            Self::InternalFault(detail) => {
                tracing::error!(detail, "an unexpected server fault");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the request could not be completed".to_owned(),
                    None,
                )
            }
        };
        let mut response = (
            status,
            Json(ErrorBody {
                error: ErrorDetail {
                    code,
                    message,
                    fields,
                    limit,
                    references,
                },
            }),
        )
            .into_response();
        if status == StatusCode::SERVICE_UNAVAILABLE && code != "feature_disabled" {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
        }
        if let Some(seconds) = retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}
