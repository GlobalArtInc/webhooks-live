use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::{CaptureError, DeleteOutcome};

use super::cursor::Cursor;
use super::dto::{CaptureDto, ListDto, SummaryDto};
use super::query::{ListParams, list_query};
use crate::error::ApiError;
use crate::state::ApiState;

impl From<CaptureError> for ApiError {
    fn from(error: CaptureError) -> Self {
        match error {
            CaptureError::Unavailable(_) => Self::StoreUnavailable,
            CaptureError::Query(source) => Self::Internal(whl_store::EndpointError::Query(source)),
            CaptureError::Invalid { reason } => Self::Internal(whl_store::EndpointError::Invalid {
                id: String::new(),
                reason,
            }),
        }
    }
}

pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let query = list_query(&params)?;
    let page = state.store.list_captures(&endpoint, &query).await?;
    let next_cursor = page.next_position.map(|position| {
        Cursor {
            order: query.order,
            position,
            fingerprint: query.filter.fingerprint(),
        }
        .encode()
    });
    Ok(Json(ListDto {
        items: page.items.iter().map(SummaryDto::from).collect(),
        next_cursor,
        total: page.total,
    }))
}

pub async fn latest(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<CaptureDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let detail = state
        .store
        .latest_capture(&endpoint)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    Ok(Json(CaptureDto::from(detail)))
}

pub async fn show(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
) -> Result<Json<CaptureDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let capture = capture_id(&raw_capture)?;
    let detail = state
        .store
        .get_capture(&endpoint, &capture)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    Ok(Json(CaptureDto::from(detail)))
}

pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let capture = capture_id(&raw_capture)?;
    match state
        .store
        .delete_capture(&endpoint, &capture, state.settings.retention)
        .await?
    {
        DeleteOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteOutcome::CaptureNotFound => Err(ApiError::CaptureNotFound),
        DeleteOutcome::EndpointGone => Err(ApiError::EndpointGone),
    }
}

pub async fn clear(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    if state
        .store
        .clear_captures(&endpoint, state.settings.retention)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::EndpointGone)
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

fn capture_id(raw: &str) -> Result<CaptureId, ApiError> {
    CaptureId::parse(raw).map_err(|_| ApiError::NotFound)
}
