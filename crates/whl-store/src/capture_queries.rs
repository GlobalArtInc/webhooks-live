use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{Postgres, QueryBuilder, Row, Transaction};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::kind::CaptureKind;
use crate::stream::{
    CaptureSummary, PATH_LIMIT_CHARS, StreamError, summary_columns, summary_from_row,
};

pub const DETAIL_BODY_PREFIX_BYTES: usize = 1024 * 1024;

const CHANNEL: &str = "whl_events";

fn detail_columns() -> String {
    format!(
        "c.url, c.host, c.headers::text AS headers, c.query::text AS query, \
         c.form::text AS form, c.meta::text AS meta, \
         substring(b.data FROM 1 FOR {DETAIL_BODY_PREFIX_BYTES}) AS body"
    )
}

const LIVE_ENDPOINT: &str = "SELECT id FROM endpoints \
    WHERE id = $1 AND deleted_at IS NULL AND last_active_at > now() - make_interval(secs => $2) \
    FOR UPDATE";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormFile {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureForm {
    pub fields: Vec<NameValue>,
    pub files: Vec<FormFile>,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("capture query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored capture is invalid: {reason}")]
    Invalid { reason: String },
}

impl From<StreamError> for CaptureError {
    fn from(error: StreamError) -> Self {
        match error {
            StreamError::Query(source) => Self::Query(source),
            StreamError::UnknownForwardState { state } => Self::Invalid {
                reason: format!("unknown forward state {state:?}"),
            },
            StreamError::UnknownCaptureKind { kind } => Self::Invalid {
                reason: format!("unknown capture kind {kind:?}"),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Class(u8),
    Unoverridden,
}

impl StatusFilter {
    fn code(self) -> i32 {
        match self {
            Self::Class(class) => i32::from(class),
            Self::Unoverridden => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

impl Order {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaptureFilter {
    pub methods: Vec<String>,
    pub statuses: Vec<StatusFilter>,
    pub kinds: Vec<CaptureKind>,
    pub search: Option<String>,
    pub search_chars: Option<i32>,
    pub after_seq: Option<i64>,
    pub since: Option<OffsetDateTime>,
    pub until: Option<OffsetDateTime>,
}

impl CaptureFilter {
    pub fn fingerprint(&self) -> String {
        let mut fingerprint = serde_json::json!({
            "methods": self.methods,
            "statuses": self.statuses.iter().map(|status| status.code()).collect::<Vec<_>>(),
            "search": self.search,
            "after_seq": self.after_seq,
            "since": self.since.map(|since| since.unix_timestamp_nanos()),
            "until": self.until.map(|until| until.unix_timestamp_nanos()),
        });
        if let Some(chars) = self.search_chars {
            fingerprint["search_chars"] = serde_json::Value::from(chars);
        }
        if !self.kinds.is_empty() {
            fingerprint["kinds"] = serde_json::Value::from(
                self.kinds
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>(),
            );
        }
        fingerprint.to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureQuery {
    pub filter: CaptureFilter,
    pub order: Order,
    pub limit: i64,
    pub position: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureSlice {
    pub items: Vec<CaptureSummary>,
    pub next_position: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapturePage {
    pub items: Vec<CaptureSummary>,
    pub next_position: Option<i64>,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureBody {
    pub prefix: Vec<u8>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureDetail {
    pub summary: CaptureSummary,
    pub url: String,
    pub host: String,
    pub headers: Vec<NameValue>,
    pub query: Vec<NameValue>,
    pub form: Option<CaptureForm>,
    pub meta: serde_json::Value,
    pub body: CaptureBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    CaptureNotFound,
    EndpointGone,
}

impl Store {
    pub async fn list_captures(
        &self,
        endpoint: &EndpointId,
        query: &CaptureQuery,
    ) -> Result<CapturePage, CaptureError> {
        let slice = self.captures_slice(endpoint, query).await?;
        let mut count = QueryBuilder::<Postgres>::new("SELECT count(*) FROM captures c");
        push_filters(&mut count, endpoint, &query.filter);
        let total: i64 = count
            .build_query_scalar()
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        Ok(CapturePage {
            items: slice.items,
            next_position: slice.next_position,
            total,
        })
    }

    pub async fn captures_slice(
        &self,
        endpoint: &EndpointId,
        query: &CaptureQuery,
    ) -> Result<CaptureSlice, CaptureError> {
        let mut page = QueryBuilder::<Postgres>::new(format!(
            "SELECT {} FROM captures c",
            summary_columns(Some(PATH_LIMIT_CHARS))
        ));
        push_filters(&mut page, endpoint, &query.filter);
        if let Some(after_seq) = query.filter.after_seq {
            page.push(" AND c.seq > ").push_bind(after_seq);
        }
        if let Some(position) = query.position {
            let comparison = match query.order {
                Order::Desc => " AND c.seq < ",
                Order::Asc => " AND c.seq > ",
            };
            page.push(comparison).push_bind(position);
        }
        page.push(" ORDER BY c.seq ")
            .push(query.order.as_str())
            .push(" LIMIT ")
            .push_bind(query.limit.saturating_add(1));
        let rows = page
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let mut items = rows
            .iter()
            .map(summary_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let next_position = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|item| item.seq)
        } else {
            None
        };
        Ok(CaptureSlice {
            items,
            next_position,
        })
    }

    pub async fn get_capture(
        &self,
        endpoint: &EndpointId,
        capture: &CaptureId,
    ) -> Result<Option<CaptureDetail>, CaptureError> {
        let sql = format!(
            "SELECT {}, {} FROM captures c \
             JOIN capture_bodies b ON b.capture_id = c.id \
             WHERE c.endpoint_id = $1 AND c.id = $2",
            summary_columns(None),
            detail_columns()
        );
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(capture.unpack())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(detail_from_row).transpose()
    }

    pub async fn latest_capture(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Option<CaptureDetail>, CaptureError> {
        let sql = format!(
            "SELECT {}, {} FROM captures c \
             JOIN capture_bodies b ON b.capture_id = c.id \
             WHERE c.endpoint_id = $1 ORDER BY c.seq DESC LIMIT 1",
            summary_columns(None),
            detail_columns()
        );
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(detail_from_row).transpose()
    }

    pub async fn delete_capture(
        &self,
        endpoint: &EndpointId,
        capture: &CaptureId,
        retention: Duration,
    ) -> Result<DeleteOutcome, CaptureError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteOutcome::EndpointGone);
        }
        let deleted = sqlx::query(
            "DELETE FROM captures WHERE endpoint_id = $1 AND id = $2 RETURNING seq, stored_bytes",
        )
        .bind(endpoint.unpack())
        .bind(capture.unpack())
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        let Some(row) = deleted else {
            return Ok(DeleteOutcome::CaptureNotFound);
        };
        let seq: i64 = row.try_get("seq").map_err(CaptureError::Query)?;
        let stored_bytes: i32 = row.try_get("stored_bytes").map_err(CaptureError::Query)?;
        sqlx::query(
            "UPDATE endpoints SET capture_count = capture_count - 1, \
             bytes_used = bytes_used - $2 WHERE id = $1",
        )
        .bind(endpoint.unpack())
        .bind(i64::from(stored_bytes))
        .execute(&mut *tx)
        .await
        .map_err(classify)?;
        let payload = serde_json::json!({
            "t": "capture_deleted",
            "e": endpoint.unpack(),
            "d": { "capture_id": capture.to_string(), "seq": seq },
        });
        notify(&mut tx, &payload).await?;
        tx.commit().await.map_err(classify)?;
        Ok(DeleteOutcome::Deleted)
    }

    pub async fn clear_captures(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<bool, CaptureError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(false);
        }
        sqlx::query("DELETE FROM captures WHERE endpoint_id = $1")
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query("UPDATE endpoints SET capture_count = 0, bytes_used = 0 WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let payload = serde_json::json!({
            "t": "captures_cleared",
            "e": endpoint.unpack(),
            "d": {},
        });
        notify(&mut tx, &payload).await?;
        tx.commit().await.map_err(classify)?;
        Ok(true)
    }
}

fn push_filters(
    query: &mut QueryBuilder<'_, Postgres>,
    endpoint: &EndpointId,
    filter: &CaptureFilter,
) {
    if filter.search.is_some() {
        query.push(" JOIN capture_bodies b ON b.capture_id = c.id");
    }
    query
        .push(" WHERE c.endpoint_id = ")
        .push_bind(endpoint.unpack().to_owned());
    if !filter.methods.is_empty() {
        query
            .push(" AND c.method = ANY(")
            .push_bind(filter.methods.clone())
            .push(")");
    }
    if !filter.statuses.is_empty() {
        let codes: Vec<i32> = filter.statuses.iter().map(|status| status.code()).collect();
        query
            .push(" AND COALESCE(c.status_override / 100, 0) = ANY(")
            .push_bind(codes)
            .push(")");
    }
    if !filter.kinds.is_empty() {
        let kinds: Vec<String> = filter
            .kinds
            .iter()
            .map(|kind| kind.as_str().to_owned())
            .collect();
        query.push(" AND c.kind = ANY(").push_bind(kinds).push(")");
    }
    if let Some(term) = &filter.search {
        match filter.search_chars {
            Some(chars) => {
                query
                    .push(" AND strpos(substr(b.search_text, 1, ")
                    .push_bind(chars)
                    .push("), lower(");
            }
            None => {
                query.push(" AND strpos(b.search_text, lower(");
            }
        }
        query.push_bind(term.clone()).push(")) > 0");
    }
    if let Some(since) = filter.since {
        query.push(" AND c.received_at >= ").push_bind(since);
    }
    if let Some(until) = filter.until {
        query.push(" AND c.received_at < ").push_bind(until);
    }
}

fn detail_from_row(row: &PgRow) -> Result<CaptureDetail, CaptureError> {
    let summary = summary_from_row(row)?;
    let headers: String = row.try_get("headers").map_err(CaptureError::Query)?;
    let query: String = row.try_get("query").map_err(CaptureError::Query)?;
    let form: Option<String> = row.try_get("form").map_err(CaptureError::Query)?;
    let meta: String = row.try_get("meta").map_err(CaptureError::Query)?;
    let prefix: Vec<u8> = row.try_get("body").map_err(CaptureError::Query)?;
    let truncated = usize::try_from(summary.body_size).is_ok_and(|size| size > prefix.len());
    let context = summary.id.clone();
    Ok(CaptureDetail {
        url: row.try_get("url").map_err(CaptureError::Query)?,
        host: row.try_get("host").map_err(CaptureError::Query)?,
        headers: decode(&context, "headers", &headers)?,
        query: decode(&context, "query", &query)?,
        form: form
            .map(|text| decode(&context, "form", &text))
            .transpose()?,
        meta: decode(&context, "meta", &meta)?,
        body: CaptureBody { prefix, truncated },
        summary,
    })
}

fn decode<T: DeserializeOwned>(id: &str, column: &str, text: &str) -> Result<T, CaptureError> {
    serde_json::from_str(text).map_err(|error| CaptureError::Invalid {
        reason: format!("capture {id} column {column}: {error}"),
    })
}

async fn lock_live_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    retention: Duration,
) -> Result<bool, CaptureError> {
    let row = sqlx::query(LIVE_ENDPOINT)
        .bind(endpoint.unpack())
        .bind(retention.as_secs_f64())
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify)?;
    Ok(row.is_some())
}

async fn notify(
    tx: &mut Transaction<'_, Postgres>,
    payload: &serde_json::Value,
) -> Result<(), CaptureError> {
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(CHANNEL)
        .bind(payload.to_string())
        .execute(&mut **tx)
        .await
        .map_err(classify)?;
    Ok(())
}

fn classify(error: sqlx::Error) -> CaptureError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        CaptureError::Unavailable(error)
    } else {
        CaptureError::Query(error)
    }
}
