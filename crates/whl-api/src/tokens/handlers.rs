use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use whl_auth::{ApiTokenSecret, AuthRejection, Caller};
use whl_store::{CreateTokenOutcome, NewApiToken, RevokeTokenOutcome, TokenCaps, UserRecord, Uuid};

use super::MAX_TOKENS_PER_HOUR;
use super::dto::{ApiTokenDto, ApiTokenInputDoc, ApiTokenListDto, CreatedApiTokenDto};
use super::input::parse_token_input;
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

fn session_user<'a>(state: &ApiState, caller: &'a Caller) -> Result<&'a UserRecord, ApiError> {
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    match caller {
        Caller::Session { .. } => caller
            .user()
            .ok_or_else(|| AuthRejection::Unauthenticated.into()),
        Caller::Token { .. } => Err(AuthRejection::SessionRequired.into()),
        Caller::Anonymous { .. } => Err(AuthRejection::Unauthenticated.into()),
    }
}

#[utoipa::path(
    get,
    path = "/tokens",
    operation_id = "tokens_list",
    tag = "Tokens",
    summary = "List the caller's API tokens",
    description = "Includes revoked and expired tokens until the maintenance job removes them. The secret is never listed. `scopes` is a subset of `read`, `endpoints:write`, `captures:write`, `rules:write`, `send`; `endpoint_ids` null means no endpoint limit.",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The caller's tokens", body = ApiTokenListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<ApiTokenListDto>, ApiError> {
    let user = session_user(&state, &caller)?;
    let records = state.store.list_api_tokens(user.id).await?;
    Ok(Json(ApiTokenListDto {
        items: records.iter().map(ApiTokenDto::from).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/tokens",
    operation_id = "tokens_create",
    tag = "Tokens",
    summary = "Create an API token",
    description = "Expiry is 1 to 365 days (default 90). At most 20 active tokens and 10 creations per hour per user. `token`, the secret, is in this response only and never again.",
    security(("session_cookie" = [])),
    request_body(content = ApiTokenInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The token with its secret", body = CreatedApiTokenDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`) or `limit_reached` (`error.limit` = `{name: \"tokens\", max}`)", body = ErrorBody),
        (status = 429, description = "`quota_exceeded`: 10 creations per hour (`error.limit.name` = `token_creation`, `Retry-After` set); also `rate_limited`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let user = session_user(&state, &caller)?;
    let max_active = state.tier_of(&caller).limits().tokens;
    let input = parse_token_input(&body)?;
    let secret = ApiTokenSecret::generate().map_err(|_| ApiError::EntropyUnavailable)?;
    let digest = secret.digest();
    let outcome = state
        .store
        .create_api_token(
            user.id,
            &NewApiToken {
                name: &input.name,
                prefix: secret.lookup_prefix(),
                token_hash: digest.unpack(),
                scopes: &input.scopes,
                endpoint_ids: input.endpoint_ids.as_deref(),
                lifetime: input.lifetime,
            },
            TokenCaps {
                max_active,
                max_per_hour: MAX_TOKENS_PER_HOUR,
            },
        )
        .await?;
    match outcome {
        CreateTokenOutcome::Created(record) => {
            tracing::info!(token_id = %record.id, user_id = %user.id, "API token created");
            Ok((
                StatusCode::CREATED,
                Json(CreatedApiTokenDto::new(&record, secret.unpack())),
            )
                .into_response())
        }
        CreateTokenOutcome::TooMany => Err(ApiError::TokenLimit { max: max_active }),
        CreateTokenOutcome::TooFast { resets_at } => Err(ApiError::TokenCreationRate { resets_at }),
        CreateTokenOutcome::UserGone => Err(AuthRejection::Unauthenticated.into()),
    }
}

#[utoipa::path(
    delete,
    path = "/tokens/{token_id}",
    operation_id = "tokens_revoke",
    tag = "Tokens",
    summary = "Revoke an API token",
    description = "A token of another user, an unknown id and a malformed id all answer 404 `token_not_found`.",
    security(("session_cookie" = [])),
    params(("token_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Token id, lowercase hyphenated UUID")),
    responses(
        (status = 204, description = "The token is revoked"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 404, description = "`token_not_found`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn revoke(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = session_user(&state, &caller)?;
    let token_id = Uuid::parse_str(&raw_id).map_err(|_| ApiError::TokenNotFound)?;
    match state.store.revoke_api_token(user.id, token_id).await? {
        RevokeTokenOutcome::Revoked => {
            tracing::info!(%token_id, user_id = %user.id, "API token revoked");
            Ok(StatusCode::NO_CONTENT)
        }
        RevokeTokenOutcome::NotFound => Err(ApiError::TokenNotFound),
    }
}
