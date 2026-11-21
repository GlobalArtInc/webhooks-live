use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use whl_core::capture_id::CaptureId;
use whl_store::Uuid;
use whl_store::rule_exec::{RerunOutcome, RunFilter, RunRecord};

use super::handlers::{endpoint_id, live_endpoint};
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;
const CAPTURE_RUNS: i64 = 10;
const STATES: [&str; 5] = ["sync", "queued", "running", "succeeded", "failed"];

#[derive(Debug, Deserialize)]
pub struct RunListParams {
    limit: Option<i64>,
    cursor: Option<String>,
    state: Option<String>,
}

#[derive(Serialize)]
pub struct RuleRunDto {
    id: String,
    capture_id: String,
    trigger: String,
    state: String,
    rules_version: i32,
    response_status: Option<i16>,
    error: Option<String>,
    entries: Value,
    created_at: String,
    finished_at: Option<String>,
}

impl From<&RunRecord> for RuleRunDto {
    fn from(record: &RunRecord) -> Self {
        Self {
            id: record.id.hyphenated().to_string(),
            capture_id: record.capture_id.hyphenated().to_string(),
            trigger: record.trigger.clone(),
            state: record.state.clone(),
            rules_version: record.rules_version,
            response_status: record.response_status,
            error: record.error.clone(),
            entries: record.entries.clone(),
            created_at: record.created_at.clone(),
            finished_at: record.finished_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct RunListDto {
    items: Vec<RuleRunDto>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct CaptureRunsDto {
    items: Vec<RuleRunDto>,
}

fn invalid(field: &str, code: FieldCode, message: &str) -> ApiError {
    ApiError::Validation(vec![FieldViolation::new(field, code, message)])
}

fn parse_uuid(raw: &str) -> Option<Uuid> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
}

fn capture_uuid(raw: &str) -> Result<Uuid, ApiError> {
    CaptureId::parse(raw)
        .map(|id| id.unpack())
        .map_err(|_| ApiError::NotFound)
}

pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    Query(params): Query<RunListParams>,
) -> Result<Json<RunListDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(invalid(
            "limit",
            FieldCode::OutOfRange,
            "must be between 1 and 200",
        ));
    }
    let after = match params.cursor.as_deref() {
        Some(raw) => Some(parse_uuid(raw).ok_or(ApiError::InvalidCursor)?),
        None => None,
    };
    if let Some(name) = params.state.as_deref()
        && !STATES.contains(&name)
    {
        return Err(invalid(
            "state",
            FieldCode::InvalidFormat,
            "must be sync, queued, running, succeeded or failed",
        ));
    }
    let filter = RunFilter {
        capture: None,
        state: params.state.clone(),
        after,
    };
    let mut records = state
        .store
        .list_rule_runs(&endpoint, &filter, limit + 1)
        .await?;
    let more = records.len() > usize::try_from(limit).unwrap_or(usize::MAX);
    records.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let next_cursor = if more {
        records
            .last()
            .map(|record| record.id.hyphenated().to_string())
    } else {
        None
    };
    Ok(Json(RunListDto {
        items: records.iter().map(RuleRunDto::from).collect(),
        next_cursor,
    }))
}

pub async fn for_capture(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
) -> Result<Json<CaptureRunsDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let capture = capture_uuid(&raw_capture)?;
    live_endpoint(&state, &endpoint).await?;
    if !state.store.capture_exists(&endpoint, capture).await? {
        return Err(ApiError::CaptureNotFound);
    }
    let filter = RunFilter {
        capture: Some(capture),
        ..RunFilter::default()
    };
    let records = state
        .store
        .list_rule_runs(&endpoint, &filter, CAPTURE_RUNS)
        .await?;
    Ok(Json(CaptureRunsDto {
        items: records.iter().map(RuleRunDto::from).collect(),
    }))
}

pub async fn rerun(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let capture = capture_uuid(&raw_capture)?;
    if !state
        .rules
        .as_ref()
        .is_some_and(|runtime| runtime.settings().enabled)
    {
        return Err(ApiError::RulesDisabled);
    }
    let id = CaptureId::generate()
        .map(|id| id.unpack())
        .map_err(|_| ApiError::InternalFault("an id for a rule run cannot be generated"))?;
    match state
        .store
        .create_rule_rerun(&endpoint, state.settings.retention, capture, id)
        .await?
    {
        RerunOutcome::Created(record) => {
            Ok((StatusCode::ACCEPTED, Json(RuleRunDto::from(&*record))).into_response())
        }
        RerunOutcome::EndpointGone => Err(ApiError::EndpointGone),
        RerunOutcome::CaptureNotFound => Err(ApiError::CaptureNotFound),
        RerunOutcome::NoRules => Err(invalid(
            "rules",
            FieldCode::Required,
            "the endpoint has no enabled rules",
        )),
        RerunOutcome::Active => Err(ApiError::RerunActive),
        RerunOutcome::TooMany => Err(ApiError::RerunLimit),
    }
}
