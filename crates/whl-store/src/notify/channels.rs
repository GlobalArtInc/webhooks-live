use std::collections::BTreeSet;
use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::SealedSecret;

use super::deliveries::sealed_from_row;
use super::model::{Channel, ChannelKind, Trigger, parse_json};
use super::{NotifyStoreError, classify};
use crate::Store;
use crate::forward::{TIMESTAMP_SQL, lock_live_endpoint};
use crate::secrets::missing_secrets;

pub const MAX_CHANNELS_PER_ENDPOINT: usize = 10;
const TELEGRAM_TOKEN_KEY: &str = "token_secret";

#[derive(Debug, Clone)]
pub struct NewChannel {
    pub id: Uuid,
    pub kind: ChannelKind,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub config: serde_json::Value,
    pub sealed: Option<SealedSecret>,
    pub template: Option<String>,
    pub per_hour: i32,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ConfigChange {
    pub config: serde_json::Value,
    pub sealed: Option<SealedSecret>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub trigger: Option<Trigger>,
    pub config: Option<ConfigChange>,
    pub template: Option<Option<String>>,
    pub per_hour: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateChannelOutcome {
    Created(Channel),
    EndpointGone,
    LimitReached,
    SecretsMissing(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateChannelOutcome {
    Updated(Channel),
    EndpointGone,
    ChannelNotFound,
    SecretsMissing(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteChannelOutcome {
    Deleted,
    EndpointGone,
    ChannelNotFound,
}

fn telegram_secrets(kind: ChannelKind, config: &serde_json::Value) -> BTreeSet<String> {
    if kind != ChannelKind::Telegram {
        return BTreeSet::new();
    }
    config
        .get(TELEGRAM_TOKEN_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .into_iter()
        .collect()
}

fn columns() -> String {
    format!(
        "id, endpoint_id, kind, name, enabled, trigger, condition::text AS condition, \
         config::text AS config, template, per_hour, created_by, \
         to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at"
    )
}

pub(crate) async fn read_channels(
    conn: &mut PgConnection,
    endpoint: &EndpointId,
) -> Result<Vec<Channel>, NotifyStoreError> {
    let sql = format!(
        "SELECT {} FROM notification_channels WHERE endpoint_id = $1 ORDER BY created_at, id",
        columns()
    );
    let rows = sqlx::query(&sql)
        .bind(endpoint.unpack())
        .fetch_all(conn)
        .await
        .map_err(classify)?;
    rows.iter().map(channel_from_row).collect()
}

impl Store {
    pub async fn list_channels(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<Channel>, NotifyStoreError> {
        let sql = format!(
            "SELECT {} FROM notification_channels WHERE endpoint_id = $1 ORDER BY created_at, id",
            columns()
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(channel_from_row).collect()
    }

    pub async fn get_channel(
        &self,
        endpoint: &EndpointId,
        channel: Uuid,
    ) -> Result<Option<Channel>, NotifyStoreError> {
        let sql = format!(
            "SELECT {} FROM notification_channels WHERE endpoint_id = $1 AND id = $2",
            columns()
        );
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(channel)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(channel_from_row).transpose()
    }

    pub async fn get_channel_sealed(
        &self,
        endpoint: &EndpointId,
        channel: Uuid,
    ) -> Result<Option<SealedSecret>, NotifyStoreError> {
        let row = sqlx::query(
            "SELECT secret_key_id, secret_nonce, secret_ciphertext FROM notification_channels \
             WHERE endpoint_id = $1 AND id = $2",
        )
        .bind(endpoint.unpack())
        .bind(channel)
        .fetch_optional(self.pool())
        .await
        .map_err(classify)?;
        row.as_ref()
            .map(sealed_from_row)
            .transpose()
            .map(Option::flatten)
    }

    pub async fn create_channel(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        channel: &NewChannel,
    ) -> Result<CreateChannelOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(CreateChannelOutcome::EndpointGone);
        }
        let missing = missing_secrets(
            &mut tx,
            endpoint,
            &telegram_secrets(channel.kind, &channel.config),
        )
        .await
        .map_err(classify)?;
        if !missing.is_empty() {
            return Ok(CreateChannelOutcome::SecretsMissing(missing));
        }
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notification_channels WHERE endpoint_id = $1")
                .bind(endpoint.unpack())
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?;
        if usize::try_from(held).map_or(true, |held| held >= MAX_CHANNELS_PER_ENDPOINT) {
            return Ok(CreateChannelOutcome::LimitReached);
        }
        let created = insert_channel(&mut tx, endpoint, channel).await?;
        tx.commit().await.map_err(classify)?;
        Ok(CreateChannelOutcome::Created(created))
    }

    pub async fn update_channel(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        channel: Uuid,
        patch: &ChannelPatch,
    ) -> Result<UpdateChannelOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(UpdateChannelOutcome::EndpointGone);
        }
        let select = format!(
            "SELECT {} FROM notification_channels WHERE endpoint_id = $1 AND id = $2 FOR UPDATE",
            columns()
        );
        let Some(row) = sqlx::query(&select)
            .bind(endpoint.unpack())
            .bind(channel)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?
        else {
            return Ok(UpdateChannelOutcome::ChannelNotFound);
        };
        let current = channel_from_row(&row)?;
        if let Some(change) = &patch.config {
            let missing = missing_secrets(
                &mut tx,
                endpoint,
                &telegram_secrets(current.kind, &change.config),
            )
            .await
            .map_err(classify)?;
            if !missing.is_empty() {
                return Ok(UpdateChannelOutcome::SecretsMissing(missing));
            }
        }
        let name = patch.name.as_deref().unwrap_or(&current.name);
        let enabled = patch.enabled.unwrap_or(current.enabled);
        let trigger = patch.trigger.unwrap_or(current.trigger);
        let template = match &patch.template {
            Some(template) => template.as_deref(),
            None => current.template.as_deref(),
        };
        let per_hour = patch.per_hour.unwrap_or(current.per_hour);
        let update = format!(
            "UPDATE notification_channels SET name = $3, enabled = $4, trigger = $5, \
             template = $6, per_hour = $7 \
             WHERE endpoint_id = $1 AND id = $2 RETURNING {}",
            columns()
        );
        let mut updated = sqlx::query(&update)
            .bind(endpoint.unpack())
            .bind(channel)
            .bind(name)
            .bind(enabled)
            .bind(trigger.as_str())
            .bind(template)
            .bind(per_hour)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        if let Some(change) = &patch.config {
            let replace = format!(
                "UPDATE notification_channels SET config = $3::text::jsonb, secret_key_id = $4, \
                 secret_nonce = $5, secret_ciphertext = $6 \
                 WHERE endpoint_id = $1 AND id = $2 RETURNING {}",
                columns()
            );
            updated = sqlx::query(&replace)
                .bind(endpoint.unpack())
                .bind(channel)
                .bind(change.config.to_string())
                .bind(
                    change
                        .sealed
                        .as_ref()
                        .map(|sealed| sealed.key_id().to_column()),
                )
                .bind(change.sealed.as_ref().map(|sealed| sealed.nonce().to_vec()))
                .bind(
                    change
                        .sealed
                        .as_ref()
                        .map(|sealed| sealed.ciphertext().to_vec()),
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?;
        }
        let channel = channel_from_row(&updated)?;
        tx.commit().await.map_err(classify)?;
        Ok(UpdateChannelOutcome::Updated(channel))
    }

    pub async fn delete_channel(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        channel: Uuid,
    ) -> Result<DeleteChannelOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteChannelOutcome::EndpointGone);
        }
        let deleted =
            sqlx::query("DELETE FROM notification_channels WHERE endpoint_id = $1 AND id = $2")
                .bind(endpoint.unpack())
                .bind(channel)
                .execute(&mut *tx)
                .await
                .map_err(classify)?
                .rows_affected();
        tx.commit().await.map_err(classify)?;
        Ok(if deleted == 0 {
            DeleteChannelOutcome::ChannelNotFound
        } else {
            DeleteChannelOutcome::Deleted
        })
    }
}

pub(crate) async fn insert_channel(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    channel: &NewChannel,
) -> Result<Channel, NotifyStoreError> {
    let sql = format!(
        "INSERT INTO notification_channels \
         (id, endpoint_id, created_by, kind, name, enabled, trigger, config, \
          secret_key_id, secret_nonce, secret_ciphertext, template, per_hour) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::jsonb, $9, $10, $11, $12, $13) \
         RETURNING {}",
        columns()
    );
    let row = sqlx::query(&sql)
        .bind(channel.id)
        .bind(endpoint.unpack())
        .bind(channel.created_by)
        .bind(channel.kind.as_str())
        .bind(channel.name.as_str())
        .bind(channel.enabled)
        .bind(channel.trigger.as_str())
        .bind(channel.config.to_string())
        .bind(
            channel
                .sealed
                .as_ref()
                .map(|sealed| sealed.key_id().to_column()),
        )
        .bind(
            channel
                .sealed
                .as_ref()
                .map(|sealed| sealed.nonce().to_vec()),
        )
        .bind(
            channel
                .sealed
                .as_ref()
                .map(|sealed| sealed.ciphertext().to_vec()),
        )
        .bind(channel.template.as_deref())
        .bind(channel.per_hour)
        .fetch_one(&mut **tx)
        .await
        .map_err(classify)?;
    channel_from_row(&row)
}

fn channel_from_row(row: &PgRow) -> Result<Channel, NotifyStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    let trigger: String = row.try_get("trigger").map_err(classify)?;
    let condition: Option<String> = row.try_get("condition").map_err(classify)?;
    let config: String = row.try_get("config").map_err(classify)?;
    Ok(Channel {
        id: row.try_get("id").map_err(classify)?,
        endpoint_id: row.try_get("endpoint_id").map_err(classify)?,
        kind: ChannelKind::parse(&kind)?,
        name: row.try_get("name").map_err(classify)?,
        enabled: row.try_get("enabled").map_err(classify)?,
        trigger: Trigger::parse(&trigger)?,
        condition: condition
            .map(|text| parse_json(&text, "channel condition"))
            .transpose()?,
        config: parse_json(&config, "channel config")?,
        template: row.try_get("template").map_err(classify)?,
        per_hour: row.try_get("per_hour").map_err(classify)?,
        created_by: row.try_get("created_by").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
    })
}
