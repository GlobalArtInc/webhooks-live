use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::http::header::{ALLOW, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;
use whl_auth::{Access, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_export::{ExportFormat, file_name};
use whl_store::composer::{QuotaCharge, QuotaWindow};
use whl_store::quota::{EXPORTS_METRIC, user_subject};
use whl_store::{EndpointError, ExportSelection};

use super::doc::{ExportFormatDoc, ExportRequestDoc};
use super::params::{ExportParams, parse_selection};
use super::stream::{self, Job};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, UnsupportedMediaType,
};
use crate::state::ApiState;

const BUSY_RETRY_SECS: u64 = 1;
const ALLOWED_METHODS: &str = "GET, POST";

#[utoipa::path(
    head,
    path = "/endpoints/{id}/export",
    operation_id = "export_head",
    tag = "Export",
    summary = "Refused: export answers GET and POST only",
    description = "Answers 405 `method_not_allowed` with `Allow: GET, POST` and no body. The access rules of GET apply first, so an unknown endpoint answers 410 as it does for GET.",
    params(EndpointIdParam),
    security(("session_cookie" = []), ()),
    responses(
        (status = 401, description = "`unauthenticated`: `sso` without a session"),
        (status = 404, description = "`not_found`: malformed endpoint id"),
        (status = 405, description = "`method_not_allowed`", headers(("Allow" = String, description = "`GET, POST`"))),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`")
    )
)]
pub async fn head() -> Response {
    let mut response = ApiError::MethodNotAllowed.into_response();
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(ALLOWED_METHODS));
    response
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/export",
    operation_id = "export_get",
    tag = "Export",
    summary = "Export the captures of an endpoint that match filters",
    description = "Streams one file with the captures that match the filters, oldest first. Needs the viewer role. Bearer tokens are refused (`session_required`); a view-protected anonymous endpoint needs `X-Webhooks-Live-Grant`.\n\nFormats: `json` is one document with `format`, `version`, `exported_at`, `endpoint` and `captures`; `ndjson` is the same header on the first line, then one capture per line; `csv` is a header row, then one row per capture (no endpoint); `zip` holds `captures.ndjson` (every capture, without bodies) and `bodies/<seq>-<capture id>.bin` for every capture with a stored body. A capture whose stored body is missing is left out of json, ndjson and csv.\n\nFilters are the capture list filters: `method`, `status`, `kind`, `starred`, `q`, `after_seq`, `since` and `until`; `order`, `limit` and `cursor` are ignored. `format` is required.\n\nThe selection is measured before the first byte: above the tier's export size the answer is 422 `export_too_large`. Once streaming has started, a failure ends the body early: a client that reads nothing for 30 seconds, output above twice the tier size, or a run longer than 10 minutes. The client must treat a body that stops before its end as failed. Quota: `exports` per hour (`quota_exceeded`, `error.limit.name` = `exports`; by default 10 per endpoint for anonymous callers and 30 per user for signed-in callers). One export at a time per caller, and 2 per replica by default (`EXPORT_CONCURRENCY`): an export over either limit answers 429 `rate_limited` with `Retry-After: 1`.",
    params(
        EndpointIdParam,
        ("format" = ExportFormatDoc, Query, description = "Output format"),
        ("method" = Option<String>, Query, description = "Comma list of methods, e.g. `POST,PUT`"),
        ("status" = Option<String>, Query, description = "Comma list of override classes `1xx` to `5xx`, or `none`"),
        ("kind" = Option<String>, Query, description = "Comma list of `http`, `email`, `dns` and `schedule`"),
        ("starred" = Option<bool>, Query, description = "Only starred (`true`) or unstarred (`false`) captures"),
        ("q" = Option<String>, Query, min_length = 1, max_length = 200, description = "Case-insensitive substring of the path, query, headers and the first 256 KiB of the body"),
        ("after_seq" = Option<i64>, Query, minimum = 0, description = "Only captures with a `seq` greater than this"),
        ("since" = Option<String>, Query, format = DateTime, description = "`received_at` at or after this time"),
        ("until" = Option<String>, Query, format = DateTime, description = "`received_at` before this time")
    ),
    security(("session_cookie" = []), ()),
    responses(
        (
            status = 200,
            description = "The file, streamed with `Content-Disposition: attachment; filename=\"webhooks-live-<endpoint id>-<yyyymmddHHMMSS>.<ext>\"`",
            content(
                (String = "application/json"),
                (String = "application/x-ndjson"),
                (String = "text/csv; charset=utf-8"),
                (String = "application/zip")
            )
        ),
        (status = 400, response = BadRequest),
        (status = 401, description = "`unauthenticated`: `sso` without a session; `grant_required`: a view-protected endpoint without a grant", body = ErrorBody),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: no viewer role", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 422, description = "`validation_failed`: `format` missing or not one of json, ndjson, csv, zip, or a filter value invalid; `export_too_large`: the selection is above the tier's export size", body = ErrorBody),
        (status = 429, description = "`rate_limited`: another export of this caller is running, `Retry-After: 1`; `quota_exceeded`: the `exports` quota of the hour is used up, `Retry-After` set", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn get(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    Query(params): Query<ExportParams>,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let (format, selection) = params.parse()?;
    start(&state, access.caller(), endpoint, format, selection).await
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/export",
    operation_id = "export_select",
    tag = "Export",
    summary = "Export selected captures of an endpoint",
    description = "The same file as `GET /endpoints/{id}/export`, for the captures named in `capture_ids` (at least 1, at most 500; duplicates are dropped; ids that are not captures of this endpoint are left out). Needs the viewer role; Bearer tokens are refused (`session_required`). Sent as `application/json`; an empty body is `invalid_json`. The quota, concurrency and size rules are those of GET.",
    params(EndpointIdParam),
    security(("session_cookie" = []), ()),
    request_body(content = ExportRequestDoc, content_type = "application/json"),
    responses(
        (
            status = 200,
            description = "The file, streamed with `Content-Disposition: attachment`",
            content(
                (String = "application/json"),
                (String = "application/x-ndjson"),
                (String = "text/csv; charset=utf-8"),
                (String = "application/zip")
            )
        ),
        (status = 400, response = BadRequest),
        (status = 401, description = "`unauthenticated`: `sso` without a session; `grant_required`: a view-protected endpoint without a grant", body = ErrorBody),
        (status = 403, description = "`session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed; `forbidden`: no viewer role", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed`: `format` missing or invalid; `capture_ids` missing or empty (`required`), above 500 (`out_of_range`) or not capture ids (`invalid_format`); `export_too_large`: the selection is above the tier's export size", body = ErrorBody),
        (status = 429, description = "`rate_limited`: another export of this caller is running, `Retry-After: 1`; `quota_exceeded`: the `exports` quota of the hour is used up, `Retry-After` set", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn post(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let (format, selection) = parse_selection(&body)?;
    start(&state, access.caller(), endpoint, format, selection).await
}

async fn start(
    state: &ApiState,
    caller: &Caller,
    endpoint: EndpointId,
    format: ExportFormat,
    selection: ExportSelection,
) -> Result<Response, ApiError> {
    let permit = state.exports.acquire(caller).ok_or(ApiError::RateLimited {
        retry_after_secs: BUSY_RETRY_SECS,
    })?;
    let record = state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let tier = state.tier_of(caller);
    let max_bytes = tier.limits().export_max_bytes;
    let subject = match caller.user() {
        Some(user) => user_subject(user.id),
        None => format!("e:{}", endpoint.unpack()),
    };
    crate::quota::charge(
        state,
        tier,
        EXPORTS_METRIC,
        &[QuotaCharge {
            subject,
            window: QuotaWindow::Hour,
            limit: tier.limits().exports_per_hour,
            name: EXPORTS_METRIC,
        }],
    )
    .await?;
    let snapshot = state.store.export_snapshot(&endpoint, &selection).await?;
    if snapshot.total_bytes > i64::try_from(max_bytes).unwrap_or(i64::MAX) {
        return Err(ApiError::ExportTooLarge);
    }
    let exported_at = OffsetDateTime::now_utc();
    let name = file_name(endpoint.unpack(), exported_at, format);
    let disposition =
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")).map_err(|error| {
            ApiError::Internal(EndpointError::Invalid {
                id: endpoint.unpack().to_owned(),
                reason: format!("the export file name cannot be sent: {error}"),
            })
        })?;
    let body: Body = stream::body(Job {
        store: state.store.clone(),
        endpoint: record,
        selection,
        snapshot,
        format,
        exported_at,
        max_bytes,
        _permit: permit,
    });
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(CONTENT_DISPOSITION, disposition);
    Ok(response)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
