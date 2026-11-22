use std::time::Duration;

use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_rules::{Rule, response_step_kinds};

use crate::Store;
use crate::captures::EVENT_CHANNEL;
use crate::forward::TIMESTAMP_SQL;

#[derive(Debug, thiserror::Error)]
pub enum RuleStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the rule query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored rule data is invalid: {0}")]
    Invalid(String),
    #[error("cannot encode the rule: {0}")]
    Encode(#[from] serde_json::Error),
}

impl RuleStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

pub(crate) fn classify(error: sqlx::Error) -> RuleStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        RuleStoreError::Unavailable(error)
    } else {
        RuleStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleRecord {
    pub id: Uuid,
    pub position: i16,
    pub name: String,
    pub enabled: bool,
    pub condition: Value,
    pub steps: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleSet {
    pub version: i32,
    pub rules: Vec<RuleRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleChange {
    Create(Rule),
    Replace { id: Uuid, rule: Rule },
    Remove(Uuid),
    Reorder(Vec<Uuid>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriteOutcome {
    Written(RuleSet),
    EndpointGone,
    Stale,
    RuleNotFound,
    OrderMismatch,
}

fn columns() -> String {
    format!(
        "id, position, name, enabled, condition::text AS condition, steps::text AS steps, \
         to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at, \
         to_char(updated_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS updated_at"
    )
}

impl Store {
    pub async fn list_rules(&self, endpoint: &EndpointId) -> Result<RuleSet, RuleStoreError> {
        let mut conn = self.pool().acquire().await.map_err(classify)?;
        read_set(&mut conn, endpoint).await
    }

    pub async fn write_rules(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        expected_version: i32,
        change: &RuleChange,
    ) -> Result<WriteOutcome, RuleStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(WriteOutcome::EndpointGone);
        }
        if current_version(&mut tx, endpoint).await? != Some(expected_version) {
            return Ok(WriteOutcome::Stale);
        }
        if let Some(outcome) = apply(&mut tx, endpoint, change).await? {
            return Ok(outcome);
        }
        refresh_flags(&mut tx, endpoint).await?;
        notify_updated(&mut tx, endpoint).await?;
        let set = read_set(&mut tx, endpoint).await?;
        tx.commit().await.map_err(classify)?;
        Ok(WriteOutcome::Written(set))
    }
}

async fn lock_live_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    retention: Duration,
) -> Result<bool, RuleStoreError> {
    let row = sqlx::query(
        "UPDATE endpoints SET last_active_at = now() \
         WHERE id = $1 AND deleted_at IS NULL \
         AND last_active_at > now() - make_interval(secs => $2) \
         RETURNING id",
    )
    .bind(endpoint.unpack())
    .bind(retention.as_secs_f64())
    .fetch_optional(&mut **tx)
    .await
    .map_err(classify)?;
    Ok(row.is_some())
}

async fn current_version(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<Option<i32>, RuleStoreError> {
    sqlx::query_scalar("SELECT rules_version FROM endpoints WHERE id = $1")
        .bind(endpoint.unpack())
        .fetch_optional(&mut *conn)
        .await
        .map_err(classify)
}

pub(crate) async fn read_rule_records(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<Vec<RuleRecord>, RuleStoreError> {
    Ok(read_set(conn, endpoint).await?.rules)
}

async fn read_set(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<RuleSet, RuleStoreError> {
    let version = current_version(conn, endpoint).await?;
    let sql = format!(
        "SELECT {} FROM rules WHERE endpoint_id = $1 ORDER BY position",
        columns()
    );
    let rows = sqlx::query(&sql)
        .bind(endpoint.unpack())
        .fetch_all(&mut *conn)
        .await
        .map_err(classify)?;
    let rules = rows
        .iter()
        .map(record_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RuleSet {
        version: version.unwrap_or(0),
        rules,
    })
}

async fn apply(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
    change: &RuleChange,
) -> Result<Option<WriteOutcome>, RuleStoreError> {
    match change {
        RuleChange::Create(rule) => {
            insert(conn, endpoint, rule).await?;
            Ok(None)
        }
        RuleChange::Replace { id, rule } => {
            let result = sqlx::query(
                "UPDATE rules SET name = $3, enabled = $4, condition = $5::text::jsonb, \
                 steps = $6::text::jsonb, updated_at = now() \
                 WHERE id = $1 AND endpoint_id = $2",
            )
            .bind(id)
            .bind(endpoint.unpack())
            .bind(rule.name.as_str())
            .bind(rule.enabled)
            .bind(serde_json::to_string(&rule.condition)?)
            .bind(serde_json::to_string(&rule.steps)?)
            .execute(&mut *conn)
            .await
            .map_err(classify)?;
            Ok((result.rows_affected() == 0).then_some(WriteOutcome::RuleNotFound))
        }
        RuleChange::Remove(id) => {
            let result = sqlx::query("DELETE FROM rules WHERE id = $1 AND endpoint_id = $2")
                .bind(id)
                .bind(endpoint.unpack())
                .execute(&mut *conn)
                .await
                .map_err(classify)?;
            Ok((result.rows_affected() == 0).then_some(WriteOutcome::RuleNotFound))
        }
        RuleChange::Reorder(ids) => {
            let current: Vec<Uuid> =
                sqlx::query_scalar("SELECT id FROM rules WHERE endpoint_id = $1")
                    .bind(endpoint.unpack())
                    .fetch_all(&mut *conn)
                    .await
                    .map_err(classify)?;
            if !same_members(current, ids) {
                return Ok(Some(WriteOutcome::OrderMismatch));
            }
            sqlx::query(
                "UPDATE rules AS r SET position = o.pos::smallint \
                 FROM unnest($2::uuid[]) WITH ORDINALITY AS o(id, pos) \
                 WHERE r.id = o.id AND r.endpoint_id = $1",
            )
            .bind(endpoint.unpack())
            .bind(ids.as_slice())
            .execute(&mut *conn)
            .await
            .map_err(classify)?;
            Ok(None)
        }
    }
}

fn same_members(mut current: Vec<Uuid>, ids: &[Uuid]) -> bool {
    let mut wanted = ids.to_vec();
    current.sort();
    wanted.sort();
    current == wanted
}

pub(crate) async fn insert(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
    rule: &Rule,
) -> Result<(), RuleStoreError> {
    sqlx::query(
        "INSERT INTO rules (id, endpoint_id, position, name, enabled, condition, steps) \
         VALUES (gen_random_uuid(), $1, \
         (SELECT COALESCE(MAX(position), 0) + 1 FROM rules WHERE endpoint_id = $1), \
         $2, $3, $4::text::jsonb, $5::text::jsonb)",
    )
    .bind(endpoint.unpack())
    .bind(rule.name.as_str())
    .bind(rule.enabled)
    .bind(serde_json::to_string(&rule.condition)?)
    .bind(serde_json::to_string(&rule.steps)?)
    .execute(&mut *conn)
    .await
    .map_err(classify)?;
    Ok(())
}

pub(crate) async fn refresh_flags(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<(), RuleStoreError> {
    sqlx::query(
        "UPDATE endpoints SET rules_version = rules_version + 1, \
         rule_count = (SELECT count(*) FROM rules WHERE endpoint_id = $1)::smallint, \
         rules_sync = EXISTS (SELECT 1 FROM rules AS r, jsonb_array_elements(r.steps) AS step \
         WHERE r.endpoint_id = $1 AND step->>'kind' = ANY($2)) \
         WHERE id = $1",
    )
    .bind(endpoint.unpack())
    .bind(response_step_kinds())
    .execute(&mut *conn)
    .await
    .map_err(classify)?;
    Ok(())
}

async fn notify_updated(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<(), RuleStoreError> {
    let payload = serde_json::json!({ "t": "endpoint_updated", "e": endpoint.unpack() });
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(EVENT_CHANNEL)
        .bind(payload.to_string())
        .execute(&mut *conn)
        .await
        .map_err(classify)?;
    Ok(())
}

fn record_from_row(row: &PgRow) -> Result<RuleRecord, RuleStoreError> {
    let condition: String = row.try_get("condition").map_err(classify)?;
    let steps: String = row.try_get("steps").map_err(classify)?;
    Ok(RuleRecord {
        id: row.try_get("id").map_err(classify)?,
        position: row.try_get("position").map_err(classify)?,
        name: row.try_get("name").map_err(classify)?,
        enabled: row.try_get("enabled").map_err(classify)?,
        condition: parse_json("condition", &condition)?,
        steps: parse_json("steps", &steps)?,
        created_at: row.try_get("created_at").map_err(classify)?,
        updated_at: row.try_get("updated_at").map_err(classify)?,
    })
}

fn parse_json(column: &str, text: &str) -> Result<Value, RuleStoreError> {
    serde_json::from_str(text)
        .map_err(|error| RuleStoreError::Invalid(format!("stored {column}: {error}")))
}
