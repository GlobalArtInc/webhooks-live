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
use whl_verify::{Verifier, VerifierDocument};

use crate::captures::EVENT_CHANNEL;
use crate::lifecycle::{
    LOCK_OWNER_SQL, Lifecycle, LifecycleError, LifecycleLimits, admit_endpoint_count,
    admit_never_expires,
};
use crate::store::Store;

pub(crate) const COLUMNS: &str = "id, title, response_status, response_content_type, \
    response_headers::text AS response_headers, response_body, response_delay_ms, cors, overflow, \
    response_templated, response_status_template, \
    capture_count, bytes_used, last_seq, created_at, last_active_at, tags, retention_seconds, \
    never_expires, max_captures, capture_ttl_seconds, \
    verifier::text AS verifier";

const GET_SQL: &str = "WITH live AS ( \
        SELECT id FROM endpoints \
        WHERE id = $1 AND deleted_at IS NULL \
        AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
    ), bumped AS ( \
        UPDATE endpoints SET last_active_at = now() \
        WHERE id IN (SELECT id FROM live) AND last_active_at < now() - interval '60 seconds' \
        RETURNING id, last_active_at \
    ) \
    SELECT e.id, e.title, e.response_status, e.response_content_type, \
        e.response_headers::text AS response_headers, e.response_body, e.response_delay_ms, e.cors, \
        e.overflow, e.response_templated, e.response_status_template, e.capture_count, \
        e.bytes_used, e.last_seq, e.created_at, e.tags, \
        COALESCE(b.last_active_at, e.last_active_at) AS last_active_at, \
        e.retention_seconds, e.never_expires, e.max_captures, e.capture_ttl_seconds, \
        e.verifier::text AS verifier \
    FROM endpoints e LEFT JOIN bumped b ON b.id = e.id \
    WHERE e.id IN (SELECT id FROM live)";

const DELETE_SQL: &str = "UPDATE endpoints SET deleted_at = now() \
    WHERE id = $1 AND deleted_at IS NULL \
    AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredVerifier {
    Configured(Verifier),
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponseTemplating {
    pub enabled: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointSettings {
    pub title: Option<String>,
    pub response: ResponseConfig,
    pub templating: ResponseTemplating,
    pub cors: bool,
    pub overflow: Overflow,
    pub tags: Vec<Tag>,
    pub verifier: Option<StoredVerifier>,
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
    pub expires_at: Option<OffsetDateTime>,
    pub lifecycle: Lifecycle,
}

#[derive(Debug, Clone, Copy)]
pub struct NewEndpoint<'a> {
    pub owner: Option<Uuid>,
    pub creator_net: Option<&'a CreatorNet>,
    pub settings: &'a EndpointSettings,
    pub lifecycle: Lifecycle,
    pub limits: LifecycleLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointChange {
    pub settings: EndpointSettings,
    pub lifecycle: Lifecycle,
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
    #[error("the verifier names a secret this endpoint does not hold")]
    VerifierSecretUnknown,
    #[error("cannot encode the verifier: {0}")]
    EncodeVerifier(#[source] serde_json::Error),
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
        self.insert_default_lifecycle(owner, None, settings, retention)
            .await
    }

    pub async fn create_endpoint_from_network(
        &self,
        creator_net: Option<&CreatorNet>,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        self.insert_default_lifecycle(None, creator_net, settings, retention)
            .await
    }

    pub async fn create_endpoint_with_lifecycle(
        &self,
        new: NewEndpoint<'_>,
        retention: Duration,
    ) -> Result<EndpointRecord, LifecycleError> {
        self.insert_endpoint(&new, retention).await
    }

    async fn insert_default_lifecycle(
        &self,
        owner: Option<Uuid>,
        creator_net: Option<&CreatorNet>,
        settings: &EndpointSettings,
        retention: Duration,
    ) -> Result<EndpointRecord, EndpointError> {
        let new = NewEndpoint {
            owner,
            creator_net,
            settings,
            lifecycle: Lifecycle::default(),
            limits: LifecycleLimits::NONE,
        };
        self.insert_endpoint(&new, retention)
            .await
            .map_err(|error| match error {
                LifecycleError::Endpoint(error) => error,
                other => EndpointError::Invalid {
                    id: String::new(),
                    reason: other.to_string(),
                },
            })
    }

    async fn insert_endpoint(
        &self,
        new: &NewEndpoint<'_>,
        retention: Duration,
    ) -> Result<EndpointRecord, LifecycleError> {
        if new.settings.verifier.is_some() {
            return Err(EndpointError::VerifierSecretUnknown.into());
        }
        let id = EndpointId::generate().map_err(EndpointError::from)?;
        let sql = format!(
            "INSERT INTO endpoints (id, title, response_status, response_content_type, \
             response_headers, response_body, response_delay_ms, cors, overflow, tags, \
             response_templated, response_status_template, owner_id, \
             creator_net, retention_seconds, never_expires, max_captures, capture_ttl_seconds) \
             VALUES ($1, $2, $3, $4, $5::text::jsonb, $6, $7, $8, $9, $10, $11, $12, $13, \
             $14::text::cidr, $15, $16, $17, $18) \
             RETURNING {COLUMNS}"
        );
        let mut transaction = self.pool().begin().await.map_err(classify)?;
        if let Some(max) = new.limits.endpoints_max {
            admit_endpoint_count(&mut transaction, new.owner, new.creator_net, max, retention)
                .await?;
        }
        if new.lifecycle.never_expires {
            admit_never_expires(&mut transaction, new.owner, new.limits).await?;
        }
        let row = bind_settings(sqlx::query(&sql).bind(id.unpack()), new.settings)?
            .bind(new.owner)
            .bind(new.creator_net.map(ToString::to_string))
            .bind(new.lifecycle.retention_seconds)
            .bind(new.lifecycle.never_expires)
            .bind(new.lifecycle.max_captures)
            .bind(new.lifecycle.capture_ttl_seconds)
            .fetch_one(&mut *transaction)
            .await
            .map_err(classify)?;
        let record = record_from_row(&row, retention)?;
        transaction.commit().await.map_err(classify)?;
        Ok(record)
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
        limits: LifecycleLimits,
        merge: F,
    ) -> Result<Option<EndpointRecord>, E>
    where
        F: FnOnce(&EndpointRecord, Option<Uuid>) -> Result<EndpointChange, E>,
        E: From<EndpointError> + From<LifecycleError>,
    {
        let mut transaction = self
            .pool()
            .begin()
            .await
            .map_err(|error| E::from(classify(error)))?;
        let known_owner: Option<Option<Uuid>> =
            sqlx::query_scalar("SELECT owner_id FROM endpoints WHERE id = $1")
                .bind(id.unpack())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| E::from(classify(error)))?;
        if let Some(Some(owner)) = known_owner {
            sqlx::query(LOCK_OWNER_SQL)
                .bind(owner)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| E::from(classify(error)))?;
        }
        let lock_sql = format!(
            "SELECT {COLUMNS}, owner_id FROM endpoints \
             WHERE id = $1 AND deleted_at IS NULL \
             AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
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
        let owner: Option<Uuid> = row
            .try_get("owner_id")
            .map_err(|error| E::from(EndpointError::Query(error)))?;
        let change = merge(&current, owner)?;
        if change.lifecycle.never_expires && !current.lifecycle.never_expires {
            admit_never_expires(&mut transaction, owner, limits)
                .await
                .map_err(E::from)?;
        }
        if let Some(StoredVerifier::Configured(configured)) = &change.settings.verifier
            && change.settings.verifier != current.settings.verifier
        {
            let held: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM endpoint_secrets WHERE endpoint_id = $1 AND name = $2)",
            )
            .bind(id.unpack())
            .bind(configured.secret_name().unpack())
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| E::from(classify(error)))?;
            if !held {
                return Err(E::from(EndpointError::VerifierSecretUnknown));
            }
        }
        let verifier = encode_verifier(&change.settings).map_err(E::from)?;
        let update_sql = format!(
            "UPDATE endpoints SET title = $2, response_status = $3, response_content_type = $4, \
             response_headers = $5::text::jsonb, response_body = $6, response_delay_ms = $7, \
             cors = $8, overflow = $9, tags = $10, response_templated = $11, \
             response_status_template = $12, retention_seconds = $13, never_expires = $14, \
             max_captures = $15, capture_ttl_seconds = $16, verifier = $17::text::jsonb, \
             last_active_at = now() \
             WHERE id = $1 RETURNING {COLUMNS}"
        );
        let row = bind_settings(sqlx::query(&update_sql).bind(id.unpack()), &change.settings)
            .map_err(E::from)?
            .bind(change.lifecycle.retention_seconds)
            .bind(change.lifecycle.never_expires)
            .bind(change.lifecycle.max_captures)
            .bind(change.lifecycle.capture_ttl_seconds)
            .bind(verifier)
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

fn encode_verifier(settings: &EndpointSettings) -> Result<Option<String>, EndpointError> {
    match &settings.verifier {
        None => Ok(None),
        Some(StoredVerifier::Unreadable(raw)) => Ok(Some(raw.clone())),
        Some(StoredVerifier::Configured(verifier)) => serde_json::to_string(&verifier.document())
            .map(Some)
            .map_err(EndpointError::EncodeVerifier),
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
        .bind(tags)
        .bind(settings.templating.enabled)
        .bind(settings.templating.status.as_deref()))
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
    let templated: bool = row
        .try_get("response_templated")
        .map_err(EndpointError::Query)?;
    let status_template: Option<String> = row
        .try_get("response_status_template")
        .map_err(EndpointError::Query)?;
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
    let lifecycle = Lifecycle {
        retention_seconds: row
            .try_get("retention_seconds")
            .map_err(EndpointError::Query)?,
        never_expires: row.try_get("never_expires").map_err(EndpointError::Query)?,
        max_captures: row.try_get("max_captures").map_err(EndpointError::Query)?,
        capture_ttl_seconds: row
            .try_get("capture_ttl_seconds")
            .map_err(EndpointError::Query)?,
    };
    let expires_at = lifecycle
        .effective_retention(retention)
        .map(|window| expiry(last_active_at, window))
        .transpose()?;
    let verifier_text: Option<String> = row.try_get("verifier").map_err(EndpointError::Query)?;
    let verifier = verifier_text.map(|text| {
        let readable = serde_json::from_str::<VerifierDocument>(&text)
            .map_err(|_| ())
            .and_then(|document| Verifier::from_document(&document).map_err(|_| ()));
        match readable {
            Ok(verifier) => StoredVerifier::Configured(verifier),
            Err(()) => StoredVerifier::Unreadable(text),
        }
    });

    Ok(EndpointRecord {
        id,
        settings: EndpointSettings {
            title,
            response,
            templating: ResponseTemplating {
                enabled: templated,
                status: status_template,
            },
            cors,
            overflow,
            tags,
            verifier,
        },
        capture_count,
        bytes_used,
        last_seq,
        created_at,
        last_active_at,
        expires_at,
        lifecycle,
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
