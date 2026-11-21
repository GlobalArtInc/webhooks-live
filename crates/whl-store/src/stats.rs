use std::collections::BTreeMap;

use sqlx::Row;
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;

use crate::capture_queries::{CaptureError, classify};
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsBucket {
    Hour,
    Day,
}

impl StatsBucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsPoint {
    pub at: OffsetDateTime,
    pub total: i64,
    pub by_method: BTreeMap<String, i64>,
    pub by_kind: BTreeMap<String, i64>,
    pub by_status: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointStats {
    pub bucket: StatsBucket,
    pub from: Option<OffsetDateTime>,
    pub to: Option<OffsetDateTime>,
    pub series: Vec<StatsPoint>,
}

const STATS_SQL: &str = "SELECT date_trunc($2::text, received_at, 'UTC') AS at, method, kind, \
     (response_status / 100)::int AS class, count(*) AS count \
     FROM captures WHERE endpoint_id = $1 \
     GROUP BY 1, 2, 3, 4 ORDER BY 1";

impl Store {
    pub async fn endpoint_stats(
        &self,
        endpoint: &EndpointId,
        bucket: StatsBucket,
    ) -> Result<EndpointStats, CaptureError> {
        let rows = sqlx::query(STATS_SQL)
            .bind(endpoint.unpack())
            .bind(bucket.as_str())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let mut series: Vec<StatsPoint> = Vec::new();
        for row in &rows {
            let at: OffsetDateTime = row.try_get("at").map_err(CaptureError::Query)?;
            let method: String = row.try_get("method").map_err(CaptureError::Query)?;
            let kind: String = row.try_get("kind").map_err(CaptureError::Query)?;
            let class: i32 = row.try_get("class").map_err(CaptureError::Query)?;
            let count: i64 = row.try_get("count").map_err(CaptureError::Query)?;
            if series.last().is_none_or(|point| point.at != at) {
                series.push(StatsPoint {
                    at,
                    total: 0,
                    by_method: BTreeMap::new(),
                    by_kind: BTreeMap::new(),
                    by_status: BTreeMap::new(),
                });
            }
            if let Some(point) = series.last_mut() {
                point.total += count;
                *point.by_method.entry(method).or_default() += count;
                *point.by_kind.entry(kind).or_default() += count;
                *point.by_status.entry(status_class(class)).or_default() += count;
            }
        }
        Ok(EndpointStats {
            bucket,
            from: series.first().map(|point| point.at),
            to: series.last().map(|point| point.at),
            series,
        })
    }
}

fn status_class(class: i32) -> String {
    if (1..=5).contains(&class) {
        format!("{class}xx")
    } else {
        "other".to_owned()
    }
}
