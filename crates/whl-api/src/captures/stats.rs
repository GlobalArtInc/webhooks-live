use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointStats, StatsBucket, StatsPoint};

use crate::dto::timestamp;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{StoreUnavailable, Unauthenticated, ValidationFailed};
use crate::state::ApiState;

#[derive(Debug, Default, Deserialize)]
pub struct StatsParams {
    bucket: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = Stats)]
pub struct StatsDto {
    #[schema(pattern = "^(hour|day)$")]
    bucket: &'static str,
    #[schema(format = DateTime)]
    from: Option<String>,
    #[schema(format = DateTime)]
    to: Option<String>,
    series: Vec<PointDto>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = StatsPoint)]
pub struct PointDto {
    #[schema(format = DateTime)]
    at: String,
    total: i64,
    by_method: BTreeMap<String, i64>,
    by_kind: BTreeMap<String, i64>,
    by_status: BTreeMap<String, i64>,
}

impl From<&StatsPoint> for PointDto {
    fn from(point: &StatsPoint) -> Self {
        Self {
            at: timestamp(point.at),
            total: point.total,
            by_method: point.by_method.clone(),
            by_kind: point.by_kind.clone(),
            by_status: point.by_status.clone(),
        }
    }
}

impl From<&EndpointStats> for StatsDto {
    fn from(stats: &EndpointStats) -> Self {
        Self {
            bucket: stats.bucket.as_str(),
            from: stats.from.map(timestamp),
            to: stats.to.map(timestamp),
            series: stats.series.iter().map(PointDto::from).collect(),
        }
    }
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/stats",
    operation_id = "captures_stats",
    tag = "Captures",
    summary = "Capture counts of an endpoint over time",
    description = "A series of buckets, oldest first. Each point has `at`, `total`, and counts keyed by `by_method` (method), `by_kind` (`http`, `email`, `dns`, `schedule`) and `by_status` (`1xx` to `5xx`, or `other`). `from` and `to` are the times of the first and last point, null when the series is empty. `bucket` `hour` or `day`; absent means `day`; any other value answers 422 `validation_failed`. Session only: a Bearer token is refused with 403 `session_required`. Needs a viewer on the endpoint.",
    security(("session_cookie" = [])),
    params(
        EndpointIdParam,
        ("bucket" = Option<String>, Query, description = "`hour` or `day`; absent means `day`"),
    ),
    responses(
        (status = 200, description = "The statistics", body = StatsDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn show(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    Query(params): Query<StatsParams>,
) -> Result<Json<StatsDto>, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let bucket = parse_bucket(params.bucket.as_deref())?;
    state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let stats = state.store.endpoint_stats(&endpoint, bucket).await?;
    Ok(Json(StatsDto::from(&stats)))
}

fn parse_bucket(text: Option<&str>) -> Result<StatsBucket, ApiError> {
    match text {
        None | Some("day") => Ok(StatsBucket::Day),
        Some("hour") => Ok(StatsBucket::Hour),
        Some(_) => Err(ApiError::Validation(vec![FieldViolation::new(
            "bucket",
            FieldCode::InvalidFormat,
            "must be hour or day",
        )])),
    }
}
