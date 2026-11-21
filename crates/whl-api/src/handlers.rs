use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_core::endpoint_id::EndpointId;

use crate::body::RawBody;
use crate::dto::{EndpointDto, InstanceDto};
use crate::error::ApiError;
use crate::input::parse_endpoint_input;
use crate::resolve::{defaults, resolve};
use crate::state::ApiState;

pub async fn instance(State(state): State<ApiState>) -> Json<InstanceDto> {
    Json(InstanceDto::new(&state.settings))
}

pub async fn create(
    State(state): State<ApiState>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let parsed = parse_endpoint_input(&body, true)?;
    let settings = resolve(
        &state.settings,
        &defaults(),
        &parsed.patch,
        parsed.violations,
    )
    .map_err(ApiError::Validation)?;
    let record = state
        .store
        .create_endpoint(&settings, state.settings.retention)
        .await?;
    let location = format!("/api/v1/endpoints/{}", record.id.unpack());
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(EndpointDto::from_record(&record, &state.settings, &[])),
    )
        .into_response())
}

pub async fn show(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<EndpointDto>, ApiError> {
    let id = endpoint_id(&raw_id)?;
    let record = state
        .store
        .get_endpoint(&id, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let rules = state.store.list_forward_rules(&id).await?;
    Ok(Json(EndpointDto::from_record(
        &record,
        &state.settings,
        &rules,
    )))
}

pub async fn update(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<EndpointDto>, ApiError> {
    let id = endpoint_id(&raw_id)?;
    let parsed = parse_endpoint_input(&body, false)?;
    let record = state
        .store
        .update_endpoint(&id, state.settings.retention, |current| {
            resolve(
                &state.settings,
                &current.settings,
                &parsed.patch,
                parsed.violations,
            )
            .map_err(ApiError::Validation)
        })
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let rules = state.store.list_forward_rules(&id).await?;
    Ok(Json(EndpointDto::from_record(
        &record,
        &state.settings,
        &rules,
    )))
}

pub async fn delete(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = endpoint_id(&raw_id)?;
    if state
        .store
        .delete_endpoint(&id, state.settings.retention)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::EndpointGone)
    }
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
