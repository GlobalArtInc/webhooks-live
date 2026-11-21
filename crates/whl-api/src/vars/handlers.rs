use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use whl_core::endpoint_id::EndpointId;
use whl_store::vars::{DeleteVarOutcome, PutVarOutcome};

use super::dto::{VarDto, VarListDto};
use super::input::{is_valid_name, parse_name, parse_put};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

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
