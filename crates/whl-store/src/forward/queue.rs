use std::time::Duration;

use bytes::Bytes;
use sqlx::postgres::{PgListener, PgRow};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use super::jobs::JobKind;
use super::{ForwardStoreError, classify, notify_event};
use crate::Store;
use crate::captures::NameValue;

pub const WAKE_CHANNEL: &str = "whl_forward_wake";

const INSERT_RULE_JOBS: &str = "WITH inserted AS ( \
       INSERT INTO forward_jobs \
         (id, capture_id, rule_id, kind, target_url, method, content_type, header_allowlist, \
          append_path, state) \
       SELECT gen_random_uuid(), $2, r.id, 'rule', r.target_url, COALESCE(r.method, $3), \
         r.content_type, r.header_allowlist, r.append_path, 'pending' \
       FROM forward_rules r \
       WHERE r.endpoint_id = $1 AND r.enabled \
       ORDER BY r.created_at, r.position \
       RETURNING 1) \
     SELECT pg_notify($4, '') FROM (SELECT 1 FROM inserted LIMIT 1) AS first_job";

const CLAIM: &str = "WITH due AS ( \
       SELECT j.id FROM forward_jobs j \
       WHERE j.state IN ('pending', 'running') \
         AND j.next_attempt_at <= now() \
         AND (j.locked_until IS NULL OR j.locked_until < now()) \
         AND j.attempts < $2::int4 \
         AND NOT EXISTS ( \
           SELECT 1 FROM captures c JOIN endpoints e ON e.id = c.endpoint_id \
           WHERE c.id = j.capture_id AND e.deleted_at IS NOT NULL) \
       ORDER BY j.next_attempt_at \
       LIMIT $1 \
       FOR UPDATE OF j SKIP LOCKED) \
     UPDATE forward_jobs j SET \
       state = 'running', \
       locked_until = now() + make_interval(secs => $3), \
       attempts = j.attempts + 1 \
     FROM due WHERE j.id = due.id \
     RETURNING j.id, j.capture_id, j.rule_id, j.kind, j.target_url, j.method, j.content_type, \
       j.header_allowlist, j.append_path, j.attempts";

const LOAD_CAPTURE: &str = "SELECT c.endpoint_id, c.path, \
       CASE WHEN strpos(c.url, '?') > 0 THEN substr(c.url, strpos(c.url, '?') + 1) ELSE '' END \
         AS query_string, \
       c.content_type, c.headers::text AS headers, b.data \
     FROM captures c JOIN capture_bodies b ON b.capture_id = c.id \
     WHERE c.id = $1";

const SETTLE: &str = "UPDATE forward_jobs SET \
       state = $3, \
       locked_until = NULL, \
       next_attempt_at = CASE WHEN $3 = 'pending' \
         THEN now() + make_interval(secs => $4) ELSE next_attempt_at END, \
       finished_at = CASE WHEN $3 = 'pending' THEN NULL ELSE now() END \
     WHERE id = $1 AND attempts = $2 AND state = 'running' \
     RETURNING capture_id, (SELECT endpoint_id FROM captures WHERE id = forward_jobs.capture_id) \
       AS endpoint_id";

const INSERT_ATTEMPT: &str = "INSERT INTO forward_attempts \
     (job_id, attempt, started_at, duration_ms, status, error, final_url, \
      response_content_type, response_body, response_truncated) \
     VALUES ($1, $2, clock_timestamp() - make_interval(secs => ($3::int4)::float8 / 1000.0), $3::int4, $4, $5, \
       $6, $7, $8, $9)";

const REAP: &str = "WITH dead AS ( \
       UPDATE forward_jobs SET state = 'failed', locked_until = NULL, finished_at = now() \
       WHERE id IN ( \
         SELECT id FROM forward_jobs \
         WHERE state = 'running' AND next_attempt_at <= now() \
           AND locked_until < now() AND attempts >= $1::int4 \
         FOR UPDATE SKIP LOCKED) \
       RETURNING id, capture_id, attempts), \
     recorded AS ( \
       INSERT INTO forward_attempts (job_id, attempt, started_at, duration_ms, error) \
       SELECT id, attempts, now(), 0, \
         'timeout: the worker stopped before it recorded a result' FROM dead \
       ON CONFLICT DO NOTHING \
       RETURNING job_id) \
     SELECT d.id, d.capture_id, c.endpoint_id \
     FROM dead d JOIN captures c ON c.id = d.capture_id";

const QUEUE_DEPTH: &str = "SELECT count(*) FROM forward_jobs \
     WHERE state IN ('pending', 'running') AND next_attempt_at <= now()";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimPolicy {
    pub limit: i64,
    pub max_attempts: i32,
    pub lease: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub capture_id: Uuid,
    pub rule_id: Option<Uuid>,
    pub kind: JobKind,
    pub target_url: String,
    pub method: String,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub append_path: bool,
    pub attempt: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobCapture {
    pub endpoint_id: String,
    pub path: String,
    pub query_string: String,
    pub content_type: Option<String>,
    pub headers: Vec<NameValue>,
    pub body: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub job_id: Uuid,
    pub attempt: i16,
    pub duration_ms: i32,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub final_url: Option<String>,
    pub response_content_type: Option<String>,
    pub response_body: Option<Vec<u8>>,
    pub response_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Succeeded,
    Failed,
    RetryIn(Duration),
}

impl Settlement {
    fn state(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::RetryIn(_) => "pending",
        }
    }

    fn delay_secs(self) -> f64 {
        match self {
            Self::RetryIn(delay) => delay.as_secs_f64(),
            Self::Succeeded | Self::Failed => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Recorded,
    Superseded,
}

#[derive(Debug)]
pub struct WakeListener {
    inner: PgListener,
}

impl WakeListener {
    pub async fn wait(&mut self) -> Result<(), ForwardStoreError> {
        self.inner.recv().await.map(|_| ()).map_err(classify)
    }
}

pub(crate) async fn insert_rule_jobs(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    capture: Uuid,
    method: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(INSERT_RULE_JOBS)
        .bind(endpoint.unpack())
        .bind(capture)
        .bind(method)
        .bind(WAKE_CHANNEL)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

impl Store {
    pub async fn claim_forward_jobs(
        &self,
        policy: ClaimPolicy,
    ) -> Result<Vec<ClaimedJob>, ForwardStoreError> {
        let rows = sqlx::query(CLAIM)
            .bind(policy.limit)
            .bind(policy.max_attempts)
            .bind(policy.lease.as_secs_f64())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(claimed_from_row).collect()
    }

    pub async fn load_job_capture(
        &self,
        capture: Uuid,
    ) -> Result<Option<JobCapture>, ForwardStoreError> {
        let row = sqlx::query(LOAD_CAPTURE)
            .bind(capture)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(capture_from_row).transpose()
    }

    pub async fn record_forward_attempt(
        &self,
        record: &AttemptRecord,
        settlement: Settlement,
    ) -> Result<RecordOutcome, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let settled = sqlx::query(SETTLE)
            .bind(record.job_id)
            .bind(record.attempt)
            .bind(settlement.state())
            .bind(settlement.delay_secs())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(settled) = settled else {
            return Ok(RecordOutcome::Superseded);
        };
        let capture_id: Uuid = settled.try_get("capture_id").map_err(classify)?;
        let endpoint_id: String = settled.try_get("endpoint_id").map_err(classify)?;
        sqlx::query(INSERT_ATTEMPT)
            .bind(record.job_id)
            .bind(record.attempt)
            .bind(record.duration_ms)
            .bind(record.status)
            .bind(record.error.as_deref())
            .bind(record.final_url.as_deref())
            .bind(record.response_content_type.as_deref())
            .bind(record.response_body.as_deref())
            .bind(record.response_truncated)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        notify_forward_result(&mut tx, &endpoint_id, capture_id, record.job_id).await?;
        tx.commit().await.map_err(classify)?;
        Ok(RecordOutcome::Recorded)
    }

    pub async fn fail_exhausted_forward_jobs(
        &self,
        max_attempts: i32,
    ) -> Result<u64, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let rows = sqlx::query(REAP)
            .bind(max_attempts)
            .fetch_all(&mut *tx)
            .await
            .map_err(classify)?;
        for row in &rows {
            let job_id: Uuid = row.try_get("id").map_err(classify)?;
            let capture_id: Uuid = row.try_get("capture_id").map_err(classify)?;
            let endpoint_id: String = row.try_get("endpoint_id").map_err(classify)?;
            notify_forward_result(&mut tx, &endpoint_id, capture_id, job_id).await?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(u64::try_from(rows.len()).unwrap_or(u64::MAX))
    }

    pub async fn forward_queue_depth(&self) -> Result<i64, ForwardStoreError> {
        sqlx::query_scalar(QUEUE_DEPTH)
            .fetch_one(self.pool())
            .await
            .map_err(classify)
    }

    pub async fn listen_for_forward_wakeups(&self) -> Result<WakeListener, ForwardStoreError> {
        let mut inner = PgListener::connect_with(self.pool())
            .await
            .map_err(classify)?;
        inner.listen(WAKE_CHANNEL).await.map_err(classify)?;
        Ok(WakeListener { inner })
    }
}

async fn notify_forward_result(
    tx: &mut Transaction<'_, Postgres>,
    endpoint_id: &str,
    capture_id: Uuid,
    job_id: Uuid,
) -> Result<(), ForwardStoreError> {
    let payload = serde_json::json!({
        "t": "forward_result",
        "e": endpoint_id,
        "d": {
            "capture_id": capture_id.hyphenated().to_string(),
            "job_id": job_id.hyphenated().to_string(),
        },
    });
    notify_event(tx, &payload).await
}

fn claimed_from_row(row: &PgRow) -> Result<ClaimedJob, ForwardStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    Ok(ClaimedJob {
        id: row.try_get("id").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
        rule_id: row.try_get("rule_id").map_err(classify)?,
        kind: JobKind::parse(&kind)?,
        target_url: row.try_get("target_url").map_err(classify)?,
        method: row.try_get("method").map_err(classify)?,
        content_type: row.try_get("content_type").map_err(classify)?,
        header_allowlist: row.try_get("header_allowlist").map_err(classify)?,
        append_path: row.try_get("append_path").map_err(classify)?,
        attempt: row.try_get("attempts").map_err(classify)?,
    })
}

fn capture_from_row(row: &PgRow) -> Result<JobCapture, ForwardStoreError> {
    let headers: String = row.try_get("headers").map_err(classify)?;
    let headers = serde_json::from_str(&headers)
        .map_err(|error| ForwardStoreError::Invalid(format!("capture headers: {error}")))?;
    let body: Vec<u8> = row.try_get("data").map_err(classify)?;
    Ok(JobCapture {
        endpoint_id: row.try_get("endpoint_id").map_err(classify)?,
        path: row.try_get("path").map_err(classify)?,
        query_string: row.try_get("query_string").map_err(classify)?,
        content_type: row.try_get("content_type").map_err(classify)?,
        headers,
        body: Bytes::from(body),
    })
}
