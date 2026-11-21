use std::fmt;

use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{KeyId, SealedSecret};

use super::{NotifyStoreError, classify, truncate_chars};
use crate::Store;
use crate::forward::TIMESTAMP_SQL;
use crate::secrets::ReplaceSealedOutcome;

pub const MAX_PUSH_SUBSCRIPTIONS_PER_USER: usize = 10;
pub const MAX_PUSH_ENDPOINT_BYTES: usize = 2048;
pub const MAX_USER_AGENT_CHARS: usize = 256;
pub const MAX_REJECTED_ATTEMPTS: i16 = 5;
pub const VAPID_SCOPE: &str = "zzzzzzzzzzzzzzzzzzzzzzzz";
pub const VAPID_SECRET: &str = "VAPID_PRIVATE_KEY";

const LOCK_USER: &str = "SELECT id FROM users WHERE id = $1 FOR UPDATE";

const COUNT_OTHER: &str =
    "SELECT count(*) FROM push_subscriptions WHERE user_id = $1 AND endpoint_url <> $2";

const UPSERT: &str = "INSERT INTO push_subscriptions \
       (id, user_id, endpoint_url, host, p256dh, auth, user_agent) \
     VALUES ($1, $2, $3, $4, $5, $6, $7) \
     ON CONFLICT (endpoint_url) DO UPDATE SET \
       id = CASE WHEN push_subscriptions.user_id = EXCLUDED.user_id \
         THEN push_subscriptions.id ELSE EXCLUDED.id END, \
       created_at = CASE WHEN push_subscriptions.user_id = EXCLUDED.user_id \
         THEN push_subscriptions.created_at ELSE now() END, \
       last_success_at = CASE WHEN push_subscriptions.user_id = EXCLUDED.user_id \
         THEN push_subscriptions.last_success_at ELSE NULL END, \
       user_id = EXCLUDED.user_id, host = EXCLUDED.host, p256dh = EXCLUDED.p256dh, \
       auth = EXCLUDED.auth, user_agent = EXCLUDED.user_agent, failures = 0 \
     WHERE push_subscriptions.user_id = EXCLUDED.user_id \
       OR (push_subscriptions.p256dh = EXCLUDED.p256dh AND push_subscriptions.auth = EXCLUDED.auth) \
     RETURNING id, to_char(created_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS created_at";

const LIST: &str = "SELECT id, host, user_agent, \
       to_char(created_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS created_at, \
       to_char(last_success_at AT TIME ZONE 'UTC', __TIMESTAMP__) AS last_success_at \
     FROM push_subscriptions WHERE user_id = $1 ORDER BY created_at, id LIMIT $2";

const MAY_VIEW: &str = "SELECT EXISTS ( \
       SELECT 1 FROM endpoints e \
       LEFT JOIN endpoint_members m ON m.endpoint_id = e.id AND m.user_id = $2 \
       WHERE e.id = $1 AND e.deleted_at IS NULL \
         AND (e.owner_id IS NULL OR e.owner_id = $2 OR m.user_id IS NOT NULL))";

#[derive(Clone, PartialEq, Eq)]
pub struct PushSubscription {
    pub id: Uuid,
    pub endpoint_url: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

impl fmt::Debug for PushSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushSubscription")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct NewPushSubscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub endpoint_url: String,
    pub host: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
    pub user_agent: Option<String>,
}

impl fmt::Debug for NewPushSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewPushSubscription")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushSubscriptionInfo {
    pub id: Uuid,
    pub host: String,
    pub user_agent: Option<String>,
    pub created_at: String,
    pub last_success_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedPushSubscription {
    pub id: Uuid,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavePushOutcome {
    Saved(SavedPushSubscription),
    LimitReached,
    AlreadyRegistered,
    UserGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PushResults<'a> {
    pub delivered: &'a [Uuid],
    pub rejected: &'a [Uuid],
}

#[derive(Clone, PartialEq, Eq)]
pub struct StoredVapid {
    pub public_key: Vec<u8>,
    pub sealed: SealedSecret,
}

impl fmt::Debug for StoredVapid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredVapid").finish_non_exhaustive()
    }
}

impl Store {
    pub async fn save_push_subscription(
        &self,
        subscription: &NewPushSubscription,
    ) -> Result<SavePushOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let locked: Option<Uuid> = sqlx::query_scalar(LOCK_USER)
            .bind(subscription.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if locked.is_none() {
            return Ok(SavePushOutcome::UserGone);
        }
        let held: i64 = sqlx::query_scalar(COUNT_OTHER)
            .bind(subscription.user_id)
            .bind(subscription.endpoint_url.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        if usize::try_from(held).map_or(true, |held| held >= MAX_PUSH_SUBSCRIPTIONS_PER_USER) {
            return Ok(SavePushOutcome::LimitReached);
        }
        let sql = UPSERT.replace("__TIMESTAMP__", TIMESTAMP_SQL);
        let agent = subscription
            .user_agent
            .as_deref()
            .map(|agent| truncate_chars(agent, MAX_USER_AGENT_CHARS));
        let row = sqlx::query(&sql)
            .bind(subscription.id)
            .bind(subscription.user_id)
            .bind(subscription.endpoint_url.as_str())
            .bind(subscription.host.as_str())
            .bind(subscription.p256dh.as_slice())
            .bind(subscription.auth.as_slice())
            .bind(agent)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(SavePushOutcome::AlreadyRegistered);
        };
        let saved = SavedPushSubscription {
            id: row.try_get("id").map_err(classify)?,
            created_at: row.try_get("created_at").map_err(classify)?,
        };
        tx.commit().await.map_err(classify)?;
        Ok(SavePushOutcome::Saved(saved))
    }

    pub async fn list_push_subscriptions(
        &self,
        user: Uuid,
    ) -> Result<Vec<PushSubscriptionInfo>, NotifyStoreError> {
        let sql = LIST.replace("__TIMESTAMP__", TIMESTAMP_SQL);
        let limit = i64::try_from(MAX_PUSH_SUBSCRIPTIONS_PER_USER).unwrap_or(i64::MAX);
        let rows = sqlx::query(&sql)
            .bind(user)
            .bind(limit)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(info_from_row).collect()
    }

    pub async fn delete_push_subscription(
        &self,
        user: Uuid,
        subscription: Uuid,
    ) -> Result<bool, NotifyStoreError> {
        let deleted = sqlx::query("DELETE FROM push_subscriptions WHERE id = $1 AND user_id = $2")
            .bind(subscription)
            .bind(user)
            .execute(self.pool())
            .await
            .map_err(classify)?
            .rows_affected();
        Ok(deleted == 1)
    }

    pub async fn user_may_view_endpoint(
        &self,
        endpoint: &EndpointId,
        user: Uuid,
    ) -> Result<bool, NotifyStoreError> {
        sqlx::query_scalar(MAY_VIEW)
            .bind(endpoint.unpack())
            .bind(user)
            .fetch_one(self.pool())
            .await
            .map_err(classify)
    }

    pub async fn push_subscriptions_of(
        &self,
        user: Uuid,
    ) -> Result<Vec<PushSubscription>, NotifyStoreError> {
        let limit = i64::try_from(MAX_PUSH_SUBSCRIPTIONS_PER_USER).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT id, endpoint_url, p256dh, auth FROM push_subscriptions \
             WHERE user_id = $1 ORDER BY created_at, id LIMIT $2",
        )
        .bind(user)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;
        rows.iter().map(subscription_from_row).collect()
    }

    pub async fn remove_push_subscriptions(&self, ids: &[Uuid]) -> Result<u64, NotifyStoreError> {
        if ids.is_empty() {
            return Ok(0);
        }
        Ok(
            sqlx::query("DELETE FROM push_subscriptions WHERE id = ANY($1)")
                .bind(ids)
                .execute(self.pool())
                .await
                .map_err(classify)?
                .rows_affected(),
        )
    }

    pub async fn record_push_results(
        &self,
        results: PushResults<'_>,
    ) -> Result<u64, NotifyStoreError> {
        if !results.delivered.is_empty() {
            sqlx::query(
                "UPDATE push_subscriptions SET last_success_at = now(), failures = 0 \
                 WHERE id = ANY($1)",
            )
            .bind(results.delivered)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        }
        if results.rejected.is_empty() {
            return Ok(0);
        }
        sqlx::query(
            "UPDATE push_subscriptions SET failures = LEAST(failures, 32000) + 1 \
             WHERE id = ANY($1)",
        )
        .bind(results.rejected)
        .execute(self.pool())
        .await
        .map_err(classify)?;
        Ok(
            sqlx::query("DELETE FROM push_subscriptions WHERE id = ANY($1) AND failures >= $2")
                .bind(results.rejected)
                .bind(MAX_REJECTED_ATTEMPTS)
                .execute(self.pool())
                .await
                .map_err(classify)?
                .rows_affected(),
        )
    }

    pub async fn get_vapid_key(&self) -> Result<Option<StoredVapid>, NotifyStoreError> {
        let row = sqlx::query(
            "SELECT public_key, secret_key_id, secret_nonce, secret_ciphertext \
             FROM push_vapid_keys WHERE id = 1",
        )
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;
        row.as_ref().map(vapid_from_row).transpose()
    }

    pub async fn insert_vapid_key(
        &self,
        public_key: &[u8],
        sealed: &SealedSecret,
    ) -> Result<bool, NotifyStoreError> {
        let inserted = sqlx::query(
            "INSERT INTO push_vapid_keys \
               (id, public_key, secret_key_id, secret_nonce, secret_ciphertext) \
             VALUES (1, $1, $2, $3, $4) ON CONFLICT (id) DO NOTHING",
        )
        .bind(public_key)
        .bind(sealed.key_id().to_column())
        .bind(sealed.nonce().as_slice())
        .bind(sealed.ciphertext())
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();
        Ok(inserted == 1)
    }

    pub async fn replace_sealed_vapid(
        &self,
        stored: &SealedSecret,
        replacement: &SealedSecret,
    ) -> Result<ReplaceSealedOutcome, NotifyStoreError> {
        let replaced = sqlx::query(
            "UPDATE push_vapid_keys SET secret_key_id = $2, secret_nonce = $3, \
               secret_ciphertext = $4 \
             WHERE id = 1 AND secret_key_id = $1 AND secret_nonce = $5",
        )
        .bind(stored.key_id().to_column())
        .bind(replacement.key_id().to_column())
        .bind(replacement.nonce().as_slice())
        .bind(replacement.ciphertext())
        .bind(stored.nonce().as_slice())
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();
        Ok(if replaced == 1 {
            ReplaceSealedOutcome::Replaced
        } else {
            ReplaceSealedOutcome::Changed
        })
    }
}

fn info_from_row(row: &PgRow) -> Result<PushSubscriptionInfo, NotifyStoreError> {
    Ok(PushSubscriptionInfo {
        id: row.try_get("id").map_err(classify)?,
        host: row.try_get("host").map_err(classify)?,
        user_agent: row.try_get("user_agent").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
        last_success_at: row.try_get("last_success_at").map_err(classify)?,
    })
}

fn subscription_from_row(row: &PgRow) -> Result<PushSubscription, NotifyStoreError> {
    Ok(PushSubscription {
        id: row.try_get("id").map_err(classify)?,
        endpoint_url: row.try_get("endpoint_url").map_err(classify)?,
        p256dh: row.try_get("p256dh").map_err(classify)?,
        auth: row.try_get("auth").map_err(classify)?,
    })
}

fn vapid_from_row(row: &PgRow) -> Result<StoredVapid, NotifyStoreError> {
    let key_id: i16 = row.try_get("secret_key_id").map_err(classify)?;
    let nonce: Vec<u8> = row.try_get("secret_nonce").map_err(classify)?;
    let ciphertext: Vec<u8> = row.try_get("secret_ciphertext").map_err(classify)?;
    let invalid = |reason: String| NotifyStoreError::Invalid(format!("vapid key: {reason}"));
    let key_id = KeyId::from_column(key_id).map_err(|error| invalid(error.to_string()))?;
    let sealed = SealedSecret::from_parts(key_id, &nonce, ciphertext)
        .map_err(|error| invalid(error.to_string()))?;
    Ok(StoredVapid {
        public_key: row.try_get("public_key").map_err(classify)?,
        sealed,
    })
}
