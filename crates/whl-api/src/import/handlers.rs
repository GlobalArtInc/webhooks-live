use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use whl_auth::Caller;
use whl_core::endpoint_id::EndpointId;
use whl_export::ImportFormat;
use whl_store::EndpointError;
use whl_store::composer::{QuotaCharge, QuotaWindow};
use whl_store::quota::{IMPORTS_METRIC, user_subject};

use super::run::{self, Placement, Target};
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::folders::signed_in;
use crate::state::ApiState;

const BUSY_RETRY_SECS: u64 = 1;

#[derive(Debug, Deserialize)]
pub struct ImportParams {
    keep_times: Option<String>,
}

#[derive(Serialize)]
struct ImportDto {
    imported: usize,
    skipped: usize,
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint_id: Option<String>,
}

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
