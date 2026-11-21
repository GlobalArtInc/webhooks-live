use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use time::OffsetDateTime;
use whl_auth::AuthRejection;
use whl_core::validation::{FieldPath, Violation, ViolationCode};
use whl_secrets::{MAX_SECRETS_PER_ENDPOINT, SealError};
use whl_store::MAX_FOLDERS_PER_USER;
use whl_store::forward::ForwardStoreError;
use whl_store::secrets::SecretStoreError;
use whl_store::{ApiTokenStoreError, EndpointError, FolderError, MembershipError};

use crate::dto::timestamp;
use crate::tokens::{MAX_TOKENS_PER_HOUR, MAX_TOKENS_PER_USER};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldCode {
    Required,
    InvalidFormat,
    TooLong,
    OutOfRange,
    TooMany,
    NotAllowed,
    TargetRefused,
    TargetUnresolvable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldViolation {
    field: String,
    code: FieldCode,
    message: String,
}

impl FieldViolation {
    pub fn new(field: impl Into<String>, code: FieldCode, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
        }
    }

    #[cfg(test)]
    pub fn field(&self) -> &str {
        &self.field
    }

    #[cfg(test)]
    pub fn code(&self) -> FieldCode {
        self.code
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
    #[error("the forward rule is not found in this endpoint")]
    ForwardRuleNotFound,
    #[error("the endpoint already has the maximum number of forward rules")]
    ForwardRuleLimit,
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
    #[error("secrets are not configured on this instance")]
    SecretsDisabled,
    #[error("secret storage failed: {0}")]
    SecretStorage(SecretStoreError),
    #[error("a secret could not be sealed: {0}")]
    SecretSeal(SealError),
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

impl From<ForwardStoreError> for ApiError {
    fn from(error: ForwardStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            Self::ForwardStorage(error)
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

impl From<AuthRejection> for ApiError {
    fn from(rejection: AuthRejection) -> Self {
        Self::Auth(rejection)
    }
}

impl From<EndpointError> for ApiError {
    fn from(error: EndpointError) -> Self {
        match error {
            EndpointError::Unavailable(_) => Self::StoreUnavailable,
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
        let retry_after = match &self {
            Self::RateLimited { retry_after_secs } => Some(*retry_after_secs),
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
            Self::Auth(_) => (
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
            Self::TokenCreationRate { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "quota_exceeded",
                self.to_string(),
                None,
            ),
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
            Self::SecretNotFound => (
                StatusCode::NOT_FOUND,
                "secret_not_found",
                self.to_string(),
                None,
            ),
            Self::SecretLimit => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit_reached",
                self.to_string(),
                None,
            ),
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
            Self::SecretSeal(error) => {
                tracing::error!(%error, "a secret could not be sealed");
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
