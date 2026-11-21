use std::time::Duration;

use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction};

use crate::endpoints::EndpointError;

pub const MIN_RETENTION_SECONDS: i32 = 3600;
pub const MIN_CAPTURE_TTL_SECONDS: i32 = 60;

const LOCK_OWNER_SQL: &str = "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE";

const COUNT_NEVER_EXPIRES_SQL: &str = "SELECT count(*) FROM endpoints \
    WHERE owner_id = $1 AND never_expires AND deleted_at IS NULL";

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
}

impl LifecycleLimits {
    pub const NONE: Self = Self {
        never_expire_max: 0,
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
    Ok(())
}
