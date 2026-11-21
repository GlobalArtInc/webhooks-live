use std::time::Duration;

use sqlx::Row;
use sqlx::postgres::PgRow;
use sqlx::types::Uuid;
use time::OffsetDateTime;
use whl_core::alias::Alias;
use whl_core::endpoint_id::EndpointId;

use crate::captures::{ACQUIRE_TIMEOUT, TRANSACTION_DEADLINE, is_unavailable_error};
use crate::store::Store;

pub const MAX_ALIASES_LISTED: i64 = 100;

const LOCK_USER_SQL: &str = "SELECT id FROM users WHERE id = $1 FOR UPDATE";

const LOCK_ENDPOINT_SQL: &str = "SELECT owner_id FROM endpoints \
    WHERE id = $1 AND deleted_at IS NULL \
    AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
    FOR SHARE";

const LOCK_ALIAS_SQL: &str = "SELECT endpoint_id, owner_id, created_at, \
        released_at IS NOT NULL AS released, \
        COALESCE(released_at > now() - make_interval(secs => $2), false) AS quarantined \
    FROM endpoint_aliases WHERE alias = $1 FOR UPDATE";

const COUNT_OWNED_SQL: &str = "SELECT count(*) FROM endpoint_aliases \
    WHERE owner_id = $1 AND alias <> $3 \
    AND (released_at IS NULL OR released_at > now() - make_interval(secs => $2))";

const INSERT_SQL: &str = "INSERT INTO endpoint_aliases (alias, endpoint_id, owner_id) \
    VALUES ($1, $2, $3) ON CONFLICT (alias) DO NOTHING \
    RETURNING alias, created_at";

const TAKE_SQL: &str = "UPDATE endpoint_aliases \
    SET endpoint_id = $2, owner_id = $3, created_at = now(), released_at = NULL \
    WHERE alias = $1 \
    RETURNING alias, created_at";

const RELEASE_SQL: &str = "UPDATE endpoint_aliases SET released_at = now(), endpoint_id = NULL \
    WHERE alias = $1 AND endpoint_id = $2 AND released_at IS NULL \
    RETURNING alias";

const LIST_SQL: &str = "SELECT alias, created_at FROM endpoint_aliases \
    WHERE endpoint_id = $1 AND released_at IS NULL \
    ORDER BY created_at, alias LIMIT $2";

const RESOLVE_SQL: &str = "SELECT endpoint_id FROM endpoint_aliases \
    WHERE alias = $1 AND released_at IS NULL AND endpoint_id IS NOT NULL";

const STANDING_SQL: &str = "SELECT owner_id, released_at IS NOT NULL AS released, \
        COALESCE(released_at > now() - make_interval(secs => $2), false) AS quarantined \
    FROM endpoint_aliases WHERE alias = $1";

#[derive(Debug, thiserror::Error)]
pub enum AliasStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("no database connection became free within {ACQUIRE_TIMEOUT:?}")]
    AcquireTimeout,
    #[error("the alias lookup did not finish within {TRANSACTION_DEADLINE:?}")]
    DeadlineExceeded,
    #[error("the alias query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored alias data is invalid: {0}")]
    Invalid(String),
}

impl AliasStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable(_) | Self::AcquireTimeout | Self::DeadlineExceeded
        )
    }
}

fn classify(error: sqlx::Error) -> AliasStoreError {
    if is_unavailable_error(&error) {
        AliasStoreError::Unavailable(error)
    } else {
        AliasStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasRecord {
    pub alias: Alias,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutAliasOutcome {
    Stored(AliasRecord),
    Taken,
    LimitReached,
    EndpointGone,
    NotOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAliasOutcome {
    Released,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasAvailability {
    Available,
    Taken,
}

#[derive(Debug, Clone, Copy)]
pub struct AliasClaim<'a> {
    pub endpoint: &'a EndpointId,
    pub user: Uuid,
    pub alias: &'a Alias,
    pub retention: Duration,
    pub quarantine: Duration,
    pub max_owned: u32,
}

impl Store {
    pub async fn put_alias(
        &self,
        claim: AliasClaim<'_>,
    ) -> Result<PutAliasOutcome, AliasStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let user = sqlx::query(LOCK_USER_SQL)
            .bind(claim.user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if user.is_none() {
            return Ok(PutAliasOutcome::NotOwner);
        }
        let endpoint = sqlx::query(LOCK_ENDPOINT_SQL)
            .bind(claim.endpoint.unpack())
            .bind(claim.retention.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(endpoint) = endpoint else {
            return Ok(PutAliasOutcome::EndpointGone);
        };
        let endpoint_owner: Option<Uuid> = endpoint.try_get("owner_id").map_err(classify)?;
        if endpoint_owner != Some(claim.user) {
            return Ok(PutAliasOutcome::NotOwner);
        }
        let existing = sqlx::query(LOCK_ALIAS_SQL)
            .bind(claim.alias.unpack())
            .bind(claim.quarantine.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let takeover = match &existing {
            None => false,
            Some(row) => {
                let released: bool = row.try_get("released").map_err(classify)?;
                let holder: Option<String> = row.try_get("endpoint_id").map_err(classify)?;
                if !released {
                    if holder.as_deref() == Some(claim.endpoint.unpack()) {
                        let created_at: OffsetDateTime =
                            row.try_get("created_at").map_err(classify)?;
                        return Ok(PutAliasOutcome::Stored(AliasRecord {
                            alias: claim.alias.clone(),
                            created_at,
                        }));
                    }
                    return Ok(PutAliasOutcome::Taken);
                }
                let quarantined: bool = row.try_get("quarantined").map_err(classify)?;
                let previous: Option<Uuid> = row.try_get("owner_id").map_err(classify)?;
                if quarantined && previous != Some(claim.user) {
                    return Ok(PutAliasOutcome::Taken);
                }
                true
            }
        };
        let held: i64 = sqlx::query_scalar(COUNT_OWNED_SQL)
            .bind(claim.user)
            .bind(claim.quarantine.as_secs_f64())
            .bind(claim.alias.unpack())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        if held >= i64::from(claim.max_owned) {
            return Ok(PutAliasOutcome::LimitReached);
        }
        let statement = if takeover { TAKE_SQL } else { INSERT_SQL };
        let stored = sqlx::query(statement)
            .bind(claim.alias.unpack())
            .bind(claim.endpoint.unpack())
            .bind(claim.user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(row) = stored else {
            return Ok(PutAliasOutcome::Taken);
        };
        let created_at: OffsetDateTime = row.try_get("created_at").map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(PutAliasOutcome::Stored(AliasRecord {
            alias: claim.alias.clone(),
            created_at,
        }))
    }

    pub async fn release_alias(
        &self,
        endpoint: &EndpointId,
        alias: &Alias,
    ) -> Result<ReleaseAliasOutcome, AliasStoreError> {
        let released = sqlx::query(RELEASE_SQL)
            .bind(alias.unpack())
            .bind(endpoint.unpack())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        Ok(if released.is_some() {
            ReleaseAliasOutcome::Released
        } else {
            ReleaseAliasOutcome::NotFound
        })
    }

    pub async fn list_aliases(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<AliasRecord>, AliasStoreError> {
        let rows = sqlx::query(LIST_SQL)
            .bind(endpoint.unpack())
            .bind(MAX_ALIASES_LISTED)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(record_from_row).collect()
    }

    pub async fn resolve_alias(
        &self,
        alias: &Alias,
    ) -> Result<Option<EndpointId>, AliasStoreError> {
        let mut connection = tokio::time::timeout(ACQUIRE_TIMEOUT, self.pool().acquire())
            .await
            .map_err(|_| AliasStoreError::AcquireTimeout)?
            .map_err(classify)?;
        let query = sqlx::query(RESOLVE_SQL)
            .bind(alias.unpack())
            .fetch_optional(&mut *connection);
        let row = tokio::time::timeout(TRANSACTION_DEADLINE, query)
            .await
            .map_err(|_| AliasStoreError::DeadlineExceeded)?
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: String = row.try_get("endpoint_id").map_err(classify)?;
        EndpointId::parse(&id)
            .map(Some)
            .map_err(|error| AliasStoreError::Invalid(error.to_string()))
    }

    pub async fn alias_availability(
        &self,
        alias: &Alias,
        user: Uuid,
        quarantine: Duration,
    ) -> Result<AliasAvailability, AliasStoreError> {
        let row = sqlx::query(STANDING_SQL)
            .bind(alias.unpack())
            .bind(quarantine.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(AliasAvailability::Available);
        };
        let released: bool = row.try_get("released").map_err(classify)?;
        let quarantined: bool = row.try_get("quarantined").map_err(classify)?;
        let previous: Option<Uuid> = row.try_get("owner_id").map_err(classify)?;
        Ok(if released && (!quarantined || previous == Some(user)) {
            AliasAvailability::Available
        } else {
            AliasAvailability::Taken
        })
    }
}

fn record_from_row(row: &PgRow) -> Result<AliasRecord, AliasStoreError> {
    let text: String = row.try_get("alias").map_err(classify)?;
    let alias = Alias::parse_shape(&text)
        .map_err(|error| AliasStoreError::Invalid(format!("alias {text:?} {error}")))?;
    Ok(AliasRecord {
        alias,
        created_at: row.try_get("created_at").map_err(classify)?,
    })
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt;

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    #[derive(Debug)]
    struct Coded(&'static str);

    impl fmt::Display for Coded {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("coded failure")
        }
    }

    impl std::error::Error for Coded {}

    impl DatabaseError for Coded {
        fn message(&self) -> &str {
            "coded failure"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.0))
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn database(code: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(Coded(code)))
    }

    #[test]
    fn deadlocks_timeouts_and_connection_failures_count_as_unavailable() {
        for code in [
            "40P01", "40001", "57014", "57P01", "53300", "55P03", "08006",
        ] {
            assert!(classify(database(code)).is_unavailable(), "{code}");
        }
        assert!(classify(sqlx::Error::PoolTimedOut).is_unavailable());
        assert!(classify(sqlx::Error::PoolClosed).is_unavailable());
    }

    #[test]
    fn ordinary_query_failures_stay_internal() {
        for code in ["23505", "42601", "22P02"] {
            assert!(!classify(database(code)).is_unavailable(), "{code}");
        }
        assert!(!classify(sqlx::Error::RowNotFound).is_unavailable());
    }
}
