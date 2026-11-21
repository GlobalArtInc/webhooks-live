use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row, Transaction};
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::captures::{CaptureStoreError, TRANSACTION_DEADLINE};
use crate::forward::{ForwardStoreError, TIMESTAMP_SQL, lock_live_endpoint};

pub const MAX_VARS_PER_ENDPOINT: usize = 50;
pub const MAX_VAR_VALUE_BYTES: usize = 4096;
pub const MAX_VAR_NAME_BYTES: usize = 64;

const LOAD_VARS: &str = "SELECT name, value::text AS value FROM endpoint_vars \
     WHERE endpoint_id = $1 AND name = ANY($2)";

const BUMP_COUNTER: &str = "UPDATE endpoint_vars SET value = to_jsonb((value::text)::bigint + $4::bigint), \
       updated_at = now() \
     WHERE endpoint_id = $1 AND name = $2 AND value::text ~ '^-?[0-9]{1,18}$' \
       AND EXISTS (SELECT 1 FROM endpoints e WHERE e.id = $1 AND e.deleted_at IS NULL \
         AND e.last_active_at > now() - make_interval(secs => $3)) \
     RETURNING (value::text)::bigint AS value";

const UPSERT_COUNTER: &str = "INSERT INTO endpoint_vars (endpoint_id, name, value) \
     VALUES ($1, $2, to_jsonb($3::bigint)) \
     ON CONFLICT (endpoint_id, name) DO UPDATE SET \
       value = to_jsonb((endpoint_vars.value::text)::bigint + $3::bigint), updated_at = now() \
     WHERE endpoint_vars.value::text ~ '^-?[0-9]{1,18}$' \
     RETURNING (endpoint_vars.value::text)::bigint AS value";

#[derive(Debug, thiserror::Error)]
pub enum VarStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the variable query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored variable data is invalid: {0}")]
    Invalid(String),
    #[error("cannot encode a variable: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the variables did not load within {0:?}")]
    Deadline(Duration),
}

impl VarStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_) | Self::Deadline(_))
    }
}

impl From<ForwardStoreError> for VarStoreError {
    fn from(error: ForwardStoreError) -> Self {
        match error {
            ForwardStoreError::Unavailable(source) => Self::Unavailable(source),
            ForwardStoreError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

impl From<CaptureStoreError> for VarStoreError {
    fn from(error: CaptureStoreError) -> Self {
        match error {
            CaptureStoreError::Unavailable { source } => Self::Unavailable(source),
            CaptureStoreError::AcquireTimeout => Self::Unavailable(sqlx::Error::PoolTimedOut),
            CaptureStoreError::Query { source } => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

const SQLSTATE_CHECK_VIOLATION: &str = "23514";
const SQLSTATE_UNTRANSLATABLE_CHARACTER: &str = "22P05";
const SQLSTATE_ZERO_BYTE: &str = "22021";

fn is_value_rejection(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database) = error else {
        return false;
    };
    database.code().is_some_and(|code| {
        [
            SQLSTATE_CHECK_VIOLATION,
            SQLSTATE_UNTRANSLATABLE_CHARACTER,
            SQLSTATE_ZERO_BYTE,
        ]
        .contains(&code.as_ref())
    })
}

fn classify(error: sqlx::Error) -> VarStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        VarStoreError::Unavailable(error)
    } else {
        VarStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarRecord {
    pub name: String,
    pub value: Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PutVarOutcome {
    Stored(VarRecord),
    EndpointGone,
    LimitReached,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteVarOutcome {
    Deleted,
    EndpointGone,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncrementOutcome {
    Value(i64),
    EndpointGone,
    LimitReached,
    NotACounter,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedVars {
    values: BTreeMap<String, Value>,
}

impl LoadedVars {
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.values
    }

    pub fn counter(&self, name: &str) -> Option<i64> {
        self.values.get(name).and_then(Value::as_i64)
    }
}

impl Store {
    pub async fn load_vars(
        &self,
        endpoint: &EndpointId,
        names: &[String],
        deadline: Duration,
    ) -> Result<LoadedVars, VarStoreError> {
        let work = async {
            let mut connection = self.acquire_capture_connection().await?;
            let rows = sqlx::query(LOAD_VARS)
                .bind(endpoint.unpack())
                .bind(names)
                .fetch_all(&mut *connection)
                .await
                .map_err(classify)?;
            Ok::<_, VarStoreError>(rows)
        };
        let rows = tokio::time::timeout(deadline, work)
            .await
            .map_err(|_| VarStoreError::Deadline(deadline))??;
        loaded_from_rows(&rows)
    }

    pub async fn load_rule_vars(
        &self,
        endpoint: &EndpointId,
        names: &[String],
        acquire: Duration,
        deadline: Duration,
    ) -> Result<LoadedVars, VarStoreError> {
        let mut connection = tokio::time::timeout(acquire, self.acquire_capture_connection())
            .await
            .map_err(|_| VarStoreError::Deadline(acquire))??;
        let query = sqlx::query(LOAD_VARS)
            .bind(endpoint.unpack())
            .bind(names)
            .fetch_all(&mut *connection);
        let rows = tokio::time::timeout(deadline, query)
            .await
            .map_err(|_| VarStoreError::Deadline(deadline))?
            .map_err(classify)?;
        loaded_from_rows(&rows)
    }

    pub async fn list_vars(&self, endpoint: &EndpointId) -> Result<Vec<VarRecord>, VarStoreError> {
        let sql = format!(
            "SELECT name, value::text AS value, \
             to_char(updated_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS updated_at \
             FROM endpoint_vars WHERE endpoint_id = $1 ORDER BY name"
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(record_from_row).collect()
    }

    pub async fn put_var(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &str,
        value: &Value,
    ) -> Result<PutVarOutcome, VarStoreError> {
        let encoded = serde_json::to_string(value)?;
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(PutVarOutcome::EndpointGone);
        }
        if !var_exists(&mut tx, endpoint, name).await? && at_limit(&mut tx, endpoint).await? {
            return Ok(PutVarOutcome::LimitReached);
        }
        let sql = format!(
            "INSERT INTO endpoint_vars (endpoint_id, name, value) VALUES ($1, $2, $3::text::jsonb) \
             ON CONFLICT (endpoint_id, name) DO UPDATE SET \
             value = EXCLUDED.value, updated_at = now() \
             RETURNING name, value::text AS value, \
             to_char(updated_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS updated_at"
        );
        let row = match sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(name)
            .bind(encoded)
            .fetch_one(&mut *tx)
            .await
        {
            Ok(row) => row,
            Err(error) if is_value_rejection(&error) => return Ok(PutVarOutcome::Rejected),
            Err(error) => return Err(classify(error)),
        };
        let record = record_from_row(&row)?;
        tx.commit().await.map_err(classify)?;
        Ok(PutVarOutcome::Stored(record))
    }

    pub async fn delete_var(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &str,
    ) -> Result<DeleteVarOutcome, VarStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteVarOutcome::EndpointGone);
        }
        let deleted = sqlx::query("DELETE FROM endpoint_vars WHERE endpoint_id = $1 AND name = $2")
            .bind(endpoint.unpack())
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
        tx.commit().await.map_err(classify)?;
        Ok(if deleted == 0 {
            DeleteVarOutcome::NotFound
        } else {
            DeleteVarOutcome::Deleted
        })
    }

    pub async fn increment_counter(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &str,
    ) -> Result<IncrementOutcome, VarStoreError> {
        self.add_to_counter(endpoint, retention, name, 1).await
    }

    pub async fn add_to_counter(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &str,
        by: i64,
    ) -> Result<IncrementOutcome, VarStoreError> {
        let bumped = tokio::time::timeout(
            TRANSACTION_DEADLINE,
            sqlx::query(BUMP_COUNTER)
                .bind(endpoint.unpack())
                .bind(name)
                .bind(retention.as_secs_f64())
                .bind(by)
                .fetch_optional(self.pool()),
        )
        .await
        .map_err(|_| VarStoreError::Deadline(TRANSACTION_DEADLINE))?
        .map_err(classify)?;
        if let Some(row) = bumped {
            return Ok(IncrementOutcome::Value(
                row.try_get("value").map_err(classify)?,
            ));
        }
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(IncrementOutcome::EndpointGone);
        }
        if !var_exists(&mut tx, endpoint, name).await? && at_limit(&mut tx, endpoint).await? {
            return Ok(IncrementOutcome::LimitReached);
        }
        let row = sqlx::query(UPSERT_COUNTER)
            .bind(endpoint.unpack())
            .bind(name)
            .bind(by)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        match row {
            Some(row) => Ok(IncrementOutcome::Value(
                row.try_get("value").map_err(classify)?,
            )),
            None => Ok(IncrementOutcome::NotACounter),
        }
    }
}

fn loaded_from_rows(rows: &[PgRow]) -> Result<LoadedVars, VarStoreError> {
    let mut values = BTreeMap::new();
    for row in rows {
        let name: String = row.try_get("name").map_err(classify)?;
        let text: String = row.try_get("value").map_err(classify)?;
        let value = serde_json::from_str(&text)
            .map_err(|error| VarStoreError::Invalid(format!("variable {name}: {error}")))?;
        values.insert(name, value);
    }
    Ok(LoadedVars { values })
}

async fn var_exists(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    name: &str,
) -> Result<bool, VarStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM endpoint_vars WHERE endpoint_id = $1 AND name = $2)",
    )
    .bind(endpoint.unpack())
    .bind(name)
    .fetch_one(&mut **tx)
    .await
    .map_err(classify)
}

async fn at_limit(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
) -> Result<bool, VarStoreError> {
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoint_vars WHERE endpoint_id = $1")
        .bind(endpoint.unpack())
        .fetch_one(&mut **tx)
        .await
        .map_err(classify)?;
    Ok(usize::try_from(held).map_or(true, |held| held >= MAX_VARS_PER_ENDPOINT))
}

fn record_from_row(row: &PgRow) -> Result<VarRecord, VarStoreError> {
    let name: String = row.try_get("name").map_err(classify)?;
    let text: String = row.try_get("value").map_err(classify)?;
    let value = serde_json::from_str(&text)
        .map_err(|error| VarStoreError::Invalid(format!("variable {name}: {error}")))?;
    Ok(VarRecord {
        name,
        value,
        updated_at: row.try_get("updated_at").map_err(classify)?,
    })
}
