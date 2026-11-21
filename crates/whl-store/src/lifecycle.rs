use std::time::Duration;

use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction};
use whl_core::creator_net::CreatorNet;

use crate::endpoints::EndpointError;

pub const MIN_RETENTION_SECONDS: i32 = 3600;
pub const MIN_CAPTURE_TTL_SECONDS: i32 = 60;

pub(crate) const LOCK_OWNER_SQL: &str = "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE";

const LOCK_NETWORK_SQL: &str = "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))";

const COUNT_NEVER_EXPIRES_SQL: &str = "SELECT count(*) FROM endpoints \
    WHERE owner_id = $1 AND never_expires AND deleted_at IS NULL";

const COUNT_OWNED_ENDPOINTS_SQL: &str = "SELECT count(*) FROM endpoints \
    WHERE owner_id = $1 AND deleted_at IS NULL \
      AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))";

const COUNT_NETWORK_ENDPOINTS_SQL: &str = "SELECT count(*) FROM endpoints \
    WHERE owner_id IS NULL AND creator_net = $1::text::cidr AND deleted_at IS NULL \
      AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))";

const OWNER_BYTES_SQL: &str = "SELECT COALESCE(sum(bytes_used), 0)::bigint FROM endpoints \
    WHERE owner_id = $1 AND deleted_at IS NULL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lifecycle {
    pub retention_seconds: Option<i32>,
    pub never_expires: bool,
    pub max_captures: Option<i32>,
    pub capture_ttl_seconds: Option<i32>,
}

impl Lifecycle {
    pub fn effective_retention(&self, default: Duration) -> Option<Duration> {
        if self.never_expires {
            return None;
        }
        Some(
            self.retention_seconds
                .and_then(|seconds| u64::try_from(seconds).ok())
                .map_or(default, Duration::from_secs),
        )
    }

    pub fn effective_max_captures(&self, default: u32) -> u32 {
        self.max_captures
            .and_then(|max| u32::try_from(max).ok())
            .unwrap_or(default)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleLimits {
    pub never_expire_max: u32,
    pub endpoints_max: Option<u32>,
    pub storage_bytes_max: Option<u64>,
}

impl LifecycleLimits {
    pub const NONE: Self = Self {
        never_expire_max: 0,
        endpoints_max: None,
        storage_bytes_max: None,
    };
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    #[error("the owner already has the maximum of {max} never-expire endpoints")]
    NeverExpireLimit { max: u32 },
    #[error("an endpoint without an owner cannot be set to never expire")]
    NeverExpireNeedsOwner,
    #[error("the maximum of {max} live endpoints is reached")]
    EndpointLimit { max: u32 },
    #[error("the owner stores more than the quota of {max} bytes")]
    StorageQuota { max: u64 },
}

pub(crate) async fn admit_endpoint_count(
    tx: &mut Transaction<'_, Postgres>,
    owner: Option<Uuid>,
    network: Option<&CreatorNet>,
    max: u32,
    retention: Duration,
) -> Result<(), LifecycleError> {
    let held: i64 = match (owner, network) {
        (Some(owner), _) => {
            sqlx::query(LOCK_OWNER_SQL)
                .bind(owner)
                .fetch_optional(&mut **tx)
                .await
                .map_err(crate::endpoints::classify)?;
            sqlx::query_scalar(COUNT_OWNED_ENDPOINTS_SQL)
                .bind(owner)
                .bind(retention.as_secs_f64())
                .fetch_one(&mut **tx)
                .await
                .map_err(crate::endpoints::classify)?
        }
        (None, Some(network)) => {
            let net = network.to_string();
            sqlx::query(LOCK_NETWORK_SQL)
                .bind(format!("endpoints:{net}"))
                .execute(&mut **tx)
                .await
                .map_err(crate::endpoints::classify)?;
            sqlx::query_scalar(COUNT_NETWORK_ENDPOINTS_SQL)
                .bind(net)
                .bind(retention.as_secs_f64())
                .fetch_one(&mut **tx)
                .await
                .map_err(crate::endpoints::classify)?
        }
        (None, None) => return Ok(()),
    };
    if held >= i64::from(max) {
        return Err(LifecycleError::EndpointLimit { max });
    }
    Ok(())
}

pub(crate) async fn admit_never_expires(
    tx: &mut Transaction<'_, Postgres>,
    owner: Option<Uuid>,
    limits: LifecycleLimits,
) -> Result<(), LifecycleError> {
    let Some(owner) = owner else {
        return Err(LifecycleError::NeverExpireNeedsOwner);
    };
    sqlx::query(LOCK_OWNER_SQL)
        .bind(owner)
        .fetch_optional(&mut **tx)
        .await
        .map_err(crate::endpoints::classify)?;
    let held: i64 = sqlx::query_scalar(COUNT_NEVER_EXPIRES_SQL)
        .bind(owner)
        .fetch_one(&mut **tx)
        .await
        .map_err(crate::endpoints::classify)?;
    if held >= i64::from(limits.never_expire_max) {
        return Err(LifecycleError::NeverExpireLimit {
            max: limits.never_expire_max,
        });
    }
    if let Some(max) = limits.storage_bytes_max {
        let used: i64 = sqlx::query_scalar(OWNER_BYTES_SQL)
            .bind(owner)
            .fetch_one(&mut **tx)
            .await
            .map_err(crate::endpoints::classify)?;
        if u64::try_from(used).is_ok_and(|used| used > max) {
            return Err(LifecycleError::StorageQuota { max });
        }
    }
    Ok(())
}
