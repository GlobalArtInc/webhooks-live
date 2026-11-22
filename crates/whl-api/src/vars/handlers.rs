use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use whl_core::endpoint_id::EndpointId;
use whl_store::vars::{DeleteVarOutcome, PutVarOutcome};

use super::dto::{VarDto, VarInputDoc, VarListDto};
use super::input::{is_valid_name, parse_name, parse_put};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

#[utoipa::path(
    get,
    path = "/endpoints/{id}/vars",
    operation_id = "vars_list",
    tag = "Variables",
    summary = "List the persistent variables",
    description = "All persistent variables of the endpoint, by name. Session only: a Bearer token answers 403 `session_required`. Needs a viewer on the endpoint.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    responses(
        (status = 200, description = "The variables", body = VarListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<VarListDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let vars = state.store.list_vars(&endpoint).await?;
    Ok(Json(VarListDto {
        items: vars.iter().map(VarDto::from).collect(),
    }))
}

#[utoipa::path(
    put,
    path = "/endpoints/{id}/vars/{name}",
    operation_id = "vars_put",
    tag = "Variables",
    summary = "Create or replace a persistent variable",
    description = "Editor role; session only. The name must match `^[a-z_][a-z0-9_]{0,63}$` (422 on `name`). At most 50 variables per endpoint: a new name beyond that answers 422 `limit_reached` with `error.limit` = `{name: \"vars\", max: 50}`. Field codes on `value`: `required` when absent, `invalid_format` for a NUL character, `too_long` above 4 KiB serialized; other keys are `not_allowed`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("name" = String, Path, pattern = "^[a-z_][a-z0-9_]{0,63}$", description = "Variable name")
    ),
    request_body(content = VarInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The stored variable", body = VarDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`) or `limit_reached` (`error.limit` = `{name: \"vars\", max: 50}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn put(
    State(state): State<ApiState>,
    Path((raw_id, raw_name)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<VarDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let name = parse_name(&raw_name)?;
    let value = parse_put(&body)?;
    match state
        .store
        .put_var(&endpoint, state.settings.retention, name, &value)
        .await?
    {
        PutVarOutcome::Stored(record) => Ok(Json(VarDto::from(&record))),
        PutVarOutcome::EndpointGone => Err(ApiError::EndpointGone),
        PutVarOutcome::LimitReached => Err(ApiError::VarLimit),
        PutVarOutcome::Rejected => Err(ApiError::Validation(vec![FieldViolation::new(
            "value",
            FieldCode::InvalidFormat,
            "cannot be stored",
        )])),
    }
}

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/vars/{name}",
    operation_id = "vars_delete",
    tag = "Variables",
    summary = "Delete a persistent variable",
    description = "Editor role; session only. A name that does not match the variable pattern answers 404 `var_not_found`, as does an unknown name.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("name" = String, Path, description = "Variable name")
    ),
    responses(
        (status = 204, description = "The variable is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id; `var_not_found`: no such variable", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    if !is_valid_name(&raw_name) {
        return Err(ApiError::VarNotFound);
    }
    match state
        .store
        .delete_var(&endpoint, state.settings.retention, &raw_name)
        .await?
    {
        DeleteVarOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteVarOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteVarOutcome::NotFound => Err(ApiError::VarNotFound),
    }
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
