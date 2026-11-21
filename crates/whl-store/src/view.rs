use std::fmt;
use std::time::Duration;

use sqlx::{PgConnection, Row};
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;

use sqlx::types::Uuid;

use crate::captures::EVENT_CHANNEL;
use crate::endpoints::{COLUMNS, EndpointError, EndpointRecord, record_from_row};
use crate::store::Store;

pub const MAX_GRANTS_PER_ENDPOINT: i64 = 50;
pub const GRANT_PURGE_BATCH: i64 = 1000;

const LIVE_ANONYMOUS: &str = "e.id = $1 AND e.deleted_at IS NULL AND e.owner_id IS NULL \
    AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $2)))";

const STANDING_SQL: &str = "WITH touched AS ( \
        UPDATE endpoint_grants SET last_used_at = now() \
        WHERE token_hash = $3 AND endpoint_id = $1 AND expires_at > now() \
            AND last_used_at < now() - make_interval(secs => 60) \
        RETURNING token_hash \
    ) \
    SELECT (e.view_password_hash IS NOT NULL) AS protected, \
        ($3::bytea IS NOT NULL AND EXISTS ( \
            SELECT 1 FROM endpoint_grants g \
            WHERE g.endpoint_id = e.id AND g.token_hash = $3 AND g.expires_at > now() \
        )) AS granted \
    FROM endpoints e WHERE ";

const HASH_SQL: &str =
    "SELECT e.view_password_hash FROM endpoints e WHERE e.view_password_hash IS NOT NULL AND ";

const ISSUE_SQL: &str = "WITH locked AS ( \
        SELECT e.id FROM endpoints e WHERE e.view_password_hash = $3 AND ";

const ISSUE_TAIL_SQL: &str = " FOR SHARE OF e) \
    INSERT INTO endpoint_grants (token_hash, endpoint_id, expires_at) \
    SELECT $4, locked.id, now() + make_interval(secs => $5) FROM locked \
    RETURNING expires_at";

const TRIM_SQL: &str = "DELETE FROM endpoint_grants WHERE endpoint_id = $1 AND ( \
        expires_at <= now() OR token_hash IN ( \
            SELECT token_hash FROM endpoint_grants WHERE endpoint_id = $1 \
            ORDER BY last_used_at DESC, created_at DESC, token_hash OFFSET $2 \
        ))";

const SET_HASH_SQL: &str = "UPDATE endpoints SET view_password_hash = $2 WHERE id = $1";

const REVOKE_SQL: &str = "DELETE FROM endpoint_grants WHERE endpoint_id = $1";

const PURGE_SQL: &str = "DELETE FROM endpoint_grants WHERE token_hash IN ( \
        SELECT token_hash FROM endpoint_grants WHERE expires_at <= now() LIMIT $1)";

const NOTIFY_SQL: &str = "SELECT pg_notify($1, $2)";

const SNAPSHOT_TAIL_SQL: &str = " FROM endpoints WHERE id = $1 AND deleted_at IS NULL \
    AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))";

#[derive(Debug, Clone)]
pub struct EndpointSnapshot {
    pub record: EndpointRecord,
    pub owner: Option<Uuid>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ViewPasswordHash(String);

impl ViewPasswordHash {
    pub fn new(phc: String) -> Self {
        Self(phc)
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ViewPasswordHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ViewPasswordHash(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPasswordChange<'a> {
    Keep,
    Set(&'a ViewPasswordHash),
    Remove,
}

impl ViewPasswordChange<'_> {
    pub fn is_requested(&self) -> bool {
        !matches!(self, Self::Keep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewStanding {
    Open,
    Locked,
    Granted,
}

impl ViewStanding {
    pub fn is_protected(self) -> bool {
        !matches!(self, Self::Open)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ViewStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the view password query failed: {0}")]
    Query(#[source] sqlx::Error),
}

impl ViewStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl From<ViewStoreError> for EndpointError {
    fn from(error: ViewStoreError) -> Self {
        match error {
            ViewStoreError::Unavailable(source) => Self::Unavailable(source),
            ViewStoreError::Query(source) => Self::Query(source),
        }
    }
}

pub(crate) fn classify(error: sqlx::Error) -> ViewStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        ViewStoreError::Unavailable(error)
    } else {
        ViewStoreError::Query(error)
    }
}

impl Store {
    pub async fn view_standing(
        &self,
        endpoint: &EndpointId,
        grant_hash: Option<&[u8]>,
        retention: Duration,
    ) -> Result<ViewStanding, ViewStoreError> {
        let sql = format!("{STANDING_SQL}{LIVE_ANONYMOUS}");
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .bind(grant_hash)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(ViewStanding::Open);
        };
        let protected: bool = row.try_get("protected").map_err(ViewStoreError::Query)?;
        let granted: bool = row.try_get("granted").map_err(ViewStoreError::Query)?;
        Ok(match (protected, granted) {
            (false, _) => ViewStanding::Open,
            (true, false) => ViewStanding::Locked,
            (true, true) => ViewStanding::Granted,
        })
    }

    pub async fn view_password_hash(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<ViewPasswordHash>, ViewStoreError> {
        let sql = format!("{HASH_SQL}{LIVE_ANONYMOUS}");
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.map(|row| {
            row.try_get::<String, _>("view_password_hash")
                .map(ViewPasswordHash::new)
                .map_err(ViewStoreError::Query)
        })
        .transpose()
    }

    pub async fn issue_view_grant(
        &self,
        endpoint: &EndpointId,
        expected: &ViewPasswordHash,
        grant_hash: &[u8],
        lifetime: Duration,
        retention: Duration,
    ) -> Result<Option<OffsetDateTime>, ViewStoreError> {
        let sql = format!("{ISSUE_SQL}{LIVE_ANONYMOUS}{ISSUE_TAIL_SQL}");
        let mut transaction = self.pool().begin().await.map_err(classify)?;
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .bind(expected.unpack())
            .bind(grant_hash)
            .bind(lifetime.as_secs_f64())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let expires_at: OffsetDateTime =
            row.try_get("expires_at").map_err(ViewStoreError::Query)?;
        sqlx::query(TRIM_SQL)
            .bind(endpoint.unpack())
            .bind(MAX_GRANTS_PER_ENDPOINT)
            .execute(&mut *transaction)
            .await
            .map_err(classify)?;
        transaction.commit().await.map_err(classify)?;
        Ok(Some(expires_at))
    }

    pub async fn endpoint_snapshot(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<EndpointSnapshot>, EndpointError> {
        let sql = format!("SELECT {COLUMNS}, owner_id{SNAPSHOT_TAIL_SQL}");
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(crate::endpoints::classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let record = record_from_row(&row, retention)?;
        let owner: Option<Uuid> = row.try_get("owner_id").map_err(EndpointError::Query)?;
        Ok(Some(EndpointSnapshot { record, owner }))
    }

    pub async fn delete_expired_view_grants(&self) -> Result<u64, ViewStoreError> {
        let outcome = sqlx::query(PURGE_SQL)
            .bind(GRANT_PURGE_BATCH)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected())
    }
}

pub(crate) async fn apply_view_password(
    connection: &mut PgConnection,
    endpoint: &EndpointId,
    change: ViewPasswordChange<'_>,
) -> Result<(), ViewStoreError> {
    let hash = match change {
        ViewPasswordChange::Keep => return Ok(()),
        ViewPasswordChange::Set(hash) => Some(hash.unpack()),
        ViewPasswordChange::Remove => None,
    };
    sqlx::query(SET_HASH_SQL)
        .bind(endpoint.unpack())
        .bind(hash)
        .execute(&mut *connection)
        .await
        .map_err(classify)?;
    revoke_view_grants(connection, endpoint).await?;
    notify_access_changed(connection, endpoint).await
}

pub(crate) async fn notify_access_changed(
    connection: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<(), ViewStoreError> {
    let payload = serde_json::json!({ "t": "access_changed", "e": endpoint.unpack() });
    sqlx::query(NOTIFY_SQL)
        .bind(EVENT_CHANNEL)
        .bind(payload.to_string())
        .execute(&mut *connection)
        .await
        .map_err(classify)?;
    Ok(())
}

pub(crate) async fn revoke_view_grants(
    connection: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<(), ViewStoreError> {
    sqlx::query(REVOKE_SQL)
        .bind(endpoint.unpack())
        .execute(&mut *connection)
        .await
        .map_err(classify)?;
    Ok(())
}
