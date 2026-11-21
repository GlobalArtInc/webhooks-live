use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use whl_auth::{AuthRejection, Caller};
use whl_notify::{
    AddressError, MAX_ADDRESS_BYTES, Mailer, VerificationToken, address_key, parse_address,
    verification_mail, verify_disable_token,
};
use whl_store::Uuid;
use whl_store::notify::{AddressLimits, AddressOutcome, AddressRequest, AddressUndo, EmailAddress};

use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
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

#[derive(Serialize)]
pub struct EmailAddressDto {
    id: String,
    address: String,
    verified: bool,
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

#[derive(Serialize)]
pub struct EmailAddressListDto {
    items: Vec<EmailAddressDto>,
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
