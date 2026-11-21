use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointStats, StatsBucket, StatsPoint};

use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

#[derive(Debug, Default, Deserialize)]
pub struct StatsParams {
    bucket: Option<String>,
}

#[derive(Serialize)]
pub struct StatsDto {
    bucket: &'static str,
    from: Option<String>,
    to: Option<String>,
    series: Vec<PointDto>,
}

#[derive(Serialize)]
pub struct PointDto {
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
