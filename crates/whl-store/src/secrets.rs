use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{Row, Transaction};
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{KeyId, SealedSecret, SecretName};

use crate::Store;
use crate::forward::{ForwardStoreError, TIMESTAMP_SQL, lock_live_endpoint};

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the secret query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored secret data is invalid: {0}")]
    Invalid(String),
}

impl SecretStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl From<ForwardStoreError> for SecretStoreError {
    fn from(error: ForwardStoreError) -> Self {
        match error {
            ForwardStoreError::Unavailable(source) => Self::Unavailable(source),
            ForwardStoreError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

fn classify(error: sqlx::Error) -> SecretStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        SecretStoreError::Unavailable(error)
    } else {
        SecretStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMeta {
    pub name: SecretName,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutSecretOutcome {
    Stored(SecretMeta),
    EndpointGone,
    LimitReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteSecretOutcome {
    Deleted,
    EndpointGone,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceSealedOutcome {
    Replaced,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSecret {
    pub endpoint: EndpointId,
    pub name: SecretName,
    pub sealed: SealedSecret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationCandidate {
    pub endpoint: String,
    pub name: String,
    pub channel: bool,
    pub stored: Option<StoredSecret>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationCursor {
    pub endpoint: String,
    pub name: String,
    pub channel: bool,
}

impl RotationCursor {
    pub fn start() -> Self {
        Self {
            endpoint: String::new(),
            name: String::new(),
            channel: false,
        }
    }
}

const CHANNEL_SECRET_NAME_SQL: &str = "'C' || upper(replace(id::text, '-', ''))";

fn rotation_union() -> String {
    format!(
        "SELECT endpoint_id, name, 0::int2 AS source, key_id, nonce, ciphertext \
         FROM endpoint_secrets \
         UNION ALL \
         SELECT endpoint_id, {CHANNEL_SECRET_NAME_SQL} AS name, 1::int2 AS source, \
           secret_key_id AS key_id, secret_nonce AS nonce, secret_ciphertext AS ciphertext \
         FROM notification_channels WHERE secret_key_id IS NOT NULL"
    )
}

fn meta_columns() -> String {
    format!(
        "name, \
         to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at, \
         to_char(updated_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS updated_at"
    )
}

impl Store {
    pub async fn list_secrets(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<SecretMeta>, SecretStoreError> {
        let sql = format!(
            "SELECT {} FROM endpoint_secrets WHERE endpoint_id = $1 ORDER BY name",
            meta_columns()
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(meta_from_row).collect()
    }

    pub async fn put_secret(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &SecretName,
        sealed: &SealedSecret,
        max_per_endpoint: usize,
    ) -> Result<PutSecretOutcome, SecretStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(PutSecretOutcome::EndpointGone);
        }
        if !secret_exists(&mut tx, endpoint, name).await? {
            let held: i64 =
                sqlx::query_scalar("SELECT count(*) FROM endpoint_secrets WHERE endpoint_id = $1")
                    .bind(endpoint.unpack())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(classify)?;
            if usize::try_from(held).map_or(true, |held| held >= max_per_endpoint) {
                return Ok(PutSecretOutcome::LimitReached);
            }
        }
        let sql = format!(
            "INSERT INTO endpoint_secrets (endpoint_id, name, key_id, nonce, ciphertext) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (endpoint_id, name) DO UPDATE SET \
             key_id = EXCLUDED.key_id, nonce = EXCLUDED.nonce, \
             ciphertext = EXCLUDED.ciphertext, updated_at = now() \
             RETURNING {}",
            meta_columns()
        );
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(name.unpack())
            .bind(sealed.key_id().to_column())
            .bind(sealed.nonce().as_slice())
            .bind(sealed.ciphertext())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let meta = meta_from_row(&row)?;
        tx.commit().await.map_err(classify)?;
        Ok(PutSecretOutcome::Stored(meta))
    }

    pub async fn delete_secret(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        name: &SecretName,
    ) -> Result<DeleteSecretOutcome, SecretStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteSecretOutcome::EndpointGone);
        }
        let deleted =
            sqlx::query("DELETE FROM endpoint_secrets WHERE endpoint_id = $1 AND name = $2")
                .bind(endpoint.unpack())
                .bind(name.unpack())
                .execute(&mut *tx)
                .await
                .map_err(classify)?
                .rows_affected();
        tx.commit().await.map_err(classify)?;
        Ok(if deleted == 0 {
            DeleteSecretOutcome::NotFound
        } else {
            DeleteSecretOutcome::Deleted
        })
    }

    pub async fn get_sealed_secret(
        &self,
        endpoint: &EndpointId,
        name: &SecretName,
    ) -> Result<Option<SealedSecret>, SecretStoreError> {
        let row = sqlx::query(
            "SELECT key_id, nonce, ciphertext FROM endpoint_secrets \
             WHERE endpoint_id = $1 AND name = $2",
        )
        .bind(endpoint.unpack())
        .bind(name.unpack())
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;
        row.map(|row| sealed_from_row(&row, endpoint.unpack(), name.unpack()))
            .transpose()
    }

    pub async fn count_secrets_outside_key(&self, active: KeyId) -> Result<u64, SecretStoreError> {
        let sql = format!(
            "SELECT count(*) FROM ({}) rows WHERE key_id <> $1",
            rotation_union()
        );
        let count: i64 = sqlx::query_scalar(&sql)
            .bind(active.to_column())
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        let vapid: i64 =
            sqlx::query_scalar("SELECT count(*) FROM push_vapid_keys WHERE secret_key_id <> $1")
                .bind(active.to_column())
                .fetch_one(self.pool())
                .await
                .map_err(classify)?;
        u64::try_from(count + vapid)
            .map_err(|_| SecretStoreError::Invalid("a negative row count".to_owned()))
    }

    pub async fn rotation_batch(
        &self,
        active: KeyId,
        after: &RotationCursor,
        limit: u32,
    ) -> Result<Vec<RotationCandidate>, SecretStoreError> {
        let sql = format!(
            "SELECT endpoint_id, name, source, key_id, nonce, ciphertext FROM ({}) rows \
             WHERE key_id <> $1 AND (endpoint_id, name, source) > ($2, $3, $4) \
             ORDER BY endpoint_id, name, source LIMIT $5",
            rotation_union()
        );
        let rows = sqlx::query(&sql)
            .bind(active.to_column())
            .bind(after.endpoint.as_str())
            .bind(after.name.as_str())
            .bind(i16::from(after.channel))
            .bind(i64::from(limit))
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(candidate_from_row).collect()
    }

    pub async fn replace_sealed_secret(
        &self,
        stored: &StoredSecret,
        replacement: &SealedSecret,
    ) -> Result<ReplaceSealedOutcome, SecretStoreError> {
        let replaced = sqlx::query(
            "UPDATE endpoint_secrets SET key_id = $4, nonce = $5, ciphertext = $6 \
             WHERE endpoint_id = $1 AND name = $2 AND key_id = $3 AND nonce = $7",
        )
        .bind(stored.endpoint.unpack())
        .bind(stored.name.unpack())
        .bind(stored.sealed.key_id().to_column())
        .bind(replacement.key_id().to_column())
        .bind(replacement.nonce().as_slice())
        .bind(replacement.ciphertext())
        .bind(stored.sealed.nonce().as_slice())
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

    pub async fn replace_sealed_channel_secret(
        &self,
        stored: &StoredSecret,
        replacement: &SealedSecret,
    ) -> Result<ReplaceSealedOutcome, SecretStoreError> {
        let sql = format!(
            "UPDATE notification_channels SET secret_key_id = $4, secret_nonce = $5, \
             secret_ciphertext = $6 \
             WHERE endpoint_id = $1 AND {CHANNEL_SECRET_NAME_SQL} = $2 \
             AND secret_key_id = $3 AND secret_nonce = $7"
        );
        let replaced = sqlx::query(&sql)
            .bind(stored.endpoint.unpack())
            .bind(stored.name.unpack())
            .bind(stored.sealed.key_id().to_column())
            .bind(replacement.key_id().to_column())
            .bind(replacement.nonce().as_slice())
            .bind(replacement.ciphertext())
            .bind(stored.sealed.nonce().as_slice())
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

async fn secret_exists(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    endpoint: &EndpointId,
    name: &SecretName,
) -> Result<bool, SecretStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM endpoint_secrets WHERE endpoint_id = $1 AND name = $2)",
    )
    .bind(endpoint.unpack())
    .bind(name.unpack())
    .fetch_one(&mut **tx)
    .await
    .map_err(classify)
}

fn meta_from_row(row: &PgRow) -> Result<SecretMeta, SecretStoreError> {
    let name: String = row.try_get("name").map_err(classify)?;
    Ok(SecretMeta {
        name: SecretName::parse(&name).map_err(|error| {
            SecretStoreError::Invalid(format!("stored secret name {name:?}: {error}"))
        })?,
        created_at: row.try_get("created_at").map_err(classify)?,
        updated_at: row.try_get("updated_at").map_err(classify)?,
    })
}

fn sealed_from_row(
    row: &PgRow,
    endpoint: &str,
    name: &str,
) -> Result<SealedSecret, SecretStoreError> {
    let key_id: i16 = row.try_get("key_id").map_err(classify)?;
    let nonce: Vec<u8> = row.try_get("nonce").map_err(classify)?;
    let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(classify)?;
    let invalid = |reason: String| {
        SecretStoreError::Invalid(format!("secret {name} of endpoint {endpoint}: {reason}"))
    };
    let key_id = KeyId::from_column(key_id).map_err(|error| invalid(error.to_string()))?;
    SealedSecret::from_parts(key_id, &nonce, ciphertext).map_err(|error| invalid(error.to_string()))
}

fn candidate_from_row(row: &PgRow) -> Result<RotationCandidate, SecretStoreError> {
    let endpoint: String = row.try_get("endpoint_id").map_err(classify)?;
    let name: String = row.try_get("name").map_err(classify)?;
    let source: i16 = row.try_get("source").map_err(classify)?;
    let stored = decode_candidate(row, &endpoint, &name);
    Ok(RotationCandidate {
        endpoint,
        name,
        channel: source == 1,
        stored,
    })
}

fn decode_candidate(row: &PgRow, endpoint: &str, name: &str) -> Option<StoredSecret> {
    Some(StoredSecret {
        endpoint: EndpointId::parse(endpoint).ok()?,
        name: SecretName::parse(name).ok()?,
        sealed: sealed_from_row(row, endpoint, name).ok()?,
    })
}
