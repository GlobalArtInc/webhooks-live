use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use whl_store::AuthStoreError;

const LONG_RETRY_AFTER: &str = "30";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("{key} is required when WEBHOOKS_LIVE_AUTH_MODE is sso")]
    MissingConfig { key: &'static str },
    #[error("WEBHOOKS_LIVE_OIDC_ISSUER must use https, or http only for a loopback host: {issuer}")]
    InsecureIssuer { issuer: String },
    #[error("{key} is not usable: {reason}")]
    InvalidConfig { key: &'static str, reason: String },
    #[error("cannot build the HTTP client for the identity provider: {0}")]
    HttpClient(String),
    #[error("OIDC discovery against {issuer} failed: {reason}")]
    Discovery { issuer: String, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LoginFailure {
    #[error("the login state is missing, expired, or does not belong to this browser")]
    InvalidState,
    #[error("the identity provider refused the sign-in")]
    ProviderDenied,
    #[error("the callback request is malformed")]
    InvalidRequest,
    #[error("the authorization code could not be exchanged")]
    ExchangeFailed,
    #[error("the ID token did not verify")]
    InvalidToken,
    #[error("the ID token carries no email address")]
    MissingEmail,
    #[error("the identity provider has not verified the email address")]
    EmailNotVerified,
    #[error("the ID token carries an email_verified claim that is not a boolean")]
    EmailVerifiedInvalid,
    #[error("too many sign-ins are pending")]
    Busy,
    #[error("the sign-in could not be completed right now")]
    Unavailable,
    #[error("the sign-in failed unexpectedly")]
    Internal,
}

impl LoginFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidState => "invalid_state",
            Self::ProviderDenied => "provider_denied",
            Self::InvalidRequest => "invalid_request",
            Self::ExchangeFailed => "exchange_failed",
            Self::InvalidToken => "invalid_token",
            Self::MissingEmail => "missing_email",
            Self::EmailNotVerified => "email_not_verified",
            Self::EmailVerifiedInvalid => "email_verified_invalid",
            Self::Busy => "busy",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal_error",
        }
    }
}

impl From<AuthStoreError> for LoginFailure {
    fn from(error: AuthStoreError) -> Self {
        match error {
            AuthStoreError::Unavailable(_) => Self::Unavailable,
            AuthStoreError::LoginsFull => Self::Busy,
            AuthStoreError::Query(_) | AuthStoreError::Invalid(_) => Self::Internal,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthRejection {
    #[error("sign in to use this route")]
    Unauthenticated,
    #[error("the Origin of this request is not the public origin")]
    CsrfRejected,
    #[error("this route needs sso mode")]
    NotAvailableInMode,
    #[error("the method is not allowed on this route")]
    MethodNotAllowed,
    #[error("the resource was not found")]
    NotFound,
    #[error("the database is unavailable")]
    StoreUnavailable,
    #[error("sign-in is not available right now")]
    AuthUnavailable,
    #[error("too many sign-ins are pending, try again shortly")]
    LoginBusy,
    #[error("the request could not be completed")]
    Internal,
}

impl From<AuthStoreError> for AuthRejection {
    fn from(error: AuthStoreError) -> Self {
        match error {
            AuthStoreError::Unavailable(_) => Self::StoreUnavailable,
            AuthStoreError::LoginsFull => Self::LoginBusy,
            other @ (AuthStoreError::Query(_) | AuthStoreError::Invalid(_)) => {
                tracing::error!(%other, "auth storage failed");
                Self::Internal
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AccessDenied {
    #[error("the endpoint is unknown, expired or deleted")]
    Gone,
    #[error("your role on this endpoint does not allow this")]
    Forbidden,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

pub(crate) fn error_response(status: StatusCode, code: &'static str, message: String) -> Response {
    let mut response = (
        status,
        Json(ErrorBody {
            error: ErrorDetail { code, message },
        }),
    )
        .into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == StatusCode::SERVICE_UNAVAILABLE {
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
    }
    response
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            Self::CsrfRejected => (StatusCode::FORBIDDEN, "csrf_rejected"),
            Self::NotAvailableInMode => (StatusCode::NOT_FOUND, "not_available_in_mode"),
            Self::MethodNotAllowed => (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::StoreUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "store_unavailable"),
            Self::AuthUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "auth_unavailable"),
            Self::LoginBusy => (StatusCode::SERVICE_UNAVAILABLE, "login_busy"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        let mut response = error_response(status, code, self.to_string());
        if matches!(self, Self::AuthUnavailable | Self::LoginBusy) {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_static(LONG_RETRY_AFTER),
            );
        }
        response
    }
}

impl IntoResponse for AccessDenied {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Gone => (StatusCode::GONE, "endpoint_gone"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
        };
        error_response(status, code, self.to_string())
    }
}
