use std::collections::HashMap;
use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use super::{ForwardStoreError, TIMESTAMP_SQL, classify, lock_live_endpoint, queue::WAKE_CHANNEL};
use crate::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Rule,
    Manual,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Manual => "manual",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self, ForwardStoreError> {
        match text {
            "rule" => Ok(Self::Rule),
            "manual" => Ok(Self::Manual),
            other => Err(ForwardStoreError::Invalid(format!(
                "unknown job kind {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Result<Self, ForwardStoreError> {
        match text {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(ForwardStoreError::Invalid(format!(
                "unknown job state {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardAttempt {
    pub attempt: i16,
    pub started_at: String,
    pub duration_ms: i32,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub final_url: Option<String>,
    pub response_content_type: Option<String>,
    pub response_body: Option<Vec<u8>>,
    pub response_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardJob {
    pub id: Uuid,
    pub capture_id: Uuid,
    pub kind: JobKind,
    pub rule_id: Option<Uuid>,
    pub target_url: String,
    pub method: String,
    pub state: JobState,
    pub attempts: Vec<ForwardAttempt>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTarget {
    pub target_url: String,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualTarget {
    Rule(Uuid),
    Custom(CustomTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualJobOutcome {
    Created(ForwardJob),
    EndpointGone,
    CaptureNotFound,
    RuleNotFound,
}

const RULE_JOB: &str = "INSERT INTO forward_jobs \
     (id, capture_id, rule_id, kind, target_url, method, content_type, header_allowlist, \
      append_path, state) \
     SELECT gen_random_uuid(), c.id, r.id, 'manual', r.target_url, COALESCE(r.method, c.method), \
       r.content_type, r.header_allowlist, r.append_path, 'pending' \
     FROM captures c, forward_rules r \
     WHERE c.id = $1 AND c.endpoint_id = $2 AND r.id = $3 AND r.endpoint_id = $2 \
     RETURNING id";

const CUSTOM_JOB: &str = "INSERT INTO forward_jobs \
     (id, capture_id, rule_id, kind, target_url, method, content_type, header_allowlist, \
      append_path, state) \
     SELECT gen_random_uuid(), c.id, NULL, 'manual', $3, COALESCE($4, c.method), $5, $6, false, \
       'pending' \
     FROM captures c WHERE c.id = $1 AND c.endpoint_id = $2 \
     RETURNING id";

fn job_columns() -> String {
    format!(
        "j.id, j.capture_id, j.rule_id, j.kind, j.target_url, j.method, j.state, \
         to_char(j.created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at, \
         to_char(j.finished_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS finished_at"
    )
}

fn attempt_columns() -> String {
    format!(
        "job_id, attempt, to_char(started_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS started_at, \
         duration_ms, status, error, final_url, response_content_type, response_body, \
         response_truncated"
    )
}

impl Store {
    pub async fn forward_job(&self, id: Uuid) -> Result<Option<ForwardJob>, ForwardStoreError> {
        let mut connection = self.pool().acquire().await.map_err(classify)?;
        fetch_job(&mut connection, id).await
    }

    pub async fn forward_jobs_for_capture(
        &self,
        capture: Uuid,
    ) -> Result<Vec<ForwardJob>, ForwardStoreError> {
        let sql = format!(
            "SELECT {} FROM forward_jobs j WHERE j.capture_id = $1 \
             ORDER BY j.created_at DESC, j.id DESC",
            job_columns()
        );
        let rows = sqlx::query(&sql)
            .bind(capture)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let mut connection = self.pool().acquire().await.map_err(classify)?;
        assemble(&mut connection, &rows).await
    }

    pub async fn create_manual_job(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        capture: Uuid,
        target: &ManualTarget,
    ) -> Result<ManualJobOutcome, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(ManualJobOutcome::EndpointGone);
        }
        let capture_exists =
            sqlx::query("SELECT 1 FROM captures WHERE id = $1 AND endpoint_id = $2")
                .bind(capture)
                .bind(endpoint.unpack())
                .fetch_optional(&mut *tx)
                .await
                .map_err(classify)?;
        if capture_exists.is_none() {
            return Ok(ManualJobOutcome::CaptureNotFound);
        }
        let inserted = match target {
            ManualTarget::Rule(rule_id) => {
                let rule_exists =
                    sqlx::query("SELECT 1 FROM forward_rules WHERE id = $1 AND endpoint_id = $2")
                        .bind(rule_id)
                        .bind(endpoint.unpack())
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(classify)?;
                if rule_exists.is_none() {
                    return Ok(ManualJobOutcome::RuleNotFound);
                }
                sqlx::query(RULE_JOB)
                    .bind(capture)
                    .bind(endpoint.unpack())
                    .bind(rule_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(classify)?
            }
            ManualTarget::Custom(custom) => sqlx::query(CUSTOM_JOB)
                .bind(capture)
                .bind(endpoint.unpack())
                .bind(custom.target_url.as_str())
                .bind(custom.method.as_deref())
                .bind(custom.content_type.as_deref())
                .bind(&custom.header_allowlist)
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?,
        };
        let job_id: Uuid = inserted.try_get("id").map_err(classify)?;
        sqlx::query("SELECT pg_notify($1, '')")
            .bind(WAKE_CHANNEL)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let job = fetch_job(&mut tx, job_id).await?.ok_or_else(|| {
            ForwardStoreError::Invalid("the job vanished inside its own transaction".to_owned())
        })?;
        tx.commit().await.map_err(classify)?;
        Ok(ManualJobOutcome::Created(job))
    }
}

async fn fetch_job(
    connection: &mut PgConnection,
    id: Uuid,
) -> Result<Option<ForwardJob>, ForwardStoreError> {
    let sql = format!(
        "SELECT {} FROM forward_jobs j WHERE j.id = $1",
        job_columns()
    );
    let rows = sqlx::query(&sql)
        .bind(id)
        .fetch_all(&mut *connection)
        .await
        .map_err(classify)?;
    Ok(assemble(connection, &rows).await?.pop())
}

async fn assemble(
    connection: &mut PgConnection,
    job_rows: &[PgRow],
) -> Result<Vec<ForwardJob>, ForwardStoreError> {
    let ids: Vec<Uuid> = job_rows
        .iter()
        .map(|row| row.try_get("id").map_err(classify))
        .collect::<Result<_, _>>()?;
    let sql = format!(
        "SELECT {} FROM forward_attempts WHERE job_id = ANY($1) ORDER BY job_id, attempt",
        attempt_columns()
    );
    let attempt_rows = sqlx::query(&sql)
        .bind(&ids)
        .fetch_all(connection)
        .await
        .map_err(classify)?;
    let mut by_job: HashMap<Uuid, Vec<ForwardAttempt>> = HashMap::new();
    for row in &attempt_rows {
        let job_id: Uuid = row.try_get("job_id").map_err(classify)?;
        by_job
            .entry(job_id)
            .or_default()
            .push(attempt_from_row(row)?);
    }
    job_rows
        .iter()
        .map(|row| {
            let id: Uuid = row.try_get("id").map_err(classify)?;
            job_from_row(row, by_job.remove(&id).unwrap_or_default())
        })
        .collect()
}

fn job_from_row(
    row: &PgRow,
    attempts: Vec<ForwardAttempt>,
) -> Result<ForwardJob, ForwardStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    let state: String = row.try_get("state").map_err(classify)?;
    Ok(ForwardJob {
        id: row.try_get("id").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
        kind: JobKind::parse(&kind)?,
        rule_id: row.try_get("rule_id").map_err(classify)?,
        target_url: row.try_get("target_url").map_err(classify)?,
        method: row.try_get("method").map_err(classify)?,
        state: JobState::parse(&state)?,
        attempts,
        created_at: row.try_get("created_at").map_err(classify)?,
        finished_at: row.try_get("finished_at").map_err(classify)?,
    })
}

fn attempt_from_row(row: &PgRow) -> Result<ForwardAttempt, ForwardStoreError> {
    Ok(ForwardAttempt {
        attempt: row.try_get("attempt").map_err(classify)?,
        started_at: row.try_get("started_at").map_err(classify)?,
        duration_ms: row.try_get("duration_ms").map_err(classify)?,
        status: row.try_get("status").map_err(classify)?,
        error: row.try_get("error").map_err(classify)?,
        final_url: row.try_get("final_url").map_err(classify)?,
        response_content_type: row.try_get("response_content_type").map_err(classify)?,
        response_body: row.try_get("response_body").map_err(classify)?,
        response_truncated: row.try_get("response_truncated").map_err(classify)?,
    })
}
