use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use whl_auth::Caller;
use whl_core::endpoint_id::EndpointId;
use whl_export::ImportFormat;
use whl_store::EndpointError;
use whl_store::composer::{QuotaCharge, QuotaWindow};
use whl_store::quota::{IMPORTS_METRIC, user_subject};

use super::run::{self, Placement, Target};
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::folders::signed_in;
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    PayloadTooLarge, StoreUnavailable, Unauthenticated, ValidationFailed,
};
use crate::state::ApiState;

const BUSY_RETRY_SECS: u64 = 1;

#[derive(Debug, Deserialize)]
pub struct ImportParams {
    keep_times: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = ImportResult)]
pub(super) struct ImportDto {
    #[schema(example = 120)]
    imported: usize,
    #[schema(example = 0)]
    skipped: usize,
    #[schema(example = json!(null))]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(
        pattern = "^[0-9a-hjkmnp-tv-z]{24}$",
        example = "7k2m9q4xw8c1d5f3h6j0n2p4"
    )]
    endpoint_id: Option<String>,
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/import",
    operation_id = "import_into",
    tag = "Import",
    summary = "Import an export file into an existing endpoint",
    description = "Needs a session; a token is refused (`session_required`). The caller needs the editor role on an owned endpoint; an unowned endpoint in `mixed` answers 409 `claim_required`. The body is the JSON document or the NDJSON file of `GET /endpoints/{id}/export`, sent as `Content-Type: application/json` or `application/x-ndjson` (parameters after `;` are ignored). Other types answer 415 before the body is read. The header must carry `format` = `webhooks-live-export`, `version` = 1, `exported_at` and `endpoint`. Response: `imported`, `skipped` and `reason`, which is `capacity`, `storage` or null when everything was imported.\n\nA declared `Content-Length` above the tier's import size answers 413 before the body is read, and a stream above it stops with 413. The upload must keep sending: 30 seconds without data, or a run longer than 10 minutes, answers 400 `invalid_json`. `keep_times` (default `true`): with `true` each capture keeps its `received_at`, which must lie between 2015-01-01 and five minutes after now; with `false` each capture gets the import time; any other value answers 422. The first problem stops the import with 422 `validation_failed`, `field` = `captures.<index>.<path>` (JSON) or `line.<n>` (NDJSON); the captures stored before it stay. Once the endpoint is full or the storage limit is reached, the rest is counted as `skipped` and `reason` names the cause.\n\nLimits: one import at a time per caller and 2 per replica (429 `rate_limited`, `Retry-After: 1`); a user whose import has finished waits 60 seconds before the next one (429 `rate_limited`, `Retry-After` set); the `imports` quota per day (429 `quota_exceeded`, `error.limit.name` = `imports`; by default 10 per user).",
    params(
        EndpointIdParam,
        ("keep_times" = Option<bool>, Query, description = "Keep the exported `received_at` times (`true`, default) or stamp the import time (`false`)")
    ),
    security(("session_cookie" = [])),
    request_body(content((String = "application/json"), (String = "application/x-ndjson")), description = "The export file, as JSON or NDJSON"),
    responses(
        (status = 200, description = "The import ran; `imported`, `skipped`, `reason`", body = ImportDto),
        (status = 400, description = "`invalid_json`: the body could not be read, the upload stalled, or the import ran longer than 10 minutes", body = ErrorBody),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed; `forbidden`: the role is viewer", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 409, description = "`claim_required`: `mixed`, the endpoint has no owner yet", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, description = "`unsupported_media_type`: the Content-Type is neither `application/json` nor `application/x-ndjson`", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 429, description = "`rate_limited`: another import is running or the 60-second cooldown has not passed (`Retry-After`); `quota_exceeded`: the `imports` quota of the day is used up (`Retry-After`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn into_endpoint(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
    Query(params): Query<ImportParams>,
    request: Request,
) -> Result<Response, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let outcome = start(
        &state,
        &caller,
        &params,
        request,
        Target::Existing(endpoint),
    )
    .await?;
    Ok(Json(dto(&outcome, false)).into_response())
}

#[utoipa::path(
    post,
    path = "/endpoints/import",
    operation_id = "import_create",
    tag = "Import",
    summary = "Create an endpoint from an export file and import its captures",
    description = "Needs a session; a token is refused (`session_required`). The new endpoint takes its settings from the header's `endpoint` object (`title`, `response`, `cors`, `overflow`; other keys answer 422 `not_allowed`) and is created under the rules of `POST /endpoints`, so its errors apply too (for example 422 `limit_reached`). The body and the limits are those of `POST /endpoints/{id}/import`. A failed import deletes the endpoint it created. Answers 201 with `Location: /api/v1/endpoints/{id}` and `endpoint_id`.",
    security(("session_cookie" = [])),
    params(
        ("keep_times" = Option<bool>, Query, description = "Keep the exported `received_at` times (`true`, default) or stamp the import time (`false`)")
    ),
    request_body(content((String = "application/json"), (String = "application/x-ndjson")), description = "The export file, as JSON or NDJSON"),
    responses(
        (status = 201, description = "The endpoint is created and its captures are imported", body = ImportDto, headers(("Location" = String, description = "`/api/v1/endpoints/{id}` of the new endpoint"))),
        (status = 400, description = "`invalid_json`: the body could not be read, the upload stalled, or the import ran longer than 10 minutes", body = ErrorBody),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, description = "`unsupported_media_type`: the Content-Type is neither `application/json` nor `application/x-ndjson`", body = ErrorBody),
        (status = 422, description = "`validation_failed`: the endpoint settings or a capture are invalid (`endpoint.<key>`, `captures.<index>.<path>`, `line.<n>`, or `keep_times`); `limit_reached`: the endpoint count limit", body = ErrorBody),
        (status = 429, description = "`rate_limited`: another import is running or the 60-second cooldown has not passed (`Retry-After`); `quota_exceeded`: the `imports` quota of the day is used up (`Retry-After`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn into_new_endpoint(
    State(state): State<ApiState>,
    caller: Caller,
    Query(params): Query<ImportParams>,
    request: Request,
) -> Result<Response, ApiError> {
    let outcome = start(&state, &caller, &params, request, Target::New).await?;
    let location = format!("/api/v1/endpoints/{}", outcome.endpoint.unpack());
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(dto(&outcome, true)),
    )
        .into_response())
}

fn dto(outcome: &run::Outcome, created: bool) -> ImportDto {
    ImportDto {
        imported: outcome.imported,
        skipped: outcome.skipped,
        reason: outcome.stop.map(|stop| stop.as_str()),
        endpoint_id: created.then(|| outcome.endpoint.unpack().to_owned()),
    }
}

async fn start(
    state: &ApiState,
    caller: &Caller,
    params: &ImportParams,
    request: Request,
    target: Target,
) -> Result<run::Outcome, ApiError> {
    let user = signed_in(state, caller)?;
    let keep_times = parse_keep_times(params.keep_times.as_deref())?;
    let (parts, body) = request.into_parts();
    let format = format_of(&parts.headers)?;
    let tier = state.tier_of(caller);
    if declared_length(&parts.headers).is_some_and(|length| length > tier.limits().import_max_bytes)
    {
        return Err(ApiError::ImportTooLarge);
    }
    state
        .import_cooldown
        .check(user)
        .map_err(|remaining| ApiError::RateLimited {
            retry_after_secs: remaining.as_secs() + 1,
        })?;
    let permit = state.imports.acquire(caller).ok_or(ApiError::RateLimited {
        retry_after_secs: BUSY_RETRY_SECS,
    })?;
    crate::quota::charge(
        state,
        tier,
        IMPORTS_METRIC,
        &[QuotaCharge {
            subject: user_subject(user),
            window: QuotaWindow::Day,
            limit: tier.limits().imports_per_day,
            name: IMPORTS_METRIC,
        }],
    )
    .await?;
    let task_state = state.clone();
    let task_caller = caller.clone();
    let task = tokio::spawn(async move {
        let _permit = permit;
        let outcome = run::execute(
            &task_state,
            &task_caller,
            target,
            Placement { keep_times },
            format,
            body,
        )
        .await;
        task_state.import_cooldown.finish(user);
        outcome
    });
    match task.await {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::error!(%error, "an import task ended abnormally");
            Err(ApiError::Internal(EndpointError::Invalid {
                id: String::new(),
                reason: "the import task ended abnormally".to_owned(),
            }))
        }
    }
}

fn parse_keep_times(text: Option<&str>) -> Result<bool, ApiError> {
    match text {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(ApiError::Validation(vec![FieldViolation::new(
            "keep_times",
            FieldCode::InvalidFormat,
            "must be true or false",
        )])),
    }
}

fn format_of(headers: &HeaderMap) -> Result<ImportFormat, ApiError> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(ImportFormat::from_content_type)
        .ok_or(ApiError::ImportMediaType)
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.parse().ok())
}
