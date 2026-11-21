use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use whl_auth::{ApiTokenSecret, AuthRejection, Caller};
use whl_store::{CreateTokenOutcome, NewApiToken, RevokeTokenOutcome, TokenCaps, UserRecord, Uuid};

use super::dto::{ApiTokenDto, ApiTokenListDto, CreatedApiTokenDto};
use super::input::parse_token_input;
use super::{MAX_TOKENS_PER_HOUR, MAX_TOKENS_PER_USER};
use crate::body::RawBody;
use crate::error::ApiError;
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

pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let user = session_user(&state, &caller)?;
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
                max_active: MAX_TOKENS_PER_USER,
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
        CreateTokenOutcome::TooMany => Err(ApiError::TokenLimit),
        CreateTokenOutcome::TooFast { resets_at } => Err(ApiError::TokenCreationRate { resets_at }),
        CreateTokenOutcome::UserGone => Err(AuthRejection::Unauthenticated.into()),
    }
}

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
