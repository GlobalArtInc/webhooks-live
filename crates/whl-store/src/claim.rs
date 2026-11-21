use std::time::Duration;

use sqlx::Row;
use sqlx::types::Uuid;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;

use crate::endpoints::{COLUMNS, EndpointError, EndpointRecord, classify, record_from_row};
use crate::store::Store;

const LOCK_SQL: &str = "SELECT e.owner_id, e.creator_net::text AS creator_net, \
        e.created_at > now() - make_interval(secs => $3) AS within_window, \
        EXISTS (SELECT 1 FROM endpoint_members m WHERE m.endpoint_id = e.id AND m.user_id = $2) AS is_member \
    FROM endpoints e \
    WHERE e.id = $1 AND e.deleted_at IS NULL \
        AND e.last_active_at > now() - make_interval(secs => $4) \
    FOR UPDATE OF e";

const DROP_MEMBERS_SQL: &str = "DELETE FROM endpoint_members WHERE endpoint_id = $1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimRefusal {
    NetworkUnknown,
    NetworkMismatch,
    WindowExpired,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaimOutcome {
    Claimed(Box<EndpointRecord>),
    AlreadyOwned,
    Gone,
    Refused(ClaimRefusal),
}

impl Store {
    pub async fn claim_endpoint(
        &self,
        endpoint: &EndpointId,
        user_id: Uuid,
        caller_net: Option<&CreatorNet>,
        retention: Duration,
        window: Duration,
    ) -> Result<ClaimOutcome, EndpointError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let Some(row) = sqlx::query(LOCK_SQL)
            .bind(endpoint.unpack())
            .bind(user_id)
            .bind(window.as_secs_f64())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?
        else {
            return Ok(ClaimOutcome::Gone);
        };
        let owner: Option<Uuid> = row.try_get("owner_id").map_err(EndpointError::Query)?;
        let is_member: bool = row.try_get("is_member").map_err(EndpointError::Query)?;
        if let Some(owner) = owner {
            return Ok(if owner == user_id || is_member {
                ClaimOutcome::AlreadyOwned
            } else {
                ClaimOutcome::Gone
            });
        }
        let stored: Option<String> = row.try_get("creator_net").map_err(EndpointError::Query)?;
        let within_window: bool = row.try_get("within_window").map_err(EndpointError::Query)?;
        let stored = stored
            .map(|text| {
                text.parse::<CreatorNet>()
                    .map_err(|error| EndpointError::Invalid {
                        id: endpoint.unpack().to_owned(),
                        reason: format!("stored creator network {text:?}: {error}"),
                    })
            })
            .transpose()?;
        let (Some(stored), Some(caller_net)) = (stored, caller_net) else {
            return Ok(ClaimOutcome::Refused(ClaimRefusal::NetworkUnknown));
        };
        if stored != *caller_net {
            return Ok(ClaimOutcome::Refused(ClaimRefusal::NetworkMismatch));
        }
        if !within_window {
            return Ok(ClaimOutcome::Refused(ClaimRefusal::WindowExpired));
        }
        sqlx::query(DROP_MEMBERS_SQL)
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let update = format!(
            "UPDATE endpoints SET owner_id = $2, creator_net = NULL \
             WHERE id = $1 AND owner_id IS NULL RETURNING {COLUMNS}"
        );
        let Some(claimed) = sqlx::query(&update)
            .bind(endpoint.unpack())
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?
        else {
            return Ok(ClaimOutcome::AlreadyOwned);
        };
        let record = record_from_row(&claimed, retention)?;
        tx.commit().await.map_err(classify)?;
        Ok(ClaimOutcome::Claimed(Box::new(record)))
    }
}
