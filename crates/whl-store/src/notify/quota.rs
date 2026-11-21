use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{NotifyStoreError, classify};
use crate::Store;

const COUNT: &str = "INSERT INTO notification_counters (subject, window_start, count) \
     VALUES ($1, to_timestamp(floor(extract(epoch FROM now()) / $3::int8) * $3::int8), 1) \
     ON CONFLICT (subject, window_start) DO UPDATE SET count = notification_counters.count + 1 \
     WHERE notification_counters.count < $2 \
     RETURNING count";

const LOCK: &str = "SELECT quota_checked FROM notification_deliveries \
     WHERE id = $1 AND attempts = $2 AND state = 'running' FOR UPDATE";

const MARK_CHECKED: &str = "UPDATE notification_deliveries SET quota_checked = true WHERE id = $1";

const SUPPRESS: &str = "UPDATE notification_deliveries SET state = 'suppressed', \
       locked_until = NULL, finished_at = now() \
     WHERE id = $1 AND attempts = $2 AND state = 'running'";

pub const HOUR_SECS: i64 = 3600;
pub const DAY_SECS: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaCap {
    pub subject: String,
    pub limit: i32,
    pub window_secs: i64,
}

impl QuotaCap {
    pub fn hourly(subject: String, limit: i32) -> Self {
        Self {
            subject,
            limit,
            window_secs: HOUR_SECS,
        }
    }

    pub fn daily(subject: String, limit: i32) -> Self {
        Self {
            subject,
            limit,
            window_secs: DAY_SECS,
        }
    }

    pub fn windowed(subject: String, limit: i32, window_secs: i64) -> Self {
        Self {
            subject,
            limit,
            window_secs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaVerdict {
    Allowed,
    Suppressed,
    Superseded,
}

pub(crate) async fn charge_caps(
    tx: &mut Transaction<'_, Postgres>,
    caps: &[QuotaCap],
) -> Result<bool, NotifyStoreError> {
    for cap in caps {
        if cap.limit < 1 || cap.window_secs < 1 {
            return Ok(false);
        }
        let counted: Option<i32> = sqlx::query_scalar(COUNT)
            .bind(cap.subject.as_str())
            .bind(cap.limit)
            .bind(cap.window_secs)
            .fetch_optional(&mut **tx)
            .await
            .map_err(classify)?;
        if counted.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Store {
    pub async fn charge_delivery(
        &self,
        delivery: Uuid,
        attempt: i16,
        caps: &[QuotaCap],
    ) -> Result<QuotaVerdict, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let locked: Option<bool> = sqlx::query_scalar(LOCK)
            .bind(delivery)
            .bind(attempt)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(already) = locked else {
            return Ok(QuotaVerdict::Superseded);
        };
        if already {
            return Ok(QuotaVerdict::Allowed);
        }
        if !charge_caps(&mut tx, caps).await? {
            tx.rollback().await.map_err(classify)?;
            return self.suppress_delivery(delivery, attempt).await;
        }
        sqlx::query(MARK_CHECKED)
            .bind(delivery)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(QuotaVerdict::Allowed)
    }

    async fn suppress_delivery(
        &self,
        delivery: Uuid,
        attempt: i16,
    ) -> Result<QuotaVerdict, NotifyStoreError> {
        let suppressed = sqlx::query(SUPPRESS)
            .bind(delivery)
            .bind(attempt)
            .execute(self.pool())
            .await
            .map_err(classify)?
            .rows_affected();
        Ok(if suppressed == 1 {
            QuotaVerdict::Suppressed
        } else {
            QuotaVerdict::Superseded
        })
    }
}
