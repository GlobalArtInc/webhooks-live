use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use whl_core::capture_id::CaptureId;
use whl_store::Uuid;
use whl_store::rule_exec::{RerunOutcome, RunFilter, RunRecord};

use super::handlers::{endpoint_id, live_endpoint};
use crate::captures::CaptureIdParam;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{StoreUnavailable, Unauthenticated};
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

#[derive(Serialize, ToSchema)]
#[schema(as = RuleRun)]
pub struct RuleRunDto {
    id: String,
    capture_id: String,
    #[schema(pattern = "^(capture|rerun|dry_run)$")]
    trigger: String,
    #[schema(pattern = "^(sync|queued|running|succeeded|failed)$")]
    state: String,
    rules_version: i32,
    response_status: Option<i16>,
    error: Option<String>,
    #[schema(value_type = Vec<Object>)]
    entries: Value,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = CaptureRuleRunList)]
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
        Some(raw) => {
            let cursor = parse_uuid(raw).ok_or(ApiError::InvalidCursor)?;
            if !state.store.rule_run_in_endpoint(&endpoint, cursor).await? {
                return Err(ApiError::InvalidCursor);
            }
            Some(cursor)
        }
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

#[utoipa::path(
    get,
    path = "/endpoints/{id}/captures/{capture_id}/rule-runs",
    operation_id = "rule_runs_list_for_capture",
    tag = "Rules",
    summary = "The rule runs of one capture",
    description = "Newest first, at most 10. `trigger` is `capture` (the rules ran when the capture arrived) or `rerun`. `entries` is in execution order: a rule entry is {rule_id, rule_name, matched}; a step entry is {rule_id, step, name, kind, outcome, duration_ms, detail, output}, with `outcome` `ok`, `failed`, `skipped`, `deferred` or `no_op`. Needs a viewer on the endpoint; a token needs the `read` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of a view-protected anonymous endpoint")
    ),
    responses(
        (status = 200, description = "The runs of the capture", body = CaptureRunsDto),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    post,
    path = "/endpoints/{id}/captures/{capture_id}/rule-runs",
    operation_id = "rule_runs_rerun",
    tag = "Rules",
    summary = "Re-run the endpoint's rules on one capture",
    description = "Queues a run of the endpoint's enabled rules on this capture and answers 202 with the run (`trigger` `rerun`, `state` `queued`). Session only: a Bearer token is refused with 403 `session_required`. Needs an editor on the endpoint. At most 10 re-runs per capture. Answers 503 `feature_disabled` while rules are switched off by the operator.",
    security(("session_cookie" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of a view-protected anonymous endpoint")
    ),
    responses(
        (status = 202, description = "The queued run", body = RuleRunDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: the role is below editor; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 409, description = "`rerun_active`: a run of this capture is still sync, queued or running", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 422, description = "`validation_failed` (`rules`): the endpoint has no enabled rules; `limit_reached`: 10 re-runs of this capture already exist (`error.limit` = `{name: \"rule_reruns\", max: 10}`)", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: rules are switched off; `store_unavailable`: the database is down", body = ErrorBody)
    )
)]
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
