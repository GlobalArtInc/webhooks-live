use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use super::quota::{QuotaCap, charge_caps};
use super::{NotifyStoreError, classify};
use crate::Store;
use crate::forward::TIMESTAMP_SQL;

const VERIFIED_SQL: &str = "(ea.verified_at IS NOT NULL AND (NOT ea.idp_verified \
     OR (u.email_verified AND lower(u.email) = lower(ea.address))))";

const ADDRESS_KEY_SQL: &str = "encode(sha256(convert_to($1, 'UTF8')), 'hex')";

const REFUND_SQL: &str = "UPDATE notification_counters SET count = GREATEST(count - 1, 0) \
     WHERE subject = $1 \
       AND window_start = to_timestamp(floor(extract(epoch FROM now()) / $2::int8) * $2::int8)";

const GLOBAL_SUBJECT: &str = "vg:all";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailAddress {
    pub id: Uuid,
    pub address: String,
    pub verified: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressLimits {
    pub max_addresses: i64,
    pub verifications_per_user_per_day: i32,
    pub verifications_per_address_per_day: i32,
    pub verifications_per_domain_per_day: i32,
    pub verifications_per_day_global: i32,
    pub resend_cooldown_secs: i64,
}

#[derive(Debug, Clone)]
pub struct AddressRequest<'a> {
    pub id: Uuid,
    pub address: &'a str,
    pub key: &'a str,
    pub domain: &'a str,
    pub token_hash: &'a [u8],
    pub lifetime: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreviousToken {
    Absent,
    Held {
        hash: Option<Vec<u8>>,
        expires_at: Option<OffsetDateTime>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressUndo {
    address_id: Uuid,
    issued_hash: Vec<u8>,
    previous: PreviousToken,
    refunds: Vec<QuotaCap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressOutcome {
    Created {
        address: EmailAddress,
        verification: Option<AddressUndo>,
    },
    Resent {
        address: EmailAddress,
        undo: AddressUndo,
    },
    Existing(EmailAddress),
    LimitReached,
    Throttled,
    UserGone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailRecipient {
    pub address: String,
    pub user_id: Uuid,
}

fn address_fields() -> String {
    format!(
        "ea.id, ea.address, {VERIFIED_SQL} AS verified, \
         to_char(ea.created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at"
    )
}

fn address_from_row(row: &PgRow) -> Result<EmailAddress, NotifyStoreError> {
    Ok(EmailAddress {
        id: row.try_get("id").map_err(classify)?,
        address: row.try_get("address").map_err(classify)?,
        verified: row.try_get("verified").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
    })
}

async fn fetch_address(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<EmailAddress, NotifyStoreError> {
    let sql = format!(
        "SELECT {} FROM email_addresses ea JOIN users u ON u.id = ea.user_id WHERE ea.id = $1",
        address_fields()
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(classify)?;
    address_from_row(&row)
}

impl Store {
    pub async fn list_email_addresses(
        &self,
        user: Uuid,
    ) -> Result<Vec<EmailAddress>, NotifyStoreError> {
        let sql = format!(
            "SELECT {} FROM email_addresses ea JOIN users u ON u.id = ea.user_id \
             WHERE ea.user_id = $1 ORDER BY ea.created_at, ea.id",
            address_fields()
        );
        let rows = sqlx::query(&sql)
            .bind(user)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(address_from_row).collect()
    }

    pub async fn find_verified_address(
        &self,
        user: Uuid,
        address: Uuid,
    ) -> Result<Option<EmailAddress>, NotifyStoreError> {
        let sql = format!(
            "SELECT {} FROM email_addresses ea JOIN users u ON u.id = ea.user_id \
             WHERE ea.id = $2 AND ea.user_id = $1 AND {VERIFIED_SQL}",
            address_fields()
        );
        let row = sqlx::query(&sql)
            .bind(user)
            .bind(address)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(address_from_row).transpose()
    }

    pub async fn request_email_address(
        &self,
        user: Uuid,
        request: &AddressRequest<'_>,
        limits: &AddressLimits,
    ) -> Result<AddressOutcome, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let identity = sqlx::query(
            "SELECT lower(email) = lower($2) AND email_verified AS own \
             FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(user)
        .bind(request.address)
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify)?;
        let Some(identity) = identity else {
            return Ok(AddressOutcome::UserGone);
        };
        let own: bool = identity.try_get("own").map_err(classify)?;
        let lookup = format!(
            "SELECT {} FROM email_addresses ea JOIN users u ON u.id = ea.user_id \
             WHERE ea.user_id = $1 AND lower(ea.address) = lower($2)",
            address_fields()
        );
        let existing = sqlx::query(&lookup)
            .bind(user)
            .bind(request.address)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?
            .as_ref()
            .map(address_from_row)
            .transpose()?;
        let key: String = sqlx::query_scalar(&format!("SELECT {ADDRESS_KEY_SQL}"))
            .bind(request.key)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let mut caps = vec![
            QuotaCap::daily(
                format!("vu:{}", user.hyphenated()),
                limits.verifications_per_user_per_day,
            ),
            QuotaCap::daily(
                format!("va:{key}"),
                limits.verifications_per_address_per_day,
            ),
            QuotaCap::daily(
                format!("vd:{}", request.domain),
                limits.verifications_per_domain_per_day,
            ),
            QuotaCap::daily(
                GLOBAL_SUBJECT.to_owned(),
                limits.verifications_per_day_global,
            ),
        ];
        if let Some(existing) = existing {
            if existing.verified {
                return Ok(AddressOutcome::Existing(existing));
            }
            caps.push(QuotaCap::windowed(
                format!("vc:{}", existing.id.hyphenated()),
                1,
                limits.resend_cooldown_secs,
            ));
            if !charge_caps(&mut tx, &caps).await? {
                return Ok(AddressOutcome::Throttled);
            }
            let previous = sqlx::query(
                "SELECT verify_token_hash, verify_expires_at FROM email_addresses WHERE id = $1",
            )
            .bind(existing.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
            let previous = PreviousToken::Held {
                hash: previous.try_get("verify_token_hash").map_err(classify)?,
                expires_at: previous.try_get("verify_expires_at").map_err(classify)?,
            };
            sqlx::query(
                "UPDATE email_addresses SET verify_token_hash = $2, \
                   verify_expires_at = now() + make_interval(secs => $3) \
                 WHERE id = $1",
            )
            .bind(existing.id)
            .bind(request.token_hash)
            .bind(request.lifetime.as_secs_f64())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
            let reissued = fetch_address(&mut tx, existing.id).await?;
            tx.commit().await.map_err(classify)?;
            return Ok(AddressOutcome::Resent {
                address: reissued,
                undo: AddressUndo {
                    address_id: existing.id,
                    issued_hash: request.token_hash.to_vec(),
                    previous,
                    refunds: caps,
                },
            });
        }
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM email_addresses WHERE user_id = $1")
                .bind(user)
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?;
        if held >= limits.max_addresses {
            return Ok(AddressOutcome::LimitReached);
        }
        let mut verification = None;
        if own {
            sqlx::query(
                "INSERT INTO email_addresses (id, user_id, address, verified_at, idp_verified) \
                 VALUES ($1, $2, $3, now(), true)",
            )
            .bind(request.id)
            .bind(user)
            .bind(request.address)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        } else {
            caps.push(QuotaCap::windowed(
                format!("vc:{}", request.id.hyphenated()),
                1,
                limits.resend_cooldown_secs,
            ));
            if !charge_caps(&mut tx, &caps).await? {
                return Ok(AddressOutcome::Throttled);
            }
            sqlx::query(
                "INSERT INTO email_addresses \
                   (id, user_id, address, verify_token_hash, verify_expires_at) \
                 VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))",
            )
            .bind(request.id)
            .bind(user)
            .bind(request.address)
            .bind(request.token_hash)
            .bind(request.lifetime.as_secs_f64())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
            verification = Some(AddressUndo {
                address_id: request.id,
                issued_hash: request.token_hash.to_vec(),
                previous: PreviousToken::Absent,
                refunds: caps,
            });
        }
        let created = fetch_address(&mut tx, request.id).await?;
        tx.commit().await.map_err(classify)?;
        Ok(AddressOutcome::Created {
            address: created,
            verification,
        })
    }

    pub async fn undo_email_request(&self, undo: &AddressUndo) -> Result<(), NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        match &undo.previous {
            PreviousToken::Absent => {
                sqlx::query(
                    "DELETE FROM email_addresses \
                     WHERE id = $1 AND verified_at IS NULL AND verify_token_hash = $2",
                )
                .bind(undo.address_id)
                .bind(undo.issued_hash.as_slice())
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
            }
            PreviousToken::Held { hash, expires_at } => {
                sqlx::query(
                    "UPDATE email_addresses SET verify_token_hash = $3, verify_expires_at = $4 \
                     WHERE id = $1 AND verify_token_hash = $2",
                )
                .bind(undo.address_id)
                .bind(undo.issued_hash.as_slice())
                .bind(hash.as_deref())
                .bind(*expires_at)
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
            }
        }
        for cap in &undo.refunds {
            sqlx::query(REFUND_SQL)
                .bind(cap.subject.as_str())
                .bind(cap.window_secs)
                .execute(&mut *tx)
                .await
                .map_err(classify)?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(())
    }

    pub async fn verify_email_address(
        &self,
        token_hash: &[u8],
    ) -> Result<Option<EmailAddress>, NotifyStoreError> {
        let sql = format!(
            "UPDATE email_addresses SET verified_at = now(), idp_verified = false, \
               verify_token_hash = NULL, verify_expires_at = NULL \
             WHERE verify_token_hash = $1 AND verify_expires_at > now() \
             RETURNING id, address, true AS verified, \
               to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at"
        );
        let row = sqlx::query(&sql)
            .bind(token_hash)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(address_from_row).transpose()
    }

    pub async fn delete_email_address(
        &self,
        user: Uuid,
        address: Uuid,
    ) -> Result<bool, NotifyStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let deleted = sqlx::query("DELETE FROM email_addresses WHERE id = $1 AND user_id = $2")
            .bind(address)
            .bind(user)
            .execute(&mut *tx)
            .await
            .map_err(classify)?
            .rows_affected();
        if deleted == 1 {
            sqlx::query(
                "UPDATE notification_channels SET enabled = false \
                 WHERE kind = 'email' AND created_by = $1 AND config ->> 'address_id' = $2::uuid::text",
            )
            .bind(user)
            .bind(address)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(deleted == 1)
    }

    pub async fn verified_recipient(
        &self,
        channel: Uuid,
    ) -> Result<Option<MailRecipient>, NotifyStoreError> {
        let sql = format!(
            "SELECT ea.address, ea.user_id \
             FROM notification_channels ch \
             JOIN email_addresses ea \
               ON ea.id::text = ch.config ->> 'address_id' AND ea.user_id = ch.created_by \
             JOIN users u ON u.id = ea.user_id \
             JOIN endpoints e ON e.id = ch.endpoint_id \
             WHERE ch.id = $1 AND ch.kind = 'email' AND {VERIFIED_SQL} \
               AND (e.owner_id = ch.created_by OR EXISTS ( \
                 SELECT 1 FROM endpoint_members m \
                 WHERE m.endpoint_id = e.id AND m.user_id = ch.created_by \
                   AND m.role = 'editor'))"
        );
        let row = sqlx::query(&sql)
            .bind(channel)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.map(|row| {
            Ok(MailRecipient {
                address: row.try_get("address").map_err(classify)?,
                user_id: row.try_get("user_id").map_err(classify)?,
            })
        })
        .transpose()
    }

    pub async fn endpoint_is_owned(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<bool>, NotifyStoreError> {
        sqlx::query_scalar(
            "SELECT owner_id IS NOT NULL FROM endpoints \
             WHERE id = $1 AND deleted_at IS NULL \
               AND last_active_at > now() - make_interval(secs => $2)",
        )
        .bind(endpoint.unpack())
        .bind(retention.as_secs_f64())
        .fetch_optional(self.pool())
        .await
        .map_err(classify)
    }

    pub async fn disable_email_channel(&self, channel: Uuid) -> Result<bool, NotifyStoreError> {
        let disabled = sqlx::query(
            "UPDATE notification_channels SET enabled = false WHERE id = $1 AND kind = 'email'",
        )
        .bind(channel)
        .execute(self.pool())
        .await
        .map_err(classify)?
        .rows_affected();
        Ok(disabled == 1)
    }
}
