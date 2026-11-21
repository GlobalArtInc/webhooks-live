use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, MAX_SECRETS_PER_ENDPOINT, SecretName};
use whl_store::secrets::{DeleteSecretOutcome, PutSecretOutcome};

use super::dto::{SecretDto, SecretListDto};
use super::input::parse_put;
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<SecretListDto>, ApiError> {
    keyring(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let secrets = state.store.list_secrets(&endpoint).await?;
    Ok(Json(SecretListDto {
        items: secrets.iter().map(SecretDto::from).collect(),
    }))
}

pub async fn put(
    State(state): State<ApiState>,
    Path((raw_id, raw_name)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<SecretDto>, ApiError> {
    let keyring = keyring(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let name = SecretName::parse(&raw_name).map_err(|error| {
        ApiError::Validation(vec![FieldViolation::new(
            "name",
            FieldCode::InvalidFormat,
            error.to_string(),
        )])
    })?;
    let value = parse_put(&body)?;
    let sealed = keyring
        .seal(&endpoint, &name, &value)
        .map_err(ApiError::SecretSeal)?;
    match state
        .store
        .put_secret(
            &endpoint,
            state.settings.retention,
            &name,
            &sealed,
            MAX_SECRETS_PER_ENDPOINT,
        )
        .await?
    {
        PutSecretOutcome::Stored(meta) => {
            tracing::info!(endpoint = %endpoint, secret = %name, "secret stored");
            Ok(Json(SecretDto::from(&meta)))
        }
        PutSecretOutcome::EndpointGone => Err(ApiError::EndpointGone),
        PutSecretOutcome::LimitReached => Err(ApiError::SecretLimit),
    }
}

pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    keyring(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let name = SecretName::parse(&raw_name).map_err(|_| ApiError::SecretNotFound)?;
    match state
        .store
        .delete_secret(&endpoint, state.settings.retention, &name)
        .await?
    {
        DeleteSecretOutcome::Deleted => {
            tracing::info!(endpoint = %endpoint, secret = %name, "secret deleted");
            Ok(StatusCode::NO_CONTENT)
        }
        DeleteSecretOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteSecretOutcome::NotFound => Err(ApiError::SecretNotFound),
    }
}

fn keyring(state: &ApiState) -> Result<&Keyring, ApiError> {
    state.secrets.as_deref().ok_or(ApiError::SecretsDisabled)
}

async fn live_endpoint(state: &ApiState, endpoint: &EndpointId) -> Result<(), ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .map(|_| ())
        .ok_or(ApiError::EndpointGone)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
