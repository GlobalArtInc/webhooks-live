use std::time::Duration;

use sqlx::Row;
use sqlx::types::Uuid;
use whl_core::tier::Tiers;

use crate::composer::{ComposerStoreError, OUTBOUND_METRIC, classify};
use crate::store::Store;

pub type QuotaStoreError = ComposerStoreError;

pub const EXPORTS_METRIC: &str = "exports";
pub const IMPORTS_METRIC: &str = "imports";
pub const ENDPOINT_CAP_DIVISOR: u64 = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndpointByteCaps {
    pub anonymous: Option<i64>,
    pub owner_default: Option<i64>,
}

impl EndpointByteCaps {
    pub fn from_tiers(tiers: &Tiers) -> Self {
        let share = |bytes: Option<u64>| {
            bytes.map(|bytes| i64::try_from(bytes / ENDPOINT_CAP_DIVISOR).unwrap_or(i64::MAX))
        };
        Self {
            anonymous: share(tiers.anonymous().limits().storage_bytes),
            owner_default: share(tiers.member().limits().storage_bytes),
        }
    }
}

const WINDOW_HOUR: &str = "date_trunc('hour', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'";

const LOCK_USERS_BY_EMAIL_SQL: &str = "SELECT id FROM users WHERE lower(email) = lower($1) AND email_verified \
     ORDER BY id FOR NO KEY UPDATE";

const LOCK_USER_BY_ID_SQL: &str = "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE";

const SET_TIER_SQL: &str = "UPDATE users SET tier = $2 WHERE id = $1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OwnerUsage {
    pub endpoints: i64,
    pub storage_bytes: i64,
    pub never_expire: i64,
    pub schedules: i64,
    pub tokens: i64,
    pub outbound_hour: i64,
    pub exports_hour: i64,
    pub notifications_hour: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserSelector {
    Email(String),
    Id(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetTierOutcome {
    Changed { user: Uuid, previous: String },
    NotFound,
    Ambiguous { users: Vec<Uuid> },
}

fn usage_sql() -> String {
    format!(
        "SELECT \
           (SELECT count(*) FROM endpoints WHERE owner_id = $1 AND deleted_at IS NULL \
              AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))) AS endpoints, \
           (SELECT COALESCE(sum(bytes_used), 0)::bigint FROM endpoints WHERE owner_id = $1 AND deleted_at IS NULL) AS storage_bytes, \
           (SELECT count(*) FROM endpoints WHERE owner_id = $1 AND never_expires AND deleted_at IS NULL) AS never_expire, \
           (SELECT count(*) FROM schedules s JOIN endpoints e ON e.id = s.endpoint_id \
              WHERE e.owner_id = $1 AND e.deleted_at IS NULL) AS schedules, \
           (SELECT count(*) FROM api_tokens WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()) AS tokens, \
           COALESCE((SELECT count FROM usage_counters WHERE subject = $3 AND metric = '{OUTBOUND_METRIC}' \
              AND window_start = {WINDOW_HOUR}), 0)::bigint AS outbound_hour, \
           COALESCE((SELECT count FROM usage_counters WHERE subject = $3 AND metric = '{EXPORTS_METRIC}' \
              AND window_start = {WINDOW_HOUR}), 0)::bigint AS exports_hour, \
           COALESCE((SELECT count FROM notification_counters WHERE subject = $3 \
              AND window_start = {WINDOW_HOUR}), 0)::bigint AS notifications_hour"
    )
}

pub fn user_subject(user: Uuid) -> String {
    format!("u:{}", user.hyphenated())
}

impl Store {
    pub async fn owner_usage(
        &self,
        user: Uuid,
        retention: Duration,
    ) -> Result<OwnerUsage, QuotaStoreError> {
        let row = sqlx::query(&usage_sql())
            .bind(user)
            .bind(retention.as_secs_f64())
            .bind(user_subject(user))
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        let figure = |name: &str| row.try_get::<i64, _>(name).map_err(classify);
        Ok(OwnerUsage {
            endpoints: figure("endpoints")?,
            storage_bytes: figure("storage_bytes")?,
            never_expire: figure("never_expire")?,
            schedules: figure("schedules")?,
            tokens: figure("tokens")?,
            outbound_hour: figure("outbound_hour")?,
            exports_hour: figure("exports_hour")?,
            notifications_hour: figure("notifications_hour")?,
        })
    }

    pub async fn set_user_tier(
        &self,
        selector: &UserSelector,
        tier: &str,
    ) -> Result<SetTierOutcome, QuotaStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let users: Vec<Uuid> = match selector {
            UserSelector::Email(email) => sqlx::query_scalar(LOCK_USERS_BY_EMAIL_SQL)
                .bind(email)
                .fetch_all(&mut *tx)
                .await
                .map_err(classify)?,
            UserSelector::Id(id) => sqlx::query_scalar(LOCK_USER_BY_ID_SQL)
                .bind(id)
                .fetch_all(&mut *tx)
                .await
                .map_err(classify)?,
        };
        let user = match users.as_slice() {
            [] => return Ok(SetTierOutcome::NotFound),
            [only] => *only,
            _ => return Ok(SetTierOutcome::Ambiguous { users }),
        };
        let previous: String = sqlx::query_scalar("SELECT tier FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query(SET_TIER_SQL)
            .bind(user)
            .bind(tier)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(SetTierOutcome::Changed { user, previous })
    }
}
