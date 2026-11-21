use std::time::Duration;

use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::captures::{CaptureStoreError, EVENT_CHANNEL, NOTIFY, NameValue, SESSION_LIMITS};

pub const WAKE_CHANNEL: &str = "whl_rules_wake";
pub const HOP_HEADER: &str = "x-webhooks-live-hop";
pub const MAX_HOPS: u32 = 3;
pub const SYNC_LEASE_SECONDS: f64 = 30.0;
pub const PLAN_LOAD_DEADLINE: Duration = Duration::from_millis(100);
pub const SYNC_WRITE_DEADLINE: Duration = Duration::from_secs(1);

const EVENT_PAYLOAD_LIMIT: usize = 7900;
const STATE_SYNC: &str = "sync";
const STATE_QUEUED: &str = "queued";
const STATE_SUCCEEDED: &str = "succeeded";
const STATE_FAILED: &str = "failed";
const CAPTURE_RUNNING: &str = "running";

const INSERT_RUN: &str = "INSERT INTO rule_runs \
       (id, capture_id, endpoint_id, trigger, rules_version, state, locked_until, plan) \
     SELECT $1::uuid, $1::uuid, $2::text, 'capture', $3::integer, $4::text, \
       CASE WHEN $4::text = 'sync' THEN now() + make_interval(secs => $5::float8) END, \
       jsonb_build_object( \
         'version', 1, \
         'rules', (SELECT COALESCE(jsonb_agg(jsonb_build_object( \
             'id', r.id, 'name', r.name, 'condition', r.condition, 'steps', r.steps) \
             ORDER BY r.position), '[]'::jsonb) \
           FROM rules r WHERE r.endpoint_id = $2::text AND r.enabled), \
         'resume', jsonb_build_object('rule', 0, 'step', 0), \
         'deferred', '[]'::jsonb, \
         'payload', 'null'::jsonb) \
     WHERE EXISTS (SELECT 1 FROM rules WHERE endpoint_id = $2::text AND enabled) \
     ON CONFLICT DO NOTHING";

const ACCOUNT_RUN: &str = "WITH run AS ( \
       SELECT octet_length(plan::text)::integer AS bytes FROM rule_runs WHERE id = $1), \
     marked AS ( \
       UPDATE captures SET rule_state = 'running', \
         stored_bytes = stored_bytes + (SELECT bytes FROM run) \
       WHERE id = $1 RETURNING 1) \
     UPDATE endpoints SET bytes_used = bytes_used + (SELECT bytes FROM run) WHERE id = $2";

const LOAD_PLAN: &str = "SELECT plan::text AS plan FROM rule_runs WHERE id = $1";

const FINISH_RUN: &str = "WITH prior AS ( \
       SELECT COALESCE(octet_length(plan::text), 0) + octet_length(vars::text) \
         + octet_length(log::text) AS bytes \
       FROM rule_runs WHERE id = $1), \
     changed AS ( \
       UPDATE rule_runs SET \
         state = $2::text, \
         plan = CASE WHEN $3::boolean THEN plan ELSE $4::text::jsonb END, \
         vars = $5::text::jsonb, \
         log = $6::text::jsonb, \
         next_step = 0, \
         response_status = $7::smallint, \
         error = $8::text, \
         locked_until = NULL, \
         next_attempt_at = now(), \
         finished_at = CASE WHEN $2::text IN ('succeeded', 'failed') THEN now() END \
       WHERE id = $1 AND state = 'sync' \
       RETURNING COALESCE(octet_length(plan::text), 0) + octet_length(vars::text) \
         + octet_length(log::text) AS bytes) \
     SELECT (changed.bytes - prior.bytes)::integer AS delta FROM changed, prior";

const FINISH_CAPTURE: &str = "UPDATE captures SET \
       rule_state = $2::text, \
       response_status = COALESCE($3::smallint, response_status), \
       stored_bytes = GREATEST(stored_bytes + $4::integer, 0) \
     WHERE id = $1";

const LOCK_ENDPOINT: &str = "SELECT e.id FROM endpoints e \
     WHERE e.id = (SELECT endpoint_id FROM captures WHERE id = $1) FOR UPDATE OF e";

const FINISH_ENDPOINT: &str = "UPDATE endpoints SET \
       bytes_used = GREATEST(bytes_used + $2::bigint, 0) \
     WHERE id = (SELECT endpoint_id FROM captures WHERE id = $1)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStart {
    Sync,
    Queued,
}

impl RunStart {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sync => STATE_SYNC,
            Self::Queued => STATE_QUEUED,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleRunSeed {
    pub rules_version: i32,
    pub start: RunStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Queued,
    Succeeded,
    Failed,
}

impl SyncState {
    fn run_state(self) -> &'static str {
        match self {
            Self::Queued => STATE_QUEUED,
            Self::Succeeded => STATE_SUCCEEDED,
            Self::Failed => STATE_FAILED,
        }
    }

    fn capture_state(self) -> &'static str {
        match self {
            Self::Queued => CAPTURE_RUNNING,
            Self::Succeeded => STATE_SUCCEEDED,
            Self::Failed => STATE_FAILED,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanWrite {
    Keep,
    Clear,
    Set(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncResult {
    pub state: SyncState,
    pub plan: PlanWrite,
    pub vars: Value,
    pub log: Value,
    pub response_status: Option<u16>,
    pub capture_status: Option<u16>,
    pub error: Option<String>,
    pub event: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishOutcome {
    Written,
    LeaseLost,
}

fn hop_from<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> u32 {
    pairs
        .filter(|(name, _)| name.eq_ignore_ascii_case(HOP_HEADER))
        .map(|(_, value)| value.trim().parse::<u32>().unwrap_or(MAX_HOPS))
        .max()
        .unwrap_or(0)
}

pub fn incoming_hop(headers: &[NameValue]) -> u32 {
    hop_from(
        headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str())),
    )
}

pub fn detail_hop(headers: &[crate::capture_queries::NameValue]) -> u32 {
    hop_from(
        headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str())),
    )
}

pub(crate) async fn insert_capture_run(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    capture: Uuid,
    rules_version: i32,
    start: RunStart,
) -> Result<Option<RuleRunSeed>, sqlx::Error> {
    let inserted = sqlx::query(INSERT_RUN)
        .bind(capture)
        .bind(endpoint.unpack())
        .bind(rules_version)
        .bind(start.as_str())
        .bind(SYNC_LEASE_SECONDS)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if inserted == 0 {
        return Ok(None);
    }
    sqlx::query(ACCOUNT_RUN)
        .bind(capture)
        .bind(endpoint.unpack())
        .execute(&mut **tx)
        .await?;
    if start == RunStart::Queued {
        sqlx::query(NOTIFY)
            .bind(WAKE_CHANNEL)
            .bind("")
            .execute(&mut **tx)
            .await?;
    }
    Ok(Some(RuleRunSeed {
        rules_version,
        start,
    }))
}

impl Store {
    pub async fn load_run_plan(
        &self,
        capture: &CaptureId,
        deadline: Duration,
    ) -> Result<Option<Value>, CaptureStoreError> {
        let load = async {
            let mut connection = self.acquire_capture_connection().await?;
            sqlx::query(LOAD_PLAN)
                .bind(capture.unpack())
                .fetch_optional(&mut *connection)
                .await
                .map_err(CaptureStoreError::classify)
        };
        let row = tokio::time::timeout(deadline, load)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)??;
        let Some(row) = row else {
            return Ok(None);
        };
        let text: Option<String> = row.try_get("plan").map_err(CaptureStoreError::classify)?;
        text.map(|text| {
            serde_json::from_str(&text).map_err(|error| {
                CaptureStoreError::Corrupt(format!("the stored rule plan: {error}"))
            })
        })
        .transpose()
    }

    pub async fn finish_sync_run(
        &self,
        capture: &CaptureId,
        result: &SyncResult,
        deadline: Duration,
    ) -> Result<FinishOutcome, CaptureStoreError> {
        let work = self.write_sync_result(capture, result);
        tokio::time::timeout(deadline, work)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
    }

    async fn write_sync_result(
        &self,
        capture: &CaptureId,
        result: &SyncResult,
    ) -> Result<FinishOutcome, CaptureStoreError> {
        let (keep_plan, plan) = match &result.plan {
            PlanWrite::Keep => (true, None),
            PlanWrite::Clear => (false, None),
            PlanWrite::Set(plan) => (false, Some(serde_json::to_string(plan)?)),
        };
        let vars = serde_json::to_string(&result.vars)?;
        let log = serde_json::to_string(&result.log)?;
        let status = result
            .response_status
            .map(i16::try_from)
            .transpose()
            .map_err(|_| CaptureStoreError::OutOfRange("response status"))?;
        let capture_status = result
            .capture_status
            .map(i16::try_from)
            .transpose()
            .map_err(|_| CaptureStoreError::OutOfRange("response status"))?;
        let mut tx = self.begin_capture_transaction().await?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let locked = sqlx::query(LOCK_ENDPOINT)
            .bind(capture.unpack())
            .fetch_optional(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        if locked.is_none() {
            tx.rollback().await.map_err(CaptureStoreError::classify)?;
            return Ok(FinishOutcome::LeaseLost);
        }
        let changed = sqlx::query(FINISH_RUN)
            .bind(capture.unpack())
            .bind(result.state.run_state())
            .bind(keep_plan)
            .bind(plan)
            .bind(vars)
            .bind(log)
            .bind(status)
            .bind(result.error.as_deref())
            .fetch_optional(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let Some(changed) = changed else {
            tx.rollback().await.map_err(CaptureStoreError::classify)?;
            return Ok(FinishOutcome::LeaseLost);
        };
        let delta: i32 = changed
            .try_get("delta")
            .map_err(CaptureStoreError::classify)?;
        sqlx::query(FINISH_CAPTURE)
            .bind(capture.unpack())
            .bind(result.state.capture_state())
            .bind(capture_status)
            .bind(delta)
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        if delta != 0 {
            sqlx::query(FINISH_ENDPOINT)
                .bind(capture.unpack())
                .bind(i64::from(delta))
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)?;
        }
        if let Some(event) = result
            .event
            .as_deref()
            .filter(|event| event.len() <= EVENT_PAYLOAD_LIMIT)
        {
            sqlx::query(NOTIFY)
                .bind(EVENT_CHANNEL)
                .bind(event)
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)?;
        }
        if result.state == SyncState::Queued {
            sqlx::query(NOTIFY)
                .bind(WAKE_CHANNEL)
                .bind("")
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)?;
        }
        tx.commit().await.map_err(CaptureStoreError::classify)?;
        Ok(FinishOutcome::Written)
    }
}
