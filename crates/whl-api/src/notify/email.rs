use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use whl_auth::{AuthRejection, Caller};
use whl_notify::{
    AddressError, MAX_ADDRESS_BYTES, Mailer, VerificationToken, address_key, parse_address,
    verification_mail, verify_disable_token,
};
use whl_store::Uuid;
use whl_store::notify::{AddressLimits, AddressOutcome, AddressRequest, AddressUndo, EmailAddress};

use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
    ValidationFailed,
};
use crate::state::ApiState;

pub const MAX_ADDRESSES_PER_USER: usize = 5;
pub const VERIFICATIONS_PER_USER_PER_DAY: i32 = 5;
pub const VERIFICATIONS_PER_ADDRESS_PER_DAY: i32 = 3;
pub const VERIFICATIONS_PER_DOMAIN_PER_DAY: i32 = 200;
pub const VERIFICATIONS_PER_DAY_GLOBAL: i32 = 2000;
pub const RESEND_COOLDOWN: Duration = Duration::from_secs(60);
pub const VERIFICATION_LIFETIME: Duration = Duration::from_secs(24 * 3600);

const THROTTLE_RETRY_AFTER_SECS: u64 = 3600;
const TOKEN_KEYS: &[&str] = &["token"];
const ADDRESS_KEYS: &[&str] = &["address"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailApiLimits {
    pub max_addresses: usize,
    pub verifications_per_user_per_day: i32,
    pub verifications_per_address_per_day: i32,
    pub verifications_per_domain_per_day: i32,
    pub verifications_per_day_global: i32,
    pub resend_cooldown: Duration,
}

impl Default for EmailApiLimits {
    fn default() -> Self {
        Self {
            max_addresses: MAX_ADDRESSES_PER_USER,
            verifications_per_user_per_day: VERIFICATIONS_PER_USER_PER_DAY,
            verifications_per_address_per_day: VERIFICATIONS_PER_ADDRESS_PER_DAY,
            verifications_per_domain_per_day: VERIFICATIONS_PER_DOMAIN_PER_DAY,
            verifications_per_day_global: VERIFICATIONS_PER_DAY_GLOBAL,
            resend_cooldown: RESEND_COOLDOWN,
        }
    }
}

impl EmailApiLimits {
    fn to_store(self) -> AddressLimits {
        AddressLimits {
            max_addresses: i64::try_from(self.max_addresses).unwrap_or(i64::MAX),
            verifications_per_user_per_day: self.verifications_per_user_per_day,
            verifications_per_address_per_day: self.verifications_per_address_per_day,
            verifications_per_domain_per_day: self.verifications_per_domain_per_day,
            verifications_per_day_global: self.verifications_per_day_global,
            resend_cooldown_secs: i64::try_from(self.resend_cooldown.as_secs()).unwrap_or(i64::MAX),
        }
    }
}

#[derive(Clone)]
pub struct EmailApi {
    pub mailer: Mailer,
    pub limits: EmailApiLimits,
}

impl EmailApi {
    pub fn new(mailer: Mailer, limits: EmailApiLimits) -> Self {
        Self { mailer, limits }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = EmailAddress)]
pub struct EmailAddressDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d72"
    )]
    id: String,
    #[schema(max_length = 254, example = "ada@example.com")]
    address: String,
    verified: bool,
    #[schema(format = DateTime)]
    created_at: String,
}

impl From<&EmailAddress> for EmailAddressDto {
    fn from(address: &EmailAddress) -> Self {
        Self {
            id: address.id.hyphenated().to_string(),
            address: address.address.clone(),
            verified: address.verified,
            created_at: address.created_at.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = EmailAddressList)]
pub struct EmailAddressListDto {
    items: Vec<EmailAddressDto>,
}

#[derive(ToSchema)]
#[schema(as = EmailAddressInput)]
#[allow(dead_code)]
pub struct EmailAddressInputDoc {
    #[schema(max_length = 254, example = "ada@example.com")]
    address: String,
}

#[derive(ToSchema)]
#[schema(
    as = LinkTokenInput,
    description = "The token of a verification or unsubscribe link. It may be sent as `?token=` instead of in the body."
)]
#[allow(dead_code)]
pub struct LinkTokenInputDoc {
    token: String,
}

#[derive(Deserialize)]
pub struct TokenQuery {
    token: Option<String>,
}

fn email_api(state: &ApiState) -> Result<&EmailApi, ApiError> {
    if !state.notifications.enabled {
        return Err(ApiError::NotFound);
    }
    state.email.as_ref().ok_or(ApiError::NotFound)
}

pub(super) fn signed_in_user(state: &ApiState, caller: &Caller) -> Result<Uuid, ApiError> {
    caller.user().map(|user| user.id).ok_or_else(|| {
        ApiError::Auth(if state.access.is_mixed() {
            AuthRejection::SignInRequired
        } else {
            AuthRejection::Unauthenticated
        })
    })
}

fn object_of(bytes: &[u8], known: &[&str]) -> Result<Map<String, Value>, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let Value::Object(object) = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?
    else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let violations: Vec<FieldViolation> = object
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .map(|key| FieldViolation::new(key.as_str(), FieldCode::NotAllowed, "is not a known field"))
        .collect();
    if violations.is_empty() {
        Ok(object)
    } else {
        Err(ApiError::Validation(violations))
    }
}

fn text_field(object: &Map<String, Value>, key: &str) -> Result<String, ApiError> {
    match object.get(key) {
        None | Some(Value::Null) => Err(ApiError::Validation(vec![FieldViolation::new(
            key,
            FieldCode::Required,
            "is required",
        )])),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(ApiError::Validation(vec![FieldViolation::new(
            key,
            FieldCode::InvalidFormat,
            "must be a string",
        )])),
    }
}

fn address_violation(error: AddressError) -> ApiError {
    let code = match error {
        AddressError::Empty => FieldCode::Required,
        AddressError::TooLong => FieldCode::TooLong,
        AddressError::Format => FieldCode::InvalidFormat,
    };
    ApiError::Validation(vec![FieldViolation::new(
        "address",
        code,
        format!("{error} (at most {MAX_ADDRESS_BYTES} bytes)"),
    )])
}

fn address_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    get,
    path = "/email-addresses",
    operation_id = "notifications_list_addresses",
    tag = "Notifications",
    summary = "List the caller's e-mail addresses",
    description = "Signed in only. Verified and unverified addresses of the account, used by e-mail channels. Answers 404 `not_found` when e-mail is not configured or notifications are switched off.",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The addresses", body = EmailAddressListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`sign_in_required`: a `mixed` instance and no account; `session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: e-mail is not configured or notifications are switched off", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<EmailAddressListDto>, ApiError> {
    email_api(&state)?;
    let user = signed_in_user(&state, &caller)?;
    let addresses = state.store.list_email_addresses(user).await?;
    Ok(Json(EmailAddressListDto {
        items: addresses.iter().map(EmailAddressDto::from).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/email-addresses",
    operation_id = "notifications_add_address",
    tag = "Notifications",
    summary = "Add an e-mail address and send its verification mail",
    description = "Signed in only. A new address answers 201 and gets a verification mail, valid for 24 hours. An address already on the account answers 200 with its current state; an unverified one gets a new mail when the resend limits allow it, also 200. At most 5 addresses per account. Confirm with `POST /email-addresses/verify`.",
    security(("session_cookie" = [])),
    request_body(content = EmailAddressInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The address is already on the account; a new mail may have been sent", body = EmailAddressDto),
        (status = 201, description = "The address is new and its verification mail is sent", body = EmailAddressDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`csrf_rejected`: the Origin check failed; `session_required`: a Bearer token was used; `sign_in_required`: a `mixed` instance and no account", body = ErrorBody),
        (status = 404, description = "`not_found`: e-mail is not configured or notifications are switched off", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (`error.fields`, field `address`: `required`, `too_long` or `invalid_format`); `limit_reached` (`error.limit` = `{name: \"email_addresses\", max: 5}`)", body = ErrorBody),
        (status = 429, description = "`rate_limited`: the verification mail limits per account, address, domain or instance are used up (`Retry-After: 3600`)", body = ErrorBody),
        (status = 503, description = "`mail_unavailable`: the verification mail could not be sent and the request is undone (`Retry-After: 5`); `store_unavailable`", body = ErrorBody)
    )
)]
pub async fn add(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let email = email_api(&state)?;
    let user = signed_in_user(&state, &caller)?;
    let object = object_of(&body, ADDRESS_KEYS)?;
    let address = parse_address(&text_field(&object, "address")?).map_err(address_violation)?;
    let token = VerificationToken::generate()
        .map_err(|_| ApiError::InternalFault("a verification token could not be generated"))?;
    let digest = token.digest();
    let key = address_key(&address);
    let request = AddressRequest {
        id: Uuid::now_v7(),
        address: &address,
        key: &key.mailbox,
        domain: &key.domain,
        token_hash: &digest,
        lifetime: VERIFICATION_LIFETIME,
    };
    let outcome = state
        .store
        .request_email_address(user, &request, &email.limits.to_store())
        .await?;
    match outcome {
        AddressOutcome::Created {
            address,
            verification,
        } => {
            if let Some(undo) = verification {
                send_verification(&state, email, &address, &token, &undo).await?;
            }
            Ok((StatusCode::CREATED, Json(EmailAddressDto::from(&address))).into_response())
        }
        AddressOutcome::Resent { address, undo } => {
            send_verification(&state, email, &address, &token, &undo).await?;
            Ok((StatusCode::OK, Json(EmailAddressDto::from(&address))).into_response())
        }
        AddressOutcome::Existing(address) => {
            Ok((StatusCode::OK, Json(EmailAddressDto::from(&address))).into_response())
        }
        AddressOutcome::LimitReached => {
            Err(ApiError::EmailAddressLimit(email.limits.max_addresses))
        }
        AddressOutcome::Throttled => Err(ApiError::RateLimited {
            retry_after_secs: THROTTLE_RETRY_AFTER_SECS,
        }),
        AddressOutcome::UserGone => Err(ApiError::Auth(AuthRejection::Unauthenticated)),
    }
}

async fn send_verification(
    state: &ApiState,
    email: &EmailApi,
    address: &EmailAddress,
    token: &VerificationToken,
    undo: &AddressUndo,
) -> Result<(), ApiError> {
    let mail = verification_mail(&state.settings.public_url, &address.address, token);
    match email.mailer.send(&mail).await {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(address_id = %address.id, %error, "a verification email was not sent");
            if let Err(undo_error) = state.store.undo_email_request(undo).await {
                tracing::error!(address_id = %address.id, error = %undo_error, "cannot undo a verification request after a send failure");
            }
            Err(ApiError::MailUnavailable)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/email-addresses/{address_id}",
    operation_id = "notifications_remove_address",
    tag = "Notifications",
    summary = "Remove an e-mail address",
    description = "Signed in only. Only the caller's own addresses can be removed; another account's id answers 404 `email_address_not_found`.",
    security(("session_cookie" = [])),
    params(("address_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Address id, lowercase hyphenated UUID")),
    responses(
        (status = 204, description = "The address is removed"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`csrf_rejected`: the Origin check failed; `session_required`: a Bearer token was used; `sign_in_required`: a `mixed` instance and no account", body = ErrorBody),
        (status = 404, description = "`email_address_not_found`: unknown id or an address of another account; `not_found`: malformed id or e-mail is not configured", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn remove(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    email_api(&state)?;
    let user = signed_in_user(&state, &caller)?;
    let id = address_id(&raw_id)?;
    if state.store.delete_email_address(user, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::EmailAddressNotFound)
    }
}

#[utoipa::path(
    post,
    path = "/email-addresses/verify",
    operation_id = "notifications_verify_address",
    tag = "Notifications",
    summary = "Confirm an e-mail address from its verification link",
    description = "Public: no sign-in. The token comes from the verification mail, in the JSON body or as `?token=`; the body may be empty or of any type and is ignored when `?token=` is present. A used, expired or unknown token answers 404 `invalid_token`.",
    security(()),
    params(("token" = Option<String>, Query, description = "Token from the verification link; when present the body is ignored")),
    request_body(content = Option<LinkTokenInputDoc>, content_type = "application/json"),
    responses(
        (status = 200, description = "The address is verified", body = EmailAddressDto),
        (status = 400, response = BadRequest),
        (status = 404, description = "`invalid_token`: the token is unknown, used or expired; `not_found`: e-mail is not configured or notifications are switched off", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn verify(
    State(state): State<ApiState>,
    Query(query): Query<TokenQuery>,
    body: Bytes,
) -> Result<Json<EmailAddressDto>, ApiError> {
    email_api(&state)?;
    let text = token_text(query, &body)?;
    let token = VerificationToken::parse(&text).ok_or(ApiError::InvalidToken)?;
    match state.store.verify_email_address(&token.digest()).await? {
        Some(address) => {
            tracing::info!(address_id = %address.id, "an email address was verified");
            Ok(Json(EmailAddressDto::from(&address)))
        }
        None => Err(ApiError::InvalidToken),
    }
}

fn token_text(query: TokenQuery, body: &[u8]) -> Result<String, ApiError> {
    match query.token {
        Some(token) => Ok(token),
        None => text_field(&object_of(body, TOKEN_KEYS)?, "token"),
    }
}

#[utoipa::path(
    post,
    path = "/notifications/disable",
    operation_id = "notifications_disable",
    tag = "Notifications",
    summary = "Turn off an e-mail channel from its unsubscribe link",
    description = "Public: no sign-in. The token comes from the unsubscribe link of a notification mail, in the JSON body or as `?token=`; mail clients send the one-click `List-Unsubscribe-Post` request with the query form and a form-encoded body, which is ignored when `?token=` is present. Sets the e-mail channel the token belongs to to disabled; the channel is kept. Opening the token needs the secrets keyring.",
    security(()),
    params(("token" = Option<String>, Query, description = "Token from the unsubscribe link; when present the body is ignored")),
    request_body(content = Option<LinkTokenInputDoc>, content_type = "application/json"),
    responses(
        (status = 204, description = "The channel is disabled"),
        (status = 400, response = BadRequest),
        (status = 404, description = "`invalid_token`: the token is not valid, was used already or its channel is gone; `not_found`: notifications are switched off", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 503, description = "`feature_disabled`: no secrets keyring on this instance; `store_unavailable`", body = ErrorBody)
    )
)]
pub async fn disable(
    State(state): State<ApiState>,
    Query(query): Query<TokenQuery>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if !state.notifications.enabled {
        return Err(ApiError::NotFound);
    }
    let keyring = state.secrets.as_deref().ok_or(ApiError::SecretsDisabled)?;
    let token = token_text(query, &body)?;
    let channel = verify_disable_token(keyring, &token).map_err(|_| ApiError::InvalidToken)?;
    if state.store.disable_email_channel(channel).await? {
        tracing::info!(channel = %channel, "an email channel was disabled from its link");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::InvalidToken)
    }
}
