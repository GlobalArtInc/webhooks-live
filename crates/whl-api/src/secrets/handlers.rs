use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, MAX_SECRETS_PER_ENDPOINT, SecretName};
use whl_store::secrets::{DeleteSecretOutcome, PutSecretOutcome};

use super::dto::{SecretDto, SecretInputDoc, SecretListDto};
use super::input::parse_put;
use super::references;
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

#[utoipa::path(
    get,
    path = "/endpoints/{id}/secrets",
    operation_id = "secrets_list",
    tag = "Secrets",
    summary = "List the secret names of an endpoint",
    description = "Names and timestamps only; values are never returned. Needs an editor on the endpoint; a token needs the `read` scope. The feature answers 503 `feature_disabled` while the instance has no secrets key.",
    security(("session_cookie" = []), ("api_token" = []), ()),
    params(EndpointIdParam),
    responses(
        (status = 200, description = "The secrets, by name", body = SecretListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `insufficient_scope`: the token lacks `read`", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: the instance has no secrets key; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
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

#[utoipa::path(
    put,
    path = "/endpoints/{id}/secrets/{name}",
    operation_id = "secrets_put",
    tag = "Secrets",
    summary = "Create or replace a secret",
    description = "Stores the value encrypted and returns the name and timestamps only. Editor role; a token needs the `endpoints:write` scope. The name must match `^[A-Z][A-Z0-9_]{0,63}$` (422 on `name`). At most 20 secrets per endpoint: a new name beyond that answers 422 `limit_reached` with `error.limit` = `{name: \"secrets\", max: 20}`. Field codes on `value`: `required` when absent or empty, `invalid_format` when not a string, `too_long` above 4096 bytes; other keys are `not_allowed`. Answers 503 `feature_disabled` while the instance has no secrets key.",
    security(("session_cookie" = []), ("api_token" = []), ()),
    params(
        EndpointIdParam,
        ("name" = String, Path, pattern = "^[A-Z][A-Z0-9_]{0,63}$", description = "Secret name")
    ),
    request_body(content = SecretInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The stored secret's name and timestamps", body = SecretDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `insufficient_scope`: the token lacks `endpoints:write`; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`) or `limit_reached` (`error.limit` = `{name: \"secrets\", max: 20}`)", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: the instance has no secrets key; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/secrets/{name}",
    operation_id = "secrets_delete",
    tag = "Secrets",
    summary = "Delete a secret",
    description = "Editor role; a token needs the `endpoints:write` scope. A name that does not match the secret pattern answers 404 `secret_not_found`, as does an unknown name. Answers 409 `secret_in_use` while anything on the endpoint still reads the secret: its verifier, a rule step or condition (enabled or not), a Telegram channel (`token_secret`) or a schedule URL or header. `error.references` lists every holder as `{kind, id, name?}` with `kind` one of `verifier` (`id` is the endpoint id, no `name`), `rule`, `channel`, `schedule` (`id` and `name` of the object). The check and the delete run in one transaction under the endpoint lock.",
    security(("session_cookie" = []), ("api_token" = []), ()),
    params(
        EndpointIdParam,
        ("name" = String, Path, description = "Secret name")
    ),
    responses(
        (status = 204, description = "The secret is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `insufficient_scope`: the token lacks `endpoints:write`; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id; `secret_not_found`: no such secret", body = ErrorBody),
        (status = 409, description = "`secret_in_use`: the verifier, a rule, a Telegram channel or a schedule of the endpoint reads the secret; `error.references` lists them as `{kind: verifier|rule|channel|schedule, id, name?}`", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: the instance has no secrets key; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    keyring(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let name = SecretName::parse(&raw_name).map_err(|_| ApiError::SecretNotFound)?;
    match state
        .store
        .delete_secret(&endpoint, state.settings.retention, &name, |snapshot| {
            references::find(snapshot, &name)
        })
        .await?
    {
        DeleteSecretOutcome::Deleted => {
            tracing::info!(endpoint = %endpoint, secret = %name, "secret deleted");
            Ok(StatusCode::NO_CONTENT)
        }
        DeleteSecretOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteSecretOutcome::NotFound => Err(ApiError::SecretNotFound),
        DeleteSecretOutcome::InUse(references) => Err(ApiError::SecretInUse { references }),
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
