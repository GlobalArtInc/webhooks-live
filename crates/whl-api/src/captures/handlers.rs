use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::Uuid;
use whl_store::rule_exec::RunFilter;
use whl_store::{CaptureDetail, CaptureError, DeleteOutcome};

use super::cursor::Cursor;
use super::doc::{CaptureIdParam, CaptureListQuery};
use super::dto::{CaptureDto, ListDto, SummaryDto};
use super::query::{ListParams, list_query};
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{StoreUnavailable, ValidationFailed};
use crate::state::ApiState;

const RULE_RUNS_IN_DETAIL: i64 = 10;

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

#[utoipa::path(
    get,
    path = "/endpoints/{id}/captures",
    operation_id = "captures_list",
    tag = "Captures",
    summary = "List the captures of an endpoint",
    description = "Newest first unless `order=asc`; `limit` 1 to 200, default 50. `next_cursor` is null on the last page; a cursor works only with the same `order` and filters. `total` counts every capture that matches the filters, ignoring the cursor and `after_seq`. Filters: `method` and `kind` are comma lists (`kind` of `http`, `email`, `dns`, `schedule`); `status` is a comma list of `1xx` to `5xx` or `none`; `starred` is `true` or `false`; `q` is a case-insensitive substring of the path, query, headers and the first 256 KiB of the body (1 to 200 characters); `after_seq` keeps captures with a greater `seq`; `since` (inclusive) and `until` (exclusive) bound `received_at` in RFC 3339. Reconnect catch-up: `after_seq=<highest seq held>&order=asc`, following `next_cursor`. Needs a viewer on the endpoint; a token needs the `read` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureListQuery,
    ),
    responses(
        (status = 200, description = "A page of capture summaries", body = ListDto),
        (status = 400, description = "`invalid_cursor`: malformed cursor, or used with another order or other filters", body = ErrorBody),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    get,
    path = "/endpoints/{id}/captures/latest",
    operation_id = "captures_latest",
    tag = "Captures",
    summary = "The newest capture of an endpoint",
    description = "Same shape as `GET .../captures/{capture_id}`. Answers 404 `capture_not_found` when the endpoint holds no capture. Needs a viewer on the endpoint; a token needs the `read` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
    ),
    responses(
        (status = 200, description = "The newest capture", body = CaptureDto),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: the endpoint holds no capture; `not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
    with_forwards(&state, &endpoint, detail).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/captures/{capture_id}",
    operation_id = "captures_get",
    tag = "Captures",
    summary = "One capture with its details",
    description = "The summary plus `url` (as the server saw it), `host`, `headers` and `query` (`{name, value}` lists in original order), `form` (`{fields, files}`, null unless the body was a parsed form), `note`, `body` (`{encoding, data, truncated}` of the first 1 MiB; the whole body is at `/body`), `forwards` newest first, `notifications` and the 10 newest `rule_runs`. `meta` holds the fields of the capture's kind: `http` {hop}; `email` {envelope_from, envelope_to, helo, tls, tag, subject, from, to, cc, date, message_id, parts}; `dns` {name, type, class, transport, dns_id, flags, edns_udp_size, client_subnet}; `schedule` {schedule_id, schedule_name, request, duration_ms, final_url, error, truncated}. Needs a viewer on the endpoint; a token needs the `read` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
    ),
    responses(
        (status = 200, description = "The capture", body = CaptureDto),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
    with_forwards(&state, &endpoint, detail).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/captures/{capture_id}",
    operation_id = "captures_delete",
    tag = "Captures",
    summary = "Delete one capture",
    description = "Removes the capture with its forward jobs and deliveries. Needs an editor on the endpoint; a token needs the `captures:write` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
    ),
    responses(
        (status = 204, description = "The capture is deleted"),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`forbidden`: the role is below editor; `insufficient_scope`: the token lacks `captures:write` or is limited to other endpoints; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/captures",
    operation_id = "captures_clear",
    tag = "Captures",
    summary = "Delete every capture of an endpoint",
    description = "Removes all captures of the endpoint. Needs an editor on the endpoint; a token needs the `captures:write` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
    ),
    responses(
        (status = 204, description = "The captures are deleted"),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`forbidden`: the role is below editor; `insufficient_scope`: the token lacks `captures:write` or is limited to other endpoints; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

async fn with_forwards(
    state: &ApiState,
    endpoint: &EndpointId,
    detail: CaptureDetail,
) -> Result<CaptureDto, ApiError> {
    let capture = Uuid::try_parse(&detail.summary.id).map_err(|_| {
        ApiError::Internal(whl_store::EndpointError::Invalid {
            id: detail.summary.id.clone(),
            reason: "the capture id is not a UUID".to_owned(),
        })
    })?;
    let forwards = state.store.forward_jobs_for_capture(capture).await?;
    let notifications = state.store.deliveries_for_capture(capture).await?;
    let rule_runs = state
        .store
        .list_rule_runs(
            endpoint,
            &RunFilter {
                capture: Some(capture),
                ..RunFilter::default()
            },
            RULE_RUNS_IN_DETAIL,
        )
        .await?;
    Ok(CaptureDto::new(
        detail,
        &forwards,
        &notifications,
        &rule_runs,
    ))
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
