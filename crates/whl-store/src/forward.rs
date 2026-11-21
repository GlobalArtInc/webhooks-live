mod jobs;
mod queue;
mod rules;

use sqlx::{Postgres, Transaction};
use whl_core::endpoint_id::EndpointId;

use crate::captures::EVENT_CHANNEL;

pub use jobs::{
    CustomTarget, ForwardAttempt, ForwardJob, JobKind, JobState, ManualJobOutcome, ManualTarget,
};
pub use queue::{
    AttemptRecord, ClaimPolicy, ClaimedJob, JobCapture, RecordOutcome, Settlement, WAKE_CHANNEL,
    WakeListener,
};
pub use rules::{
    CreateRuleOutcome, DeleteRuleOutcome, ForwardRule, ForwardRulePatch, MAX_RULES_PER_ENDPOINT,
    NewForwardRule, UpdateRuleOutcome,
};

pub(crate) use queue::insert_rule_jobs;

pub(crate) const TIMESTAMP_SQL: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

#[derive(Debug, thiserror::Error)]
pub enum ForwardStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the forward query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored forward data is invalid: {0}")]
    Invalid(String),
    #[error("cannot encode the forward notification: {0}")]
    Encode(#[from] serde_json::Error),
}

impl ForwardStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

pub(crate) fn classify(error: sqlx::Error) -> ForwardStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        ForwardStoreError::Unavailable(error)
    } else {
        ForwardStoreError::Query(error)
    }
}

pub(crate) async fn notify_endpoint_updated(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
) -> Result<(), ForwardStoreError> {
    let payload = serde_json::json!({ "t": "endpoint_updated", "e": endpoint.unpack() });
    notify_event(tx, &payload).await
}

pub(crate) async fn notify_event(
    tx: &mut Transaction<'_, Postgres>,
    payload: &serde_json::Value,
) -> Result<(), ForwardStoreError> {
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(EVENT_CHANNEL)
        .bind(payload.to_string())
        .execute(&mut **tx)
        .await
        .map_err(classify)?;
    Ok(())
}

pub(crate) async fn lock_live_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    retention: std::time::Duration,
) -> Result<bool, ForwardStoreError> {
    let row = sqlx::query(
        "UPDATE endpoints SET last_active_at = now() \
         WHERE id = $1 AND deleted_at IS NULL \
         AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
         RETURNING id",
    )
    .bind(endpoint.unpack())
    .bind(retention.as_secs_f64())
    .fetch_optional(&mut **tx)
    .await
    .map_err(classify)?;
    Ok(row.is_some())
}
