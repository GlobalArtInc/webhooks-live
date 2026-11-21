use std::time::Duration;

use serde_json::Value;
use sqlx::Row;
use sqlx::postgres::PgRow;
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::Uuid;
use crate::forward::TIMESTAMP_SQL;

pub const MAX_SENT_PER_OWNER: i64 = 50;
pub const MAX_STORED_RESPONSE_BODY_BYTES: usize = 64 * 1024;
pub const OUTBOUND_METRIC: &str = "outbound";
pub const USAGE_RETENTION_DAYS: i32 = 2;

const FOREIGN_KEY_VIOLATION: &str = "23503";
const HOUR_SECONDS: i64 = 3600;
const DAY_SECONDS: i64 = 86_400;

const SESSION_LIMITS: &str = "SELECT \
       set_config('statement_timeout', '2s', true), \
       set_config('idle_in_transaction_session_timeout', '5s', true)";

const CONSUME: &str = "INSERT INTO usage_counters (subject, metric, window_start, count) \
     VALUES ($1, $2, date_trunc($3, now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC', 1) \
     ON CONFLICT (subject, metric, window_start) \
     DO UPDATE SET count = usage_counters.count + 1 \
     WHERE usage_counters.count < $4 \
     RETURNING count";

const PURGE_USAGE: &str =
    "DELETE FROM usage_counters WHERE window_start < now() - make_interval(days => $1)";

const LOCK_OWNER: &str = "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))";

const INSERT_SENT: &str = "INSERT INTO sent_requests \
     (id, user_id, endpoint_id, source_capture_id, request, result, response_body) \
     VALUES ($1, $2, $3, $4, $5::text::jsonb, $6::text::jsonb, $7)";

#[derive(Debug, thiserror::Error)]
pub enum ComposerStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the composer query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored composer data is invalid: {0}")]
    Invalid(String),
}

impl ComposerStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

fn classify(error: sqlx::Error) -> ComposerStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        ComposerStoreError::Unavailable(error)
    } else {
        ComposerStoreError::Query(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaWindow {
    Hour,
    Day,
}

impl QuotaWindow {
    fn truncation(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }

    fn length_seconds(self) -> i64 {
        match self {
            Self::Hour => HOUR_SECONDS,
            Self::Day => DAY_SECONDS,
        }
    }

    pub fn retry_after(self, now: OffsetDateTime) -> Duration {
        let length = self.length_seconds();
        let elapsed = now.unix_timestamp().rem_euclid(length);
        Duration::from_secs(u64::try_from(length - elapsed).unwrap_or(1).max(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaCharge {
    pub subject: String,
    pub window: QuotaWindow,
    pub limit: u32,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaOutcome {
    Allowed,
    Exceeded {
        name: &'static str,
        limit: u32,
        retry_after: Duration,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentOwner {
    User(Uuid),
    Endpoint(EndpointId),
}

impl SentOwner {
    fn lock_key(&self) -> String {
        match self {
            Self::User(id) => format!("sent:u:{}", id.hyphenated()),
            Self::Endpoint(id) => format!("sent:e:{}", id.unpack()),
        }
    }

    fn predicate(&self) -> &'static str {
        match self {
            Self::User(_) => "user_id = $1",
            Self::Endpoint(_) => "endpoint_id = $1 AND user_id IS NULL",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSent {
    pub id: Uuid,
    pub owner: SentOwner,
    pub source_capture_id: Option<Uuid>,
    pub request: Value,
    pub result: Value,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentOutcome {
    Recorded,
    OwnerGone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SentRecord {
    pub id: Uuid,
    pub source_capture_id: Option<Uuid>,
    pub created_at: String,
    pub request: Value,
    pub result: Value,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SentPage {
    pub items: Vec<SentRecord>,
    pub next_after: Option<Uuid>,
}

impl Store {
    pub async fn consume_quota(
        &self,
        metric: &str,
        charges: &[QuotaCharge],
    ) -> Result<QuotaOutcome, ComposerStoreError> {
        let mut ordered: Vec<&QuotaCharge> = charges.iter().collect();
        ordered.sort_by(|left, right| {
            (&left.subject, left.window.truncation())
                .cmp(&(&right.subject, right.window.truncation()))
        });
        if let Some(charge) = ordered.iter().find(|charge| charge.limit == 0) {
            return Ok(exceeded(charge));
        }
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        for charge in ordered {
            let counted = sqlx::query(CONSUME)
                .bind(&charge.subject)
                .bind(metric)
                .bind(charge.window.truncation())
                .bind(i64::from(charge.limit))
                .fetch_optional(&mut *tx)
                .await
                .map_err(classify)?;
            if counted.is_none() {
                tx.rollback().await.map_err(classify)?;
                return Ok(exceeded(charge));
            }
        }
        tx.commit().await.map_err(classify)?;
        Ok(QuotaOutcome::Allowed)
    }

    pub async fn delete_old_usage_windows(&self) -> Result<u64, ComposerStoreError> {
        let outcome = sqlx::query(PURGE_USAGE)
            .bind(USAGE_RETENTION_DAYS)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected())
    }

    pub async fn record_sent(&self, sent: &NewSent) -> Result<SentOutcome, ComposerStoreError> {
        if sent
            .response_body
            .as_ref()
            .is_some_and(|body| body.len() > MAX_STORED_RESPONSE_BODY_BYTES)
        {
            return Err(ComposerStoreError::Invalid(
                "the stored response body is over its cap".to_owned(),
            ));
        }
        let request = serde_json::to_string(&sent.request)
            .map_err(|error| ComposerStoreError::Invalid(error.to_string()))?;
        let result = serde_json::to_string(&sent.result)
            .map_err(|error| ComposerStoreError::Invalid(error.to_string()))?;
        let (user_id, endpoint_id) = match &sent.owner {
            SentOwner::User(id) => (Some(*id), None),
            SentOwner::Endpoint(id) => (None, Some(id.unpack().to_owned())),
        };
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query(LOCK_OWNER)
            .bind(sent.owner.lock_key())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let inserted = sqlx::query(INSERT_SENT)
            .bind(sent.id)
            .bind(user_id)
            .bind(endpoint_id)
            .bind(sent.source_capture_id)
            .bind(request)
            .bind(result)
            .bind(sent.response_body.as_deref())
            .execute(&mut *tx)
            .await;
        if let Err(error) = inserted {
            let violated = error
                .as_database_error()
                .and_then(|database| database.code())
                .is_some_and(|code| code == FOREIGN_KEY_VIOLATION);
            if violated {
                return Ok(SentOutcome::OwnerGone);
            }
            return Err(classify(error));
        }
        let trim = format!(
            "DELETE FROM sent_requests WHERE id IN ( \
               SELECT id FROM sent_requests WHERE {} \
               ORDER BY created_at DESC, id DESC OFFSET $2)",
            sent.owner.predicate()
        );
        let query = sqlx::query(&trim);
        let query = match &sent.owner {
            SentOwner::User(id) => query.bind(*id),
            SentOwner::Endpoint(id) => query.bind(id.unpack().to_owned()),
        };
        query
            .bind(MAX_SENT_PER_OWNER)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(SentOutcome::Recorded)
    }

    pub async fn list_sent(
        &self,
        owner: &SentOwner,
        limit: i64,
        after: Option<Uuid>,
    ) -> Result<SentPage, ComposerStoreError> {
        let predicate = owner.predicate();
        let sql = format!(
            "SELECT id, source_capture_id, \
               to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_text, \
               request::text AS request_text, result::text AS result_text, response_body \
             FROM sent_requests \
             WHERE {predicate} \
               AND ($2::uuid IS NULL OR (created_at, id) < ( \
                 SELECT created_at, id FROM sent_requests WHERE id = $2 AND {predicate})) \
             ORDER BY created_at DESC, id DESC LIMIT $3"
        );
        let query = sqlx::query(&sql);
        let query = match owner {
            SentOwner::User(id) => query.bind(*id),
            SentOwner::Endpoint(id) => query.bind(id.unpack().to_owned()),
        };
        let rows = query
            .bind(after)
            .bind(limit + 1)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let mut items = rows
            .iter()
            .map(record_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let more = i64::try_from(items.len()).is_ok_and(|held| held > limit);
        if more {
            items.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        let next_after = if more {
            items.last().map(|last| last.id)
        } else {
            None
        };
        Ok(SentPage { items, next_after })
    }

    pub async fn sent_owner(&self, id: Uuid) -> Result<Option<SentOwner>, ComposerStoreError> {
        let row = sqlx::query("SELECT user_id, endpoint_id FROM sent_requests WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let user: Option<Uuid> = row.try_get("user_id").map_err(classify)?;
        let endpoint: Option<String> = row.try_get("endpoint_id").map_err(classify)?;
        match (user, endpoint) {
            (Some(user), _) => Ok(Some(SentOwner::User(user))),
            (None, Some(endpoint)) => EndpointId::parse(&endpoint)
                .map(|endpoint| Some(SentOwner::Endpoint(endpoint)))
                .map_err(|_| ComposerStoreError::Invalid("a stored endpoint id is invalid".into())),
            (None, None) => Err(ComposerStoreError::Invalid(
                "a history entry has no owner".to_owned(),
            )),
        }
    }

    pub async fn delete_sent(
        &self,
        id: Uuid,
        owner: &SentOwner,
    ) -> Result<bool, ComposerStoreError> {
        let sql = format!(
            "DELETE FROM sent_requests WHERE id = $2 AND {}",
            owner.predicate()
        );
        let query = sqlx::query(&sql);
        let query = match owner {
            SentOwner::User(user) => query.bind(*user),
            SentOwner::Endpoint(endpoint) => query.bind(endpoint.unpack().to_owned()),
        };
        let outcome = query
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected() > 0)
    }
}

fn exceeded(charge: &QuotaCharge) -> QuotaOutcome {
    QuotaOutcome::Exceeded {
        name: charge.name,
        limit: charge.limit,
        retry_after: charge.window.retry_after(OffsetDateTime::now_utc()),
    }
}

fn parse_json(text: &str) -> Result<Value, ComposerStoreError> {
    serde_json::from_str(text).map_err(|error| ComposerStoreError::Invalid(error.to_string()))
}

fn record_from_row(row: &PgRow) -> Result<SentRecord, ComposerStoreError> {
    let request: String = row.try_get("request_text").map_err(classify)?;
    let result: String = row.try_get("result_text").map_err(classify)?;
    Ok(SentRecord {
        id: row.try_get("id").map_err(classify)?,
        source_capture_id: row.try_get("source_capture_id").map_err(classify)?,
        created_at: row.try_get("created_text").map_err(classify)?,
        request: parse_json(&request)?,
        result: parse_json(&result)?,
        response_body: row.try_get("response_body").map_err(classify)?,
    })
}
