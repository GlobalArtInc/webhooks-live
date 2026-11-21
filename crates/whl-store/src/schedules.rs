use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::forward::{ForwardStoreError, lock_live_endpoint};

pub const MAX_RUN_HISTORY: i64 = 50;
pub const MAX_PAUSED_REASON_BYTES: usize = 500;
pub const MAX_RUN_ERROR_BYTES: usize = 500;
pub const QUOTA_RETENTION_HOURS: i32 = 2;

const COLUMNS: &str = "id, endpoint_id, name, cron, timezone, request::text AS request, enabled, \
     paused_reason, next_run_at, last_run_at, consecutive_failures, created_at";

const CLAIM: &str = "WITH due AS ( \
       SELECT s.id FROM schedules s JOIN endpoints e ON e.id = s.endpoint_id \
       WHERE s.enabled AND s.next_run_at IS NOT NULL AND s.next_run_at <= $1 \
         AND (s.locked_until IS NULL OR s.locked_until < $1) \
         AND e.deleted_at IS NULL \
         AND e.last_active_at > $1 - make_interval(secs => $4) \
       ORDER BY s.next_run_at, s.id \
       LIMIT $2 \
       FOR UPDATE OF s SKIP LOCKED) \
     UPDATE schedules s SET locked_until = $1 + make_interval(secs => $3) \
     FROM due WHERE s.id = due.id \
     RETURNING s.id, s.endpoint_id, s.name, s.cron, s.timezone, s.request::text AS request, \
       s.next_run_at, s.consecutive_failures, s.locked_until, \
       (SELECT e.owner_id FROM endpoints e WHERE e.id = s.endpoint_id) AS owner_id";

const SETTLE: &str = "UPDATE schedules SET \
       last_run_at = COALESCE($3, last_run_at), \
       consecutive_failures = $4, \
       enabled = $5, \
       paused_reason = $6, \
       next_run_at = CASE \
         WHEN NOT $5 THEN NULL \
         WHEN next_run_at IS DISTINCT FROM $7 THEN next_run_at \
         ELSE $8 END, \
       locked_until = NULL \
     WHERE id = $1 AND locked_until = $2";

const INSERT_RUN: &str = "INSERT INTO schedule_runs \
       (id, schedule_id, started_at, duration_ms, outcome, status, error, capture_id) \
     SELECT $1, $2, $3, $4, $5, $6, $7, $8 \
     WHERE EXISTS (SELECT 1 FROM schedules WHERE id = $2)";

const TRIM_RUNS: &str = "DELETE FROM schedule_runs WHERE schedule_id = $1 AND id NOT IN ( \
       SELECT id FROM schedule_runs WHERE schedule_id = $1 \
       ORDER BY started_at DESC, id DESC LIMIT $2)";

const CONSUME_QUOTA: &str = "WITH pruned AS ( \
       DELETE FROM schedule_quota_windows \
       WHERE owner_id = $1 AND window_start < $2 - make_interval(hours => $4)) \
     INSERT INTO schedule_quota_windows (owner_id, window_start, count) VALUES ($1, $2, 1) \
     ON CONFLICT (owner_id, window_start) DO UPDATE \
       SET count = schedule_quota_windows.count + 1 \
       WHERE schedule_quota_windows.count < $3 \
     RETURNING count";

#[derive(Debug, thiserror::Error)]
pub enum ScheduleStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the schedule query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored schedule data is invalid: {0}")]
    Invalid(String),
}

impl ScheduleStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl From<ForwardStoreError> for ScheduleStoreError {
    fn from(error: ForwardStoreError) -> Self {
        match error {
            ForwardStoreError::Unavailable(source) => Self::Unavailable(source),
            ForwardStoreError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

fn classify(error: sqlx::Error) -> ScheduleStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        ScheduleStoreError::Unavailable(error)
    } else {
        ScheduleStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<ScheduleHeader>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRecord {
    pub id: Uuid,
    pub endpoint_id: String,
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub request: ScheduleRequest,
    pub enabled: bool,
    pub paused_reason: Option<String>,
    pub next_run_at: Option<OffsetDateTime>,
    pub last_run_at: Option<OffsetDateTime>,
    pub consecutive_failures: i16,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSchedule {
    pub created_by: Uuid,
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub request: ScheduleRequest,
    pub enabled: bool,
    pub next_run_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleUpdate {
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub request: ScheduleRequest,
    pub enabled: bool,
    pub next_run_at: Option<OffsetDateTime>,
    pub reset_failures: bool,
    pub release_lease: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateScheduleOutcome {
    Created(Box<ScheduleRecord>),
    EndpointGone,
    NoOwner,
    LimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateScheduleOutcome {
    Updated(Box<ScheduleRecord>),
    EndpointGone,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchScheduleOutcome<E> {
    Updated(Box<ScheduleRecord>),
    EndpointGone,
    NotFound,
    Refused(E),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteScheduleOutcome {
    Deleted,
    EndpointGone,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueRunOutcome {
    Queued(OffsetDateTime),
    EndpointGone,
    NotFound,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleClaimPolicy {
    pub now: OffsetDateTime,
    pub limit: i64,
    pub lease: Duration,
    pub retention: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSchedule {
    pub id: Uuid,
    pub endpoint_id: EndpointId,
    pub owner_id: Option<Uuid>,
    pub name: String,
    pub cron: String,
    pub timezone: String,
    pub request: ScheduleRequest,
    pub scheduled_for: OffsetDateTime,
    pub consecutive_failures: i16,
    pub lease: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    Failed,
    Skipped,
}

impl RunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Succeeded, Self::Failed, Self::Skipped]
            .into_iter()
            .find(|outcome| outcome.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewScheduleRun {
    pub id: Uuid,
    pub started_at: OffsetDateTime,
    pub duration_ms: i32,
    pub outcome: RunOutcome,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub capture_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRunRecord {
    pub id: Uuid,
    pub started_at: OffsetDateTime,
    pub duration_ms: i32,
    pub outcome: RunOutcome,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub capture_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSettlement {
    pub id: Uuid,
    pub lease: OffsetDateTime,
    pub scheduled_for: OffsetDateTime,
    pub last_run_at: Option<OffsetDateTime>,
    pub consecutive_failures: i16,
    pub paused_reason: Option<String>,
    pub next_run_at: Option<OffsetDateTime>,
    pub run: NewScheduleRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleOutcome {
    Settled,
    LeaseLost,
}

impl Store {
    pub async fn list_schedules(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<ScheduleRecord>, ScheduleStoreError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM schedules WHERE endpoint_id = $1 ORDER BY created_at, id"
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(schedule_from_row).collect()
    }

    pub async fn get_schedule(
        &self,
        endpoint: &EndpointId,
        id: Uuid,
    ) -> Result<Option<ScheduleRecord>, ScheduleStoreError> {
        let sql = format!("SELECT {COLUMNS} FROM schedules WHERE endpoint_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(schedule_from_row).transpose()
    }

    pub async fn create_schedule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        schedule: &NewSchedule,
        max_per_owner: usize,
    ) -> Result<CreateScheduleOutcome, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let owner: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT owner_id FROM endpoints WHERE id = $1 AND deleted_at IS NULL \
             AND last_active_at > now() - make_interval(secs => $2)",
        )
        .bind(endpoint.unpack())
        .bind(retention.as_secs_f64())
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        let Some(owner) = owner else {
            return Ok(CreateScheduleOutcome::EndpointGone);
        };
        let Some(owner) = owner else {
            return Ok(CreateScheduleOutcome::NoOwner);
        };
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(CreateScheduleOutcome::EndpointGone);
        }
        let held = count_for_owner(&mut tx, owner).await?;
        if usize::try_from(held).map_or(true, |held| held >= max_per_owner) {
            return Ok(CreateScheduleOutcome::LimitReached);
        }
        let created = insert_schedule(&mut tx, endpoint, schedule).await?;
        tx.commit().await.map_err(classify)?;
        Ok(CreateScheduleOutcome::Created(Box::new(created)))
    }

    pub async fn update_schedule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        id: Uuid,
        update: &ScheduleUpdate,
    ) -> Result<UpdateScheduleOutcome, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(UpdateScheduleOutcome::EndpointGone);
        }
        let Some(updated) = write_update(&mut tx, endpoint, id, update).await? else {
            return Ok(UpdateScheduleOutcome::NotFound);
        };
        tx.commit().await.map_err(classify)?;
        Ok(UpdateScheduleOutcome::Updated(Box::new(updated)))
    }

    pub async fn patch_schedule<E>(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        id: Uuid,
        plan: impl FnOnce(&ScheduleRecord) -> Result<ScheduleUpdate, E>,
    ) -> Result<PatchScheduleOutcome<E>, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(PatchScheduleOutcome::EndpointGone);
        }
        let sql = format!(
            "SELECT {COLUMNS} FROM schedules WHERE id = $1 AND endpoint_id = $2 FOR UPDATE"
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(endpoint.unpack())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(PatchScheduleOutcome::NotFound);
        };
        let existing = schedule_from_row(&row)?;
        let update = match plan(&existing) {
            Ok(update) => update,
            Err(refusal) => {
                tx.rollback().await.map_err(classify)?;
                return Ok(PatchScheduleOutcome::Refused(refusal));
            }
        };
        let Some(updated) = write_update(&mut tx, endpoint, id, &update).await? else {
            return Ok(PatchScheduleOutcome::NotFound);
        };
        tx.commit().await.map_err(classify)?;
        Ok(PatchScheduleOutcome::Updated(Box::new(updated)))
    }

    pub async fn delete_schedule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        id: Uuid,
    ) -> Result<DeleteScheduleOutcome, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteScheduleOutcome::EndpointGone);
        }
        let deleted = sqlx::query("DELETE FROM schedules WHERE id = $1 AND endpoint_id = $2")
            .bind(id)
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
        tx.commit().await.map_err(classify)?;
        Ok(if deleted == 0 {
            DeleteScheduleOutcome::NotFound
        } else {
            DeleteScheduleOutcome::Deleted
        })
    }

    pub async fn queue_schedule_run(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        id: Uuid,
        now: OffsetDateTime,
    ) -> Result<QueueRunOutcome, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(QueueRunOutcome::EndpointGone);
        }
        let queued = sqlx::query(
            "UPDATE schedules SET next_run_at = LEAST(COALESCE(next_run_at, $3), $3) \
             WHERE id = $1 AND endpoint_id = $2 AND enabled RETURNING 1",
        )
        .bind(id)
        .bind(endpoint.unpack())
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        if queued.is_some() {
            tx.commit().await.map_err(classify)?;
            return Ok(QueueRunOutcome::Queued(now));
        }
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM schedules WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(id)
        .bind(endpoint.unpack())
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        tx.rollback().await.map_err(classify)?;
        Ok(if exists {
            QueueRunOutcome::Disabled
        } else {
            QueueRunOutcome::NotFound
        })
    }

    pub async fn claim_due_schedules(
        &self,
        policy: ScheduleClaimPolicy,
    ) -> Result<Vec<ClaimedSchedule>, ScheduleStoreError> {
        let rows = sqlx::query(CLAIM)
            .bind(policy.now)
            .bind(policy.limit)
            .bind(policy.lease.as_secs_f64())
            .bind(policy.retention.as_secs_f64())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(claimed_from_row).collect()
    }

    pub async fn settle_schedule(
        &self,
        settlement: &ScheduleSettlement,
    ) -> Result<SettleOutcome, ScheduleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let enabled = settlement.paused_reason.is_none();
        let settled = sqlx::query(SETTLE)
            .bind(settlement.id)
            .bind(settlement.lease)
            .bind(settlement.last_run_at)
            .bind(settlement.consecutive_failures)
            .bind(enabled)
            .bind(settlement.paused_reason.as_deref())
            .bind(settlement.scheduled_for)
            .bind(settlement.next_run_at)
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
        let run = &settlement.run;
        sqlx::query(INSERT_RUN)
            .bind(run.id)
            .bind(settlement.id)
            .bind(run.started_at)
            .bind(run.duration_ms)
            .bind(run.outcome.as_str())
            .bind(run.status)
            .bind(run.error.as_deref())
            .bind(run.capture_id)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query(TRIM_RUNS)
            .bind(settlement.id)
            .bind(MAX_RUN_HISTORY)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(if settled == 0 {
            SettleOutcome::LeaseLost
        } else {
            SettleOutcome::Settled
        })
    }

    pub async fn consume_schedule_quota(
        &self,
        owner: Uuid,
        window_start: OffsetDateTime,
        limit: u32,
    ) -> Result<bool, ScheduleStoreError> {
        if limit == 0 {
            return Ok(false);
        }
        let admitted = sqlx::query(CONSUME_QUOTA)
            .bind(owner)
            .bind(window_start)
            .bind(i32::try_from(limit).unwrap_or(i32::MAX))
            .bind(QUOTA_RETENTION_HOURS)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        Ok(admitted.is_some())
    }

    pub async fn list_schedule_runs(
        &self,
        endpoint: &EndpointId,
        id: Uuid,
    ) -> Result<Option<Vec<ScheduleRunRecord>>, ScheduleStoreError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM schedules WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(id)
        .bind(endpoint.unpack())
        .fetch_one(self.pool())
        .await
        .map_err(classify)?;
        if !exists {
            return Ok(None);
        }
        let rows = sqlx::query(
            "SELECT id, started_at, duration_ms, outcome, status, error, capture_id \
             FROM schedule_runs WHERE schedule_id = $1 \
             ORDER BY started_at DESC, id DESC LIMIT $2",
        )
        .bind(id)
        .bind(MAX_RUN_HISTORY)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;
        rows.iter()
            .map(run_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}

async fn write_update(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    endpoint: &EndpointId,
    id: Uuid,
    update: &ScheduleUpdate,
) -> Result<Option<ScheduleRecord>, ScheduleStoreError> {
    let request = encode_request(&update.request)?;
    let sql = format!(
        "UPDATE schedules SET name = $3, cron = $4, timezone = $5, \
           request = $6::text::jsonb, enabled = $7, next_run_at = $8, \
           paused_reason = CASE WHEN $7 THEN NULL ELSE paused_reason END, \
           consecutive_failures = CASE WHEN $9 THEN 0 ELSE consecutive_failures END, \
           locked_until = CASE WHEN $10 THEN NULL ELSE locked_until END \
         WHERE id = $1 AND endpoint_id = $2 RETURNING {COLUMNS}"
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .bind(endpoint.unpack())
        .bind(update.name.as_str())
        .bind(update.cron.as_str())
        .bind(update.timezone.as_str())
        .bind(request)
        .bind(update.enabled)
        .bind(update.next_run_at)
        .bind(update.reset_failures)
        .bind(update.release_lease)
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify)?;
    row.as_ref().map(schedule_from_row).transpose()
}

pub(crate) async fn insert_schedule(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    endpoint: &EndpointId,
    schedule: &NewSchedule,
) -> Result<ScheduleRecord, ScheduleStoreError> {
    let request = encode_request(&schedule.request)?;
    let sql = format!(
        "INSERT INTO schedules \
         (id, endpoint_id, created_by, name, cron, timezone, request, enabled, next_run_at) \
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6::text::jsonb, $7, $8) \
         RETURNING {COLUMNS}"
    );
    let row = sqlx::query(&sql)
        .bind(endpoint.unpack())
        .bind(schedule.created_by)
        .bind(schedule.name.as_str())
        .bind(schedule.cron.as_str())
        .bind(schedule.timezone.as_str())
        .bind(request)
        .bind(schedule.enabled)
        .bind(schedule.next_run_at)
        .fetch_one(&mut **tx)
        .await
        .map_err(classify)?;
    schedule_from_row(&row)
}

pub(crate) async fn count_for_owner(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
) -> Result<i64, ScheduleStoreError> {
    sqlx::query_scalar(
        "SELECT count(*) FROM schedules s JOIN endpoints e ON e.id = s.endpoint_id \
         WHERE e.owner_id = $1 AND e.deleted_at IS NULL",
    )
    .bind(owner)
    .fetch_one(&mut **tx)
    .await
    .map_err(classify)
}

fn encode_request(request: &ScheduleRequest) -> Result<String, ScheduleStoreError> {
    serde_json::to_string(request)
        .map_err(|error| ScheduleStoreError::Invalid(format!("cannot encode the request: {error}")))
}

fn decode_request(text: &str, id: Uuid) -> Result<ScheduleRequest, ScheduleStoreError> {
    serde_json::from_str(text)
        .map_err(|error| ScheduleStoreError::Invalid(format!("request of schedule {id}: {error}")))
}

fn schedule_from_row(row: &PgRow) -> Result<ScheduleRecord, ScheduleStoreError> {
    let id: Uuid = row.try_get("id").map_err(classify)?;
    let request: String = row.try_get("request").map_err(classify)?;
    Ok(ScheduleRecord {
        id,
        endpoint_id: row.try_get("endpoint_id").map_err(classify)?,
        name: row.try_get("name").map_err(classify)?,
        cron: row.try_get("cron").map_err(classify)?,
        timezone: row.try_get("timezone").map_err(classify)?,
        request: decode_request(&request, id)?,
        enabled: row.try_get("enabled").map_err(classify)?,
        paused_reason: row.try_get("paused_reason").map_err(classify)?,
        next_run_at: row.try_get("next_run_at").map_err(classify)?,
        last_run_at: row.try_get("last_run_at").map_err(classify)?,
        consecutive_failures: row.try_get("consecutive_failures").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
    })
}

fn claimed_from_row(row: &PgRow) -> Result<ClaimedSchedule, ScheduleStoreError> {
    let id: Uuid = row.try_get("id").map_err(classify)?;
    let endpoint: String = row.try_get("endpoint_id").map_err(classify)?;
    let request: String = row.try_get("request").map_err(classify)?;
    let endpoint_id = EndpointId::parse(&endpoint).map_err(|error| {
        ScheduleStoreError::Invalid(format!("stored endpoint id {endpoint:?}: {error}"))
    })?;
    let scheduled_for: Option<OffsetDateTime> = row.try_get("next_run_at").map_err(classify)?;
    let lease: Option<OffsetDateTime> = row.try_get("locked_until").map_err(classify)?;
    Ok(ClaimedSchedule {
        id,
        endpoint_id,
        owner_id: row.try_get("owner_id").map_err(classify)?,
        name: row.try_get("name").map_err(classify)?,
        cron: row.try_get("cron").map_err(classify)?,
        timezone: row.try_get("timezone").map_err(classify)?,
        request: decode_request(&request, id)?,
        scheduled_for: scheduled_for.ok_or_else(|| {
            ScheduleStoreError::Invalid(format!("claimed schedule {id} has no next run"))
        })?,
        consecutive_failures: row.try_get("consecutive_failures").map_err(classify)?,
        lease: lease.ok_or_else(|| {
            ScheduleStoreError::Invalid(format!("claimed schedule {id} has no lease"))
        })?,
    })
}

fn run_from_row(row: &PgRow) -> Result<ScheduleRunRecord, ScheduleStoreError> {
    let outcome: String = row.try_get("outcome").map_err(classify)?;
    Ok(ScheduleRunRecord {
        id: row.try_get("id").map_err(classify)?,
        started_at: row.try_get("started_at").map_err(classify)?,
        duration_ms: row.try_get("duration_ms").map_err(classify)?,
        outcome: RunOutcome::parse(&outcome).ok_or_else(|| {
            ScheduleStoreError::Invalid(format!("unknown run outcome {outcome:?}"))
        })?,
        status: row.try_get("status").map_err(classify)?,
        error: row.try_get("error").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
    })
}
