use std::time::Duration;

use serde_json::Value;
use sqlx::Row;
use sqlx::postgres::{PgListener, PgRow};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::captures::{EVENT_CHANNEL, NOTIFY, SESSION_LIMITS};
use crate::forward::TIMESTAMP_SQL;
use crate::rule_runs::WAKE_CHANNEL;
use crate::rules::{RuleStoreError, classify};

pub const RERUNS_PER_CAPTURE: i64 = 10;
pub const MAX_LISTED_RUNS: i64 = 200;
pub const EVENT_PAYLOAD_LIMIT: usize = 7900;

const STATE_RUNNING: &str = "running";
const STATE_QUEUED: &str = "queued";
const STATE_SUCCEEDED: &str = "succeeded";
const STATE_FAILED: &str = "failed";
const REAP_ERROR: &str = "internal: the worker stopped before the run finished";

const CLAIM: &str = "WITH busy AS ( \
       SELECT endpoint_id, count(*)::int4 AS running FROM rule_runs \
       WHERE state = 'running' AND locked_until > now() GROUP BY endpoint_id), \
     ranked AS ( \
       SELECT r.id, r.next_attempt_at, \
         (row_number() OVER (PARTITION BY r.endpoint_id ORDER BY r.next_attempt_at, r.id) \
           + COALESCE(busy.running, 0))::int4 AS slot \
       FROM rule_runs r LEFT JOIN busy ON busy.endpoint_id = r.endpoint_id \
       WHERE r.attempts < $3::int4 \
         AND ((r.state = 'queued' AND r.next_attempt_at <= now()) \
           OR (r.state IN ('sync', 'running') AND r.locked_until < now()))), \
     due AS ( \
       SELECT r.id, r.state AS previous FROM rule_runs r JOIN ranked ON ranked.id = r.id \
       WHERE ranked.slot <= $2::int4 \
         AND r.attempts < $3::int4 \
         AND ((r.state = 'queued' AND r.next_attempt_at <= now()) \
           OR (r.state IN ('sync', 'running') AND r.locked_until < now())) \
         AND NOT EXISTS ( \
           SELECT 1 FROM endpoints e WHERE e.id = r.endpoint_id AND e.deleted_at IS NOT NULL) \
       ORDER BY ranked.next_attempt_at, r.id \
       LIMIT $1 \
       FOR UPDATE OF r SKIP LOCKED), \
     claimed AS ( \
       UPDATE rule_runs r SET \
         state = 'running', \
         locked_until = now() + make_interval(secs => $4::float8), \
         attempts = r.attempts + 1 \
       FROM due WHERE r.id = due.id \
       RETURNING r.id, r.capture_id, r.endpoint_id, r.trigger, r.rules_version, due.previous, \
         r.attempts, r.next_step, r.step_attempts, r.response_status, \
         to_char(r.created_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS created_at, \
         r.plan::text AS plan, r.vars::text AS vars, r.log::text AS log, \
         r.step_results::text AS step_results) \
     SELECT c.*, e.owner_id, ou.tier AS owner_tier, e.creator_net::text AS creator_net, e.title \
     FROM claimed c JOIN endpoints e ON e.id = c.endpoint_id \
     LEFT JOIN users ou ON ou.id = e.owner_id";

const SAVE: &str = "WITH prior AS ( \
       SELECT COALESCE(octet_length(plan::text), 0) + octet_length(vars::text) \
         + octet_length(log::text) + octet_length(step_results::text) AS bytes \
       FROM rule_runs WHERE id = $1), \
     changed AS ( \
       UPDATE rule_runs SET \
         next_step = $3::smallint, \
         vars = $4::text::jsonb, \
         log = $5::text::jsonb, \
         step_results = $6::text::jsonb, \
         state = $7::text, \
         step_attempts = $8::smallint, \
         locked_until = CASE WHEN $7::text = 'running' \
           THEN now() + make_interval(secs => $9::float8) END, \
         next_attempt_at = CASE WHEN $7::text = 'queued' \
           THEN now() + make_interval(secs => $10::float8) ELSE next_attempt_at END, \
         error = $11::text, \
         finished_at = CASE WHEN $7::text IN ('succeeded', 'failed') THEN now() END \
       WHERE id = $1 AND state = 'running' AND attempts = $2 \
       RETURNING COALESCE(octet_length(plan::text), 0) + octet_length(vars::text) \
         + octet_length(log::text) + octet_length(step_results::text) AS bytes) \
     SELECT (changed.bytes - prior.bytes)::integer AS delta FROM changed, prior";

const LOCK_ENDPOINT: &str = "SELECT e.id FROM endpoints e \
     WHERE e.id = (SELECT endpoint_id FROM rule_runs WHERE id = $1) FOR UPDATE OF e";

const SAVE_CAPTURE: &str = "UPDATE captures SET \
       rule_state = COALESCE($2::text, rule_state), \
       stored_bytes = GREATEST(stored_bytes + $3::integer, 0) \
     WHERE id = $1";

const SAVE_ENDPOINT: &str = "UPDATE endpoints SET \
       bytes_used = GREATEST(bytes_used + $2::bigint, 0) \
     WHERE id = (SELECT endpoint_id FROM captures WHERE id = $1)";

const REAP: &str = "WITH dead AS ( \
       UPDATE rule_runs SET state = 'failed', locked_until = NULL, finished_at = now(), \
         error = $2::text \
       WHERE id IN ( \
         SELECT id FROM rule_runs \
         WHERE state IN ('queued', 'running', 'sync') AND attempts >= $1::int4 \
           AND (locked_until IS NULL OR locked_until < now()) \
         FOR UPDATE SKIP LOCKED) \
       RETURNING id, capture_id), \
     marked AS ( \
       UPDATE captures SET rule_state = 'failed' \
       FROM dead WHERE captures.id = dead.capture_id RETURNING captures.id) \
     SELECT count(*)::bigint FROM dead";

const QUEUE_DEPTH: &str = "SELECT count(*) FROM rule_runs \
     WHERE state = 'queued' AND next_attempt_at <= now()";

const RUN_COLUMNS: &str = "id, capture_id, trigger, state, rules_version, response_status, \
       error, log::text AS log, \
       to_char(created_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS created_at, \
       to_char(finished_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS finished_at";

const LIVE_ENDPOINT: &str = "SELECT rules_version FROM endpoints \
     WHERE id = $1 AND deleted_at IS NULL \
       AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
     FOR UPDATE";

const INSERT_RERUN: &str = "INSERT INTO rule_runs \
       (id, capture_id, endpoint_id, trigger, rules_version, state, plan) \
     SELECT $1::uuid, $2::uuid, $3::text, 'rerun', $4::integer, 'queued', \
       jsonb_build_object( \
         'version', 1, \
         'rules', (SELECT COALESCE(jsonb_agg(jsonb_build_object( \
             'id', r.id, 'name', r.name, 'condition', r.condition, 'steps', r.steps) \
             ORDER BY r.position), '[]'::jsonb) \
           FROM rules r WHERE r.endpoint_id = $3::text AND r.enabled), \
         'resume', jsonb_build_object('rule', 0, 'step', 0), \
         'deferred', '[]'::jsonb, \
         'payload', 'null'::jsonb) \
     WHERE EXISTS (SELECT 1 FROM rules WHERE endpoint_id = $3::text AND enabled) \
     ON CONFLICT DO NOTHING";

const ACCOUNT_RERUN: &str = "WITH run AS ( \
       SELECT octet_length(plan::text)::integer AS bytes FROM rule_runs WHERE id = $1), \
     marked AS ( \
       UPDATE captures SET rule_state = 'running', \
         stored_bytes = stored_bytes + (SELECT bytes FROM run) \
       WHERE id = $2 RETURNING 1) \
     UPDATE endpoints SET bytes_used = bytes_used + (SELECT bytes FROM run) WHERE id = $3";

const FORWARD_RULE_JOB: &str = "INSERT INTO forward_jobs \
       (id, capture_id, rule_id, kind, target_url, method, content_type, header_allowlist, \
        append_path, state, payload) \
     SELECT $1, c.id, r.id, 'rule', r.target_url, COALESCE(r.method, c.method), r.content_type, \
       r.header_allowlist, r.append_path, 'pending', $4 \
     FROM captures c, forward_rules r \
     WHERE c.id = $2 AND c.endpoint_id = $3 AND r.id = $5 AND r.endpoint_id = $3 \
     ON CONFLICT (id) DO NOTHING";

const FORWARD_CUSTOM_JOB: &str = "INSERT INTO forward_jobs \
       (id, capture_id, rule_id, kind, target_url, method, content_type, header_allowlist, \
        append_path, state, payload) \
     SELECT $1, c.id, NULL, 'rule', $5, COALESCE($6, c.method), $7, $8, false, 'pending', $4 \
     FROM captures c WHERE c.id = $2 AND c.endpoint_id = $3 \
     ON CONFLICT (id) DO NOTHING";

const DELIVERY: &str = "INSERT INTO notification_deliveries \
       (id, channel_id, capture_id, kind, message, state) \
     SELECT $1, ch.id, c.id, 'single', $5, 'pending' \
     FROM notification_channels ch, captures c \
     WHERE ch.id = $4 AND ch.endpoint_id = $3 AND c.id = $2 AND c.endpoint_id = $3 \
     ON CONFLICT DO NOTHING";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTrigger {
    Capture,
    Rerun,
}

impl RunTrigger {
    fn parse(text: &str) -> Result<Self, RuleStoreError> {
        match text {
            "capture" => Ok(Self::Capture),
            "rerun" => Ok(Self::Rerun),
            other => Err(RuleStoreError::Invalid(format!(
                "unknown run trigger {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimedFrom {
    Sync,
    Queued,
    Running,
}

impl ClaimedFrom {
    fn parse(text: &str) -> Result<Self, RuleStoreError> {
        match text {
            "sync" => Ok(Self::Sync),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            other => Err(RuleStoreError::Invalid(format!(
                "unknown previous run state {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunClaimPolicy {
    pub limit: i64,
    pub per_endpoint: i32,
    pub max_claims: i32,
    pub lease: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedRun {
    pub id: Uuid,
    pub capture_id: Uuid,
    pub endpoint_id: String,
    pub trigger: RunTrigger,
    pub rules_version: i32,
    pub previous: ClaimedFrom,
    pub claim: i16,
    pub next_step: i32,
    pub step_attempts: i16,
    pub response_status: Option<i16>,
    pub created_at: String,
    pub plan: Value,
    pub vars: Value,
    pub log: Value,
    pub step_results: Value,
    pub owner_id: Option<Uuid>,
    pub owner_tier: Option<String>,
    pub creator_net: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalState {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Disposition {
    Continue {
        lease: Duration,
    },
    Retry {
        after: Duration,
        step_attempts: i16,
    },
    Release {
        step_attempts: i16,
    },
    Finish {
        state: FinalState,
        error: Option<String>,
        event: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunSave {
    pub run: Uuid,
    pub claim: i16,
    pub position: i32,
    pub vars: Value,
    pub log: Value,
    pub steps: Value,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    LeaseLost,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRecord {
    pub id: Uuid,
    pub capture_id: Uuid,
    pub trigger: String,
    pub state: String,
    pub rules_version: i32,
    pub response_status: Option<i16>,
    pub error: Option<String>,
    pub entries: Value,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFilter {
    pub capture: Option<Uuid>,
    pub state: Option<String>,
    pub after: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RerunOutcome {
    Created(Box<RunRecord>),
    EndpointGone,
    CaptureNotFound,
    NoRules,
    Active,
    TooMany,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardStepJob {
    pub id: Uuid,
    pub endpoint: EndpointId,
    pub capture: Uuid,
    pub rule: Option<Uuid>,
    pub target_url: Option<String>,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepDelivery {
    pub id: Uuid,
    pub endpoint: EndpointId,
    pub capture: Uuid,
    pub channel: Uuid,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Queued,
    CaptureNotFound,
    RuleNotFound,
    ChannelNotFound,
    ChannelDisabled,
}

#[derive(Debug)]
pub struct RuleWakeListener {
    inner: PgListener,
}

impl RuleWakeListener {
    pub async fn wait(&mut self) -> Result<(), RuleStoreError> {
        self.inner.recv().await.map(|_| ()).map_err(classify)
    }
}

fn run_columns() -> String {
    RUN_COLUMNS.replace("__TIMESTAMP__", TIMESTAMP_SQL)
}

fn parse_json(text: &str, what: &str) -> Result<Value, RuleStoreError> {
    serde_json::from_str(text).map_err(|error| RuleStoreError::Invalid(format!("{what}: {error}")))
}

fn claimed_from_row(row: &PgRow) -> Result<ClaimedRun, RuleStoreError> {
    let trigger: String = row.try_get("trigger").map_err(classify)?;
    let previous: String = row.try_get("previous").map_err(classify)?;
    let plan: Option<String> = row.try_get("plan").map_err(classify)?;
    let vars: String = row.try_get("vars").map_err(classify)?;
    let log: String = row.try_get("log").map_err(classify)?;
    let step_results: String = row.try_get("step_results").map_err(classify)?;
    let next_step: i16 = row.try_get("next_step").map_err(classify)?;
    Ok(ClaimedRun {
        id: row.try_get("id").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
        endpoint_id: row.try_get("endpoint_id").map_err(classify)?,
        trigger: RunTrigger::parse(&trigger)?,
        rules_version: row.try_get("rules_version").map_err(classify)?,
        previous: ClaimedFrom::parse(&previous)?,
        claim: row.try_get("attempts").map_err(classify)?,
        next_step: i32::from(next_step),
        step_attempts: row.try_get("step_attempts").map_err(classify)?,
        response_status: row.try_get("response_status").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
        plan: match plan {
            Some(text) => parse_json(&text, "the run plan")?,
            None => Value::Null,
        },
        vars: parse_json(&vars, "the run variables")?,
        log: parse_json(&log, "the run log")?,
        step_results: parse_json(&step_results, "the step results")?,
        owner_id: row.try_get("owner_id").map_err(classify)?,
        owner_tier: row.try_get("owner_tier").map_err(classify)?,
        creator_net: row.try_get("creator_net").map_err(classify)?,
        title: row.try_get("title").map_err(classify)?,
    })
}

fn record_from_row(row: &PgRow) -> Result<RunRecord, RuleStoreError> {
    let log: String = row.try_get("log").map_err(classify)?;
    Ok(RunRecord {
        id: row.try_get("id").map_err(classify)?,
        capture_id: row.try_get("capture_id").map_err(classify)?,
        trigger: row.try_get("trigger").map_err(classify)?,
        state: row.try_get("state").map_err(classify)?,
        rules_version: row.try_get("rules_version").map_err(classify)?,
        response_status: row.try_get("response_status").map_err(classify)?,
        error: row.try_get("error").map_err(classify)?,
        entries: parse_json(&log, "the run log")?,
        created_at: row.try_get("created_at").map_err(classify)?,
        finished_at: row.try_get("finished_at").map_err(classify)?,
    })
}

impl Store {
    pub async fn claim_rule_runs(
        &self,
        policy: RunClaimPolicy,
    ) -> Result<Vec<ClaimedRun>, RuleStoreError> {
        let sql = CLAIM.replace("__TIMESTAMP__", TIMESTAMP_SQL);
        let rows = sqlx::query(&sql)
            .bind(policy.limit)
            .bind(policy.per_endpoint)
            .bind(policy.max_claims)
            .bind(policy.lease.as_secs_f64())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(claimed_from_row).collect()
    }

    pub async fn save_rule_run(&self, save: &RunSave) -> Result<SaveOutcome, RuleStoreError> {
        let (state, lease, delay, step_attempts, error, event) = match &save.disposition {
            Disposition::Continue { lease } => {
                (STATE_RUNNING, lease.as_secs_f64(), 0.0, 0_i16, None, None)
            }
            Disposition::Retry {
                after,
                step_attempts,
            } => (
                STATE_QUEUED,
                0.0,
                after.as_secs_f64(),
                *step_attempts,
                None,
                None,
            ),
            Disposition::Release { step_attempts } => {
                (STATE_QUEUED, 0.0, 0.0, *step_attempts, None, None)
            }
            Disposition::Finish {
                state,
                error,
                event,
            } => (
                match state {
                    FinalState::Succeeded => STATE_SUCCEEDED,
                    FinalState::Failed => STATE_FAILED,
                },
                0.0,
                0.0,
                0_i16,
                error.as_deref(),
                event.as_deref(),
            ),
        };
        let position = i16::try_from(save.position)
            .map_err(|_| RuleStoreError::Invalid("the run position is out of range".to_owned()))?;
        let vars = serde_json::to_string(&save.vars)?;
        let log = serde_json::to_string(&save.log)?;
        let steps = serde_json::to_string(&save.steps)?;
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let locked = sqlx::query(LOCK_ENDPOINT)
            .bind(save.run)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if locked.is_none() {
            tx.rollback().await.map_err(classify)?;
            return Ok(SaveOutcome::LeaseLost);
        }
        let changed = sqlx::query(SAVE)
            .bind(save.run)
            .bind(save.claim)
            .bind(position)
            .bind(vars)
            .bind(log)
            .bind(steps)
            .bind(state)
            .bind(step_attempts)
            .bind(lease)
            .bind(delay)
            .bind(error)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(changed) = changed else {
            tx.rollback().await.map_err(classify)?;
            return Ok(SaveOutcome::LeaseLost);
        };
        let delta: i32 = changed.try_get("delta").map_err(classify)?;
        let capture_state = match &save.disposition {
            Disposition::Finish {
                state: FinalState::Succeeded,
                ..
            } => Some(STATE_SUCCEEDED),
            Disposition::Finish {
                state: FinalState::Failed,
                ..
            } => Some(STATE_FAILED),
            _ => None,
        };
        let capture: Uuid = sqlx::query_scalar("SELECT capture_id FROM rule_runs WHERE id = $1")
            .bind(save.run)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query(SAVE_CAPTURE)
            .bind(capture)
            .bind(capture_state)
            .bind(delta)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        if delta != 0 {
            sqlx::query(SAVE_ENDPOINT)
                .bind(capture)
                .bind(i64::from(delta))
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        if let Some(event) = event.filter(|event| event.len() <= EVENT_PAYLOAD_LIMIT) {
            sqlx::query(NOTIFY)
                .bind(EVENT_CHANNEL)
                .bind(event)
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        if matches!(save.disposition, Disposition::Release { .. }) {
            sqlx::query(NOTIFY)
                .bind(WAKE_CHANNEL)
                .bind("")
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(SaveOutcome::Saved)
    }

    pub async fn fail_exhausted_rule_runs(&self, max_claims: i32) -> Result<i64, RuleStoreError> {
        sqlx::query_scalar(REAP)
            .bind(max_claims)
            .bind(REAP_ERROR)
            .fetch_one(self.pool())
            .await
            .map_err(classify)
    }

    pub async fn rule_queue_depth(&self) -> Result<i64, RuleStoreError> {
        sqlx::query_scalar(QUEUE_DEPTH)
            .fetch_one(self.pool())
            .await
            .map_err(classify)
    }

    pub async fn listen_for_rule_wakeups(&self) -> Result<RuleWakeListener, RuleStoreError> {
        let mut inner = PgListener::connect_with(self.pool())
            .await
            .map_err(classify)?;
        inner.listen(WAKE_CHANNEL).await.map_err(classify)?;
        Ok(RuleWakeListener { inner })
    }

    pub async fn load_capture_body(
        &self,
        capture: Uuid,
        max_bytes: i32,
    ) -> Result<Option<Vec<u8>>, RuleStoreError> {
        sqlx::query_scalar(
            "SELECT substring(data FROM 1 FOR $2::int4) FROM capture_bodies WHERE capture_id = $1",
        )
        .bind(capture)
        .bind(max_bytes)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)
    }

    pub async fn capture_exists(
        &self,
        endpoint: &EndpointId,
        capture: Uuid,
    ) -> Result<bool, RuleStoreError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM captures WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(capture)
        .bind(endpoint.unpack())
        .fetch_one(self.pool())
        .await
        .map_err(classify)
    }

    pub async fn list_rule_runs(
        &self,
        endpoint: &EndpointId,
        filter: &RunFilter,
        limit: i64,
    ) -> Result<Vec<RunRecord>, RuleStoreError> {
        let sql = format!(
            "SELECT {} FROM rule_runs \
             WHERE endpoint_id = $1 \
               AND ($2::uuid IS NULL OR capture_id = $2) \
               AND ($3::text IS NULL OR state = $3) \
               AND ($4::uuid IS NULL OR (created_at, id) < \
                 (SELECT created_at, id FROM rule_runs WHERE id = $4)) \
             ORDER BY created_at DESC, id DESC LIMIT $5",
            run_columns()
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(filter.capture)
            .bind(filter.state.as_deref())
            .bind(filter.after)
            .bind(limit.clamp(1, MAX_LISTED_RUNS))
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(record_from_row).collect()
    }

    pub async fn create_rule_rerun(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        capture: Uuid,
        id: Uuid,
    ) -> Result<RerunOutcome, RuleStoreError> {
        if !self.rules_enabled() {
            return Ok(RerunOutcome::NoRules);
        }
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let version: Option<i32> = sqlx::query_scalar(LIVE_ENDPOINT)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(version) = version else {
            return Ok(RerunOutcome::EndpointGone);
        };
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM captures WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(capture)
        .bind(endpoint.unpack())
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if !present {
            return Ok(RerunOutcome::CaptureNotFound);
        }
        let existing: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM rule_runs WHERE capture_id = $1 AND trigger = 'rerun'",
        )
        .bind(capture)
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if existing >= RERUNS_PER_CAPTURE {
            return Ok(RerunOutcome::TooMany);
        }
        let inserted = sqlx::query(INSERT_RERUN)
            .bind(id)
            .bind(capture)
            .bind(endpoint.unpack())
            .bind(version)
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
        if inserted == 0 {
            let any_rule: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM rules WHERE endpoint_id = $1 AND enabled)",
            )
            .bind(endpoint.unpack())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
            return Ok(if any_rule {
                RerunOutcome::Active
            } else {
                RerunOutcome::NoRules
            });
        }
        sqlx::query(ACCOUNT_RERUN)
            .bind(id)
            .bind(capture)
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query(NOTIFY)
            .bind(WAKE_CHANNEL)
            .bind("")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let sql = format!("SELECT {} FROM rule_runs WHERE id = $1", run_columns());
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let record = record_from_row(&row)?;
        tx.commit().await.map_err(classify)?;
        Ok(RerunOutcome::Created(Box::new(record)))
    }

    pub async fn enqueue_step_forward(
        &self,
        job: &ForwardStepJob,
    ) -> Result<EnqueueOutcome, RuleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM captures WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(job.capture)
        .bind(job.endpoint.unpack())
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if !present {
            return Ok(EnqueueOutcome::CaptureNotFound);
        }
        let inserted = match job.rule {
            Some(rule) => {
                let rule_present: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM forward_rules WHERE id = $1 AND endpoint_id = $2)",
                )
                .bind(rule)
                .bind(job.endpoint.unpack())
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?;
                if !rule_present {
                    return Ok(EnqueueOutcome::RuleNotFound);
                }
                sqlx::query(FORWARD_RULE_JOB)
                    .bind(job.id)
                    .bind(job.capture)
                    .bind(job.endpoint.unpack())
                    .bind(job.payload.as_deref())
                    .bind(rule)
                    .execute(&mut *tx)
                    .await
                    .map_err(classify)?
                    .rows_affected()
            }
            None => sqlx::query(FORWARD_CUSTOM_JOB)
                .bind(job.id)
                .bind(job.capture)
                .bind(job.endpoint.unpack())
                .bind(job.payload.as_deref())
                .bind(job.target_url.as_deref())
                .bind(job.method.as_deref())
                .bind(job.content_type.as_deref())
                .bind(&job.header_allowlist)
                .execute(&mut *tx)
                .await
                .map_err(classify)?
                .rows_affected(),
        };
        if inserted > 0 {
            sqlx::query(NOTIFY)
                .bind(crate::forward::WAKE_CHANNEL)
                .bind("")
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(EnqueueOutcome::Queued)
    }

    pub async fn enqueue_step_delivery(
        &self,
        delivery: &StepDelivery,
    ) -> Result<EnqueueOutcome, RuleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let channel: Option<bool> = sqlx::query_scalar(
            "SELECT enabled FROM notification_channels WHERE id = $1 AND endpoint_id = $2",
        )
        .bind(delivery.channel)
        .bind(delivery.endpoint.unpack())
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        match channel {
            None => return Ok(EnqueueOutcome::ChannelNotFound),
            Some(false) => return Ok(EnqueueOutcome::ChannelDisabled),
            Some(true) => {}
        }
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM captures WHERE id = $1 AND endpoint_id = $2)",
        )
        .bind(delivery.capture)
        .bind(delivery.endpoint.unpack())
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        if !present {
            return Ok(EnqueueOutcome::CaptureNotFound);
        }
        let inserted =
            sqlx::query(DELIVERY)
                .bind(delivery.id)
                .bind(delivery.capture)
                .bind(delivery.endpoint.unpack())
                .bind(delivery.channel)
                .bind(delivery.message.as_deref().map(|text| {
                    crate::notify::truncate_chars(text, crate::notify::MAX_MESSAGE_CHARS)
                }))
                .execute(&mut *tx)
                .await
                .map_err(classify)?
                .rows_affected();
        if inserted > 0 {
            sqlx::query(NOTIFY)
                .bind(crate::notify::WAKE_CHANNEL)
                .bind("")
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(EnqueueOutcome::Queued)
    }
}
