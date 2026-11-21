use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::types::Uuid;
use sqlx::{Postgres, Row};
use time::OffsetDateTime;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::IdGenerationError;
use whl_core::header::HeaderInput;
use whl_core::response::{ResponseConfig, ResponseConfigInput};
use whl_core::tag::Tag;

use crate::captures::EVENT_CHANNEL;
use crate::store::Store;

pub(crate) const COLUMNS: &str = "id, title, response_status, response_content_type, \
    response_headers::text AS response_headers, response_body, response_delay_ms, cors, overflow, \
    capture_count, bytes_used, last_seq, created_at, last_active_at, tags";

const GET_SQL: &str = "WITH live AS ( \
        SELECT id FROM endpoints \
        WHERE id = $1 AND deleted_at IS NULL AND last_active_at > now() - make_interval(secs => $2) \
    ), bumped AS ( \
        UPDATE endpoints SET last_active_at = now() \
        WHERE id IN (SELECT id FROM live) AND last_active_at < now() - interval '60 seconds' \
        RETURNING id, last_active_at \
    ) \
    SELECT e.id, e.title, e.response_status, e.response_content_type, \
        e.response_headers::text AS response_headers, e.response_body, e.response_delay_ms, e.cors, \
        e.overflow, e.capture_count, e.bytes_used, e.last_seq, e.created_at, e.tags, \
        COALESCE(b.last_active_at, e.last_active_at) AS last_active_at \
    FROM endpoints e LEFT JOIN bumped b ON b.id = e.id \
    WHERE e.id IN (SELECT id FROM live)";

const DELETE_SQL: &str = "UPDATE endpoints SET deleted_at = now() \
    WHERE id = $1 AND deleted_at IS NULL AND last_active_at > now() - make_interval(secs => $2)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Refuse,
    Ring,
}

impl Overflow {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Ring => "ring",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "refuse" => Some(Self::Refuse),
            "ring" => Some(Self::Ring),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointSettings {
    pub title: Option<String>,
    pub response: ResponseConfig,
    pub cors: bool,
    pub overflow: Overflow,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointRecord {
    pub id: EndpointId,
    pub settings: EndpointSettings,
    pub capture_count: i32,
    pub bytes_used: i64,
    pub last_seq: i64,
    pub created_at: OffsetDateTime,
    pub last_active_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum EndpointError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("endpoint query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("cannot generate an endpoint id: {0}")]
    Id(#[from] IdGenerationError),
    #[error("cannot encode the response headers: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("stored endpoint {id} is invalid: {reason}")]
    Invalid { id: String, reason: String },
    #[error("retention {0:?} cannot be applied to a timestamp")]
    Retention(Duration),
}

#[derive(Serialize, Deserialize)]
struct StoredHeader {
    name: String,
    value: String,
}

impl Store {
    pub async fn create_endpoint(
        &self,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        self.create_endpoint_owned(None, settings, retention).await
    }

    pub async fn create_endpoint_owned(
        &self,
        owner: Option<Uuid>,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        self.insert_endpoint(owner, None, settings, retention).await
    }

    pub async fn create_endpoint_from_network(
        &self,
        creator_net: Option<&CreatorNet>,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        self.insert_endpoint(None, creator_net, settings, retention)
            .await
    }

    async fn insert_endpoint(
        &self,
        owner: Option<Uuid>,
        creator_net: Option<&CreatorNet>,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        let id = EndpointId::generate()?;
        let sql = format!(
            "INSERT INTO endpoints (id, title, response_status, response_content_type, \
             response_headers, response_body, response_delay_ms, cors, overflow, tags, owner_id, \
             creator_net) \
             VALUES ($1, $2, $3, $4, $5::text::jsonb, $6, $7, $8, $9, $10, $11, $12::text::cidr) \
             RETURNING {COLUMNS}"
        );
        let row = bind_settings(sqlx::query(&sql).bind(id.unpack()), settings)?
            .bind(owner)
            .bind(creator_net.map(ToString::to_string))
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        record_from_row(&row, retention)
    }

    pub async fn get_endpoint(
        &self,
        id: &EndpointId,
        retention: Duration,
    ) -> Result<Option<EndpointRecord>, EndpointError> {
        let row = sqlx::query(GET_SQL)
            .bind(id.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref()
            .map(|row| record_from_row(row, retention))
            .transpose()
    }

    pub async fn update_endpoint<E, F>(
        &self,
        id: &EndpointId,
        retention: Duration,
        merge: F,
    ) -> Result<Option<EndpointRecord>, E>
    where
        F: FnOnce(&EndpointRecord) -> Result<EndpointSettings, E>,
        E: From<EndpointError>,
    {
        let mut transaction = self
            .pool()
            .begin()
            .await
            .map_err(|error| E::from(classify(error)))?;
        let lock_sql = format!(
            "SELECT {COLUMNS} FROM endpoints \
             WHERE id = $1 AND deleted_at IS NULL AND last_active_at > now() - make_interval(secs => $2) \
             FOR UPDATE"
        );
        let Some(row) = sqlx::query(&lock_sql)
            .bind(id.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| E::from(classify(error)))?
        else {
            return Ok(None);
        };
        let current = record_from_row(&row, retention).map_err(E::from)?;
        let settings = merge(&current)?;
        let update_sql = format!(
            "UPDATE endpoints SET title = $2, response_status = $3, response_content_type = $4, \
             response_headers = $5::text::jsonb, response_body = $6, response_delay_ms = $7, \
             cors = $8, overflow = $9, tags = $10, last_active_at = now() \
             WHERE id = $1 RETURNING {COLUMNS}"
        );
        let row = bind_settings(sqlx::query(&update_sql).bind(id.unpack()), &settings)
            .map_err(E::from)?
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| E::from(classify(error)))?;
        let record = record_from_row(&row, retention).map_err(E::from)?;
        let payload = serde_json::json!({ "t": "endpoint_updated", "e": id.unpack() });
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(EVENT_CHANNEL)
            .bind(payload.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(|error| E::from(classify(error)))?;
        transaction
            .commit()
            .await
            .map_err(|error| E::from(classify(error)))?;
        Ok(Some(record))
    }

    pub async fn delete_endpoint(
        &self,
        id: &EndpointId,
        retention: Duration,
    ) -> Result<bool, EndpointError> {
        let result = sqlx::query(DELETE_SQL)
            .bind(id.unpack())
            .bind(retention.as_secs_f64())
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(result.rows_affected() == 1)
    }
}

fn bind_settings<'q>(
    query: Query<'q, Postgres, PgArguments>,
    settings: &'q EndpointSettings,
) -> Result<Query<'q, Postgres, PgArguments>, EndpointError> {
    let headers: Vec<StoredHeader> = settings
        .response
        .headers()
        .iter()
        .map(|header| StoredHeader {
            name: header.name.unpack().to_owned(),
            value: header.value.unpack().to_owned(),
        })
        .collect();
    let headers = serde_json::to_string(&headers).map_err(EndpointError::Encode)?;
    let tags: Vec<String> = settings
        .tags
        .iter()
        .map(|tag| tag.unpack().to_owned())
        .collect();
    let status = i16::try_from(settings.response.status().unpack()).unwrap_or(i16::MAX);
    let delay = i32::try_from(settings.response.delay().as_millis()).unwrap_or(i32::MAX);
    Ok(query
        .bind(settings.title.as_deref())
        .bind(status)
        .bind(settings.response.content_type().unpack())
        .bind(headers)
        .bind(settings.response.body())
        .bind(delay)
        .bind(settings.cors)
        .bind(settings.overflow.as_str())
        .bind(tags))
}

pub(crate) fn record_from_row(
    row: &PgRow,
    retention: Duration,
) -> Result<EndpointRecord, EndpointError> {
    let id_text: String = row.try_get("id").map_err(EndpointError::Query)?;
    let invalid = |reason: String| EndpointError::Invalid {
        id: id_text.clone(),
        reason,
    };
    let id = EndpointId::parse(&id_text).map_err(|error| invalid(error.to_string()))?;

    let title: Option<String> = row.try_get("title").map_err(EndpointError::Query)?;
    let status: i16 = row
        .try_get("response_status")
        .map_err(EndpointError::Query)?;
    let content_type: String = row
        .try_get("response_content_type")
        .map_err(EndpointError::Query)?;
    let headers_text: String = row
        .try_get("response_headers")
        .map_err(EndpointError::Query)?;
    let body: Vec<u8> = row.try_get("response_body").map_err(EndpointError::Query)?;
    let delay: i32 = row
        .try_get("response_delay_ms")
        .map_err(EndpointError::Query)?;
    let cors: bool = row.try_get("cors").map_err(EndpointError::Query)?;
    let overflow_text: String = row.try_get("overflow").map_err(EndpointError::Query)?;
    let tags_text: Vec<String> = row.try_get("tags").map_err(EndpointError::Query)?;
    let capture_count: i32 = row.try_get("capture_count").map_err(EndpointError::Query)?;
    let bytes_used: i64 = row.try_get("bytes_used").map_err(EndpointError::Query)?;
    let last_seq: i64 = row.try_get("last_seq").map_err(EndpointError::Query)?;
    let created_at: OffsetDateTime = row.try_get("created_at").map_err(EndpointError::Query)?;
    let last_active_at: OffsetDateTime = row
        .try_get("last_active_at")
        .map_err(EndpointError::Query)?;

    let stored: Vec<StoredHeader> =
        serde_json::from_str(&headers_text).map_err(|error| invalid(error.to_string()))?;
    let response = ResponseConfig::validate(ResponseConfigInput {
        status: i64::from(status),
        content_type,
        headers: stored
            .into_iter()
            .map(|header| HeaderInput {
                name: header.name,
                value: header.value,
            })
            .collect(),
        body,
        delay_ms: i64::from(delay),
    })
    .map_err(|error| invalid(error.to_string()))?;
    let overflow = Overflow::parse(&overflow_text)
        .ok_or_else(|| invalid(format!("unknown overflow mode {overflow_text}")))?;
    let tags = tags_text
        .iter()
        .map(|text| Tag::parse(text).map_err(|error| invalid(error.to_string())))
        .collect::<Result<Vec<Tag>, _>>()?;
    let expires_at = expiry(last_active_at, retention)?;

    Ok(EndpointRecord {
        id,
        settings: EndpointSettings {
            title,
            response,
            cors,
            overflow,
            tags,
        },
        capture_count,
        bytes_used,
        last_seq,
        created_at,
        last_active_at,
        expires_at,
    })
}

fn expiry(
    last_active_at: OffsetDateTime,
    retention: Duration,
) -> Result<OffsetDateTime, EndpointError> {
    let span =
        time::Duration::try_from(retention).map_err(|_| EndpointError::Retention(retention))?;
    last_active_at
        .checked_add(span)
        .ok_or(EndpointError::Retention(retention))
}

pub(crate) fn classify(error: sqlx::Error) -> EndpointError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        EndpointError::Unavailable(error)
    } else {
        EndpointError::Query(error)
    }
}
