use std::time::Duration;

use bytes::Bytes;
use sqlx::Row;
use sqlx::postgres::PgRow;
use time::OffsetDateTime;
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{KeyId, SealedSecret};

use super::model::{
    ChannelKind, Delivery, DeliveryKind, delivery_columns, delivery_from_row, parse_json,
};
use super::{
    MAX_ERROR_CHARS, NotifyStoreError, TEST_BURST, WAKE_CHANNEL, classify, truncate_chars,
};
use crate::Store;
use crate::captures::NameValue;
use crate::forward::{RecordOutcome, Settlement, TIMESTAMP_SQL, lock_live_endpoint};
use crate::kind::CaptureKind;

const CAPTURE_BODY_PREFIX_BYTES: i32 = 256 * 1024;

const CLAIM: &str = "WITH due AS ( \
       SELECT d.id FROM notification_deliveries d \
       WHERE d.state IN ('pending', 'running') \
         AND d.next_attempt_at <= now() \
         AND (d.locked_until IS NULL OR d.locked_until < now()) \
         AND d.attempts < $2::int4 \
         AND NOT EXISTS ( \
           SELECT 1 FROM notification_channels ch JOIN endpoints e ON e.id = ch.endpoint_id \
           WHERE ch.id = d.channel_id AND e.deleted_at IS NOT NULL) \
       ORDER BY d.next_attempt_at \
       LIMIT $1 \
       FOR UPDATE OF d SKIP LOCKED), \
     claimed AS ( \
       UPDATE notification_deliveries d SET \
         state = 'running', \
         locked_until = now() + make_interval(secs => $3), \
         attempts = d.attempts + 1 \
       FROM due WHERE d.id = due.id \
       RETURNING d.id, d.channel_id, d.capture_id, d.kind, d.message, d.attempts, \
         d.quota_checked, d.window_start, d.suppressed_count) \
     SELECT c.id, c.channel_id, c.capture_id, c.kind, c.message, c.attempts, c.quota_checked, \
       to_char(c.window_start AT TIME ZONE 'UTC', __TIMESTAMP__) AS window_start, \
       c.suppressed_count, \
       ch.kind AS channel_kind, ch.enabled, ch.config::text AS config, ch.secret_key_id, \
       ch.secret_nonce, ch.secret_ciphertext, ch.template, ch.per_hour, ch.endpoint_id, \
       ch.created_by, e.title, e.owner_id, e.creator_net::text AS creator_net \
     FROM claimed c \
     JOIN notification_channels ch ON ch.id = c.channel_id \
     JOIN endpoints e ON e.id = ch.endpoint_id";

const LOAD_CAPTURE: &str = "SELECT c.id, c.seq, c.received_at, \
       to_char(c.received_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS received_text, c.method, c.url, c.path, c.host, \
       c.client_ip::text AS client_ip, c.content_type, c.headers::text AS headers, \
       c.query::text AS query, c.body_size, c.status_override, c.response_status, c.kind, \
       CASE WHEN strpos(c.url, '?') > 0 THEN substr(c.url, strpos(c.url, '?') + 1) ELSE '' END \
         AS query_string, \
       b.data \
     FROM captures c \
     JOIN LATERAL (SELECT substring(data from 1 for $2::int4) AS data \
                   FROM capture_bodies WHERE capture_id = c.id) b ON true \
     WHERE c.id = $1";

const SETTLE: &str = "UPDATE notification_deliveries SET \
       state = $3, \
       locked_until = NULL, \
       status = $5, \
       error = $6, \
       next_attempt_at = CASE WHEN $3 = 'pending' \
         THEN now() + make_interval(secs => $4) ELSE next_attempt_at END, \
       finished_at = CASE WHEN $3 = 'pending' THEN NULL ELSE now() END \
     WHERE id = $1 AND attempts = $2 AND state = 'running'";

const REAP: &str = "UPDATE notification_deliveries SET state = 'failed', locked_until = NULL, \
       finished_at = now(), error = 'timeout: the worker stopped before it recorded a result' \
     WHERE id IN ( \
       SELECT id FROM notification_deliveries \
       WHERE state IN ('pending', 'running') AND attempts >= $1::int4 \
         AND (locked_until IS NULL OR locked_until < now()) \
       FOR UPDATE SKIP LOCKED)";

const QUEUE_DEPTH: &str = "SELECT count(*) FROM notification_deliveries \
     WHERE state IN ('pending', 'running') AND next_attempt_at <= now()";

const ENQUEUE_SUMMARIES: &str = "INSERT INTO notification_deliveries \
       (id, channel_id, kind, state, quota_checked, window_start, suppressed_count) \
     SELECT gen_random_uuid(), s.channel_id, 'summary', 'pending', true, s.hour, s.total \
     FROM ( \
       SELECT channel_id, \
         to_timestamp(floor(extract(epoch FROM finished_at) / 3600) * 3600) AS hour, \
         count(*)::int4 AS total \
       FROM notification_deliveries \
       WHERE state = 'suppressed' AND finished_at >= now() - make_interval(hours => $1::int4) \
       GROUP BY 1, 2) s \
     WHERE s.hour + interval '1 hour' <= now() \
     ON CONFLICT (channel_id, window_start) WHERE kind = 'summary' DO NOTHING";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimPolicy {
    pub limit: i64,
    pub max_attempts: i32,
    pub lease: Duration,
}

#[derive(Debug, Clone)]
pub struct DeliveryChannel {
    pub kind: ChannelKind,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub sealed: Option<SealedSecret>,
    pub template: Option<String>,
    pub per_hour: i32,
    pub endpoint_id: String,
    pub endpoint_title: Option<String>,
    pub owner_id: Option<Uuid>,
    pub creator_net: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ClaimedDelivery {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub capture_id: Option<Uuid>,
    pub kind: DeliveryKind,
    pub message: Option<String>,
    pub attempt: i16,
    pub quota_checked: bool,
    pub window_start: Option<String>,
    pub suppressed_count: Option<i32>,
    pub channel: DeliveryChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryResult {
    pub id: Uuid,
    pub attempt: i16,
    pub status: Option<i16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NotifyCapture {
    pub id: Uuid,
    pub seq: i64,
    pub received_at: OffsetDateTime,
    pub received_text: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub query_string: String,
    pub host: String,
    pub client_ip: String,
    pub content_type: Option<String>,
    pub headers: Vec<NameValue>,
    pub query: Vec<NameValue>,
    pub body_size: i32,
    pub status_override: Option<i16>,
    pub response_status: i16,
    pub kind: CaptureKind,
    pub body: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    Queued(Delivery),
    EndpointGone,
    ChannelNotFound,
    Throttled,
}

impl Store {
    pub async fn claim_deliveries(
        &self,
        policy: ClaimPolicy,
    ) -> Result<Vec<ClaimedDelivery>, NotifyStoreError> {
        let sql = CLAIM.replace("__TIMESTAMP__", TIMESTAMP_SQL);
        let rows = sqlx::query(&sql)
            .bind(policy.limit)
            .bind(policy.max_attempts)
            .bind(policy.lease.as_secs_f64())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(claimed_from_row).collect()
    }

    pub async fn load_notify_capture(
        &self,
        capture: Uuid,
    ) -> Result<Option<NotifyCapture>, NotifyStoreError> {
        let sql = LOAD_CAPTURE.replace("__TIMESTAMP__", TIMESTAMP_SQL);
        let row = sqlx::query(&sql)
            .bind(capture)
            .bind(CAPTURE_BODY_PREFIX_BYTES)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(capture_from_row).transpose()
    }

    pub async fn settle_delivery(
        &self,
        result: &DeliveryResult,
        settlement: Settlement,
    ) -> Result<RecordOutcome, NotifyStoreError> {
        let (state, delay) = match settlement {
            Settlement::Succeeded => ("succeeded", 0.0),
            Settlement::Failed => ("failed", 0.0),
            Settlement::RetryIn(delay) => ("pending", delay.as_secs_f64()),
        };
        let error = result
            .error
            .as_deref()
            .map(|text| truncate_chars(text, MAX_ERROR_CHARS));
        let settled = sqlx::query(SETTLE)
            .bind(result.id)
            .bind(result.attempt)
            .bind(state)
            .bind(delay)
            .bind(result.status)
            .bind(error)
            .execute(self.pool())
            .await
            .map_err(classify)?
            .rows_affected();
        Ok(if settled == 1 {
            RecordOutcome::Recorded
        } else {
            RecordOutcome::Superseded
        })
    }

    pub async fn fail_exhausted_deliveries(
        &self,
        max_attempts: i32,
    ) -> Result<u64, NotifyStoreError> {
        Ok(sqlx::query(REAP)
            .bind(max_attempts)
            .execute(self.pool())
            .await
            .map_err(classify)?
            .rows_affected())
    }

    pub async fn notification_queue_depth(&self) -> Result<i64, NotifyStoreError> {
        sqlx::query_scalar(QUEUE_DEPTH)
            .fetch_one(self.pool())
            .await
            .map_err(classify)
    }

    pub async fn enqueue_summaries(&self, look_back_hours: i32) -> Result<u64, NotifyStoreError> {
        let inserted = sqlx::query(ENQUEUE_SUMMARIES)
            .bind(look_back_hours)
            .execute(self.pool())
            .await
            .map_err(classify)?
            .rows_affected();
        if inserted > 0 {
            sqlx::query("SELECT pg_notify($1, '')")
                .bind(WAKE_CHANNEL)
                .execute(self.pool())
                .await
                .map_err(classify)?;
        }
        Ok(inserted)
    }

    pub async fn prune_old_deliveries(&self) -> Result<u64, NotifyStoreError> {
        Ok(sqlx::query(
            "DELETE FROM notification_deliveries \
             WHERE capture_id IS NULL AND finished_at < now() - interval '7 days'",
        )
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected())
    }

    pub async fn prune_notification_counters(&self) -> Result<u64, NotifyStoreError> {
        Ok(sqlx::query(
            "DELETE FROM notification_counters WHERE window_start < now() - interval '2 days'",
        )
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected())
    }

    pub async fn enqueue_test_delivery(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        channel: Uuid,
        message: &str,
    ) -> Result<TestOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(TestOutcome::EndpointGone);
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM notification_channels WHERE endpoint_id = $1 AND id = $2)",
        )
        .bind(endpoint.unpack())
        .bind(channel)
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if !exists {
            return Ok(TestOutcome::ChannelNotFound);
        }
        let recent: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM notification_deliveries \
             WHERE channel_id = $1 AND kind = 'test' AND created_at > now() - interval '1 minute'",
        )
        .bind(channel)
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if recent >= TEST_BURST {
            return Ok(TestOutcome::Throttled);
        }
        let sql = format!(
            "INSERT INTO notification_deliveries (id, channel_id, kind, message, state) \
             VALUES ($1, $2, 'test', $3, 'pending') RETURNING {}",
            delivery_columns()
        );
        let row = sqlx::query(&sql)
            .bind(Uuid::now_v7())
            .bind(channel)
            .bind(truncate_chars(message, super::MAX_MESSAGE_CHARS))
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let delivery = delivery_from_row(&row)?;
        sqlx::query("SELECT pg_notify($1, '')")
            .bind(WAKE_CHANNEL)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(TestOutcome::Queued(delivery))
    }

    pub async fn deliveries_for_capture(
        &self,
        capture: Uuid,
    ) -> Result<Vec<Delivery>, NotifyStoreError> {
        let sql = format!(
            "SELECT {} FROM notification_deliveries WHERE capture_id = $1 \
             ORDER BY created_at, id LIMIT 50",
            delivery_columns()
        );
        let rows = sqlx::query(&sql)
            .bind(capture)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(delivery_from_row).collect()
    }
}

fn claimed_from_row(row: &PgRow) -> Result<ClaimedDelivery, NotifyStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    let channel_kind: String = row.try_get("channel_kind").map_err(classify)?;
    let config: String = row.try_get("config").map_err(classify)?;
    Ok(ClaimedDelivery {
        id: row.try_get("id").map_err(classify)?,
        channel_id: row.try_get("channel_id").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
        kind: DeliveryKind::parse(&kind)?,
        message: row.try_get("message").map_err(classify)?,
        attempt: row.try_get("attempts").map_err(classify)?,
        quota_checked: row.try_get("quota_checked").map_err(classify)?,
        window_start: row.try_get("window_start").map_err(classify)?,
        suppressed_count: row.try_get("suppressed_count").map_err(classify)?,
        channel: DeliveryChannel {
            kind: ChannelKind::parse(&channel_kind)?,
            enabled: row.try_get("enabled").map_err(classify)?,
            config: parse_json(&config, "channel config")?,
            sealed: sealed_from_row(row)?,
            template: row.try_get("template").map_err(classify)?,
            per_hour: row.try_get("per_hour").map_err(classify)?,
            endpoint_id: row.try_get("endpoint_id").map_err(classify)?,
            endpoint_title: row.try_get("title").map_err(classify)?,
            owner_id: row.try_get("owner_id").map_err(classify)?,
            creator_net: row.try_get("creator_net").map_err(classify)?,
            created_by: row.try_get("created_by").map_err(classify)?,
        },
    })
}

fn sealed_from_row(row: &PgRow) -> Result<Option<SealedSecret>, NotifyStoreError> {
    let key_id: Option<i16> = row.try_get("secret_key_id").map_err(classify)?;
    let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(classify)?;
    let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(classify)?;
    let (Some(key_id), Some(nonce), Some(ciphertext)) = (key_id, nonce, ciphertext) else {
        return Ok(None);
    };
    let invalid = |reason: String| NotifyStoreError::Invalid(format!("channel secret: {reason}"));
    let key_id = KeyId::from_column(key_id).map_err(|error| invalid(error.to_string()))?;
    SealedSecret::from_parts(key_id, &nonce, ciphertext)
        .map(Some)
        .map_err(|error| invalid(error.to_string()))
}

fn capture_from_row(row: &PgRow) -> Result<NotifyCapture, NotifyStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    let headers: String = row.try_get("headers").map_err(classify)?;
    let query: String = row.try_get("query").map_err(classify)?;
    let body: Vec<u8> = row.try_get("data").map_err(classify)?;
    Ok(NotifyCapture {
        id: row.try_get("id").map_err(classify)?,
        seq: row.try_get("seq").map_err(classify)?,
        received_at: row.try_get("received_at").map_err(classify)?,
        received_text: row.try_get("received_text").map_err(classify)?,
        method: row.try_get("method").map_err(classify)?,
        url: row.try_get("url").map_err(classify)?,
        path: row.try_get("path").map_err(classify)?,
        query_string: row.try_get("query_string").map_err(classify)?,
        host: row.try_get("host").map_err(classify)?,
        client_ip: row.try_get("client_ip").map_err(classify)?,
        content_type: row.try_get("content_type").map_err(classify)?,
        headers: serde_json::from_str(&headers)
            .map_err(|error| NotifyStoreError::Invalid(format!("capture headers: {error}")))?,
        query: serde_json::from_str(&query)
            .map_err(|error| NotifyStoreError::Invalid(format!("capture query: {error}")))?,
        body_size: row.try_get("body_size").map_err(classify)?,
        status_override: row.try_get("status_override").map_err(classify)?,
        response_status: row.try_get("response_status").map_err(classify)?,
        kind: CaptureKind::parse(&kind)
            .ok_or_else(|| NotifyStoreError::Invalid(format!("capture kind {kind:?}")))?,
        body: Bytes::from(body),
    })
}
