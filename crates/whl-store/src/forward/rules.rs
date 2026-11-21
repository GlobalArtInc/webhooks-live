use std::collections::HashMap;
use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{Row, Transaction};
use uuid::Uuid;
use whl_core::endpoint_id::EndpointId;

use super::{
    ForwardStoreError, TIMESTAMP_SQL, classify, lock_live_endpoint, notify_endpoint_updated,
};
use crate::Store;

pub const MAX_RULES_PER_ENDPOINT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRule {
    pub id: Uuid,
    pub enabled: bool,
    pub target_url: String,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub append_path: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewForwardRule {
    pub enabled: bool,
    pub target_url: String,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub append_path: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForwardRulePatch {
    pub enabled: Option<bool>,
    pub target_url: Option<String>,
    pub method: Option<Option<String>>,
    pub content_type: Option<Option<String>>,
    pub header_allowlist: Option<Vec<String>>,
    pub append_path: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateRuleOutcome {
    Created(ForwardRule),
    EndpointGone,
    LimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRuleOutcome {
    Updated(ForwardRule),
    EndpointGone,
    RuleNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteRuleOutcome {
    Deleted,
    EndpointGone,
    RuleNotFound,
}

fn columns() -> String {
    format!(
        "id, enabled, target_url, method, content_type, header_allowlist, append_path, \
         to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at"
    )
}

impl Store {
    pub async fn list_forward_rules(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<ForwardRule>, ForwardStoreError> {
        let sql = format!(
            "SELECT {} FROM forward_rules WHERE endpoint_id = $1 ORDER BY created_at, position",
            columns()
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(rule_from_row).collect()
    }

    pub async fn list_forward_rules_for(
        &self,
        endpoints: &[EndpointId],
    ) -> Result<HashMap<EndpointId, Vec<ForwardRule>>, ForwardStoreError> {
        let mut grouped: HashMap<EndpointId, Vec<ForwardRule>> = HashMap::new();
        if endpoints.is_empty() {
            return Ok(grouped);
        }
        let ids: Vec<&str> = endpoints.iter().map(EndpointId::unpack).collect();
        let sql = format!(
            "SELECT endpoint_id, {} FROM forward_rules WHERE endpoint_id = ANY($1) \
             ORDER BY endpoint_id, created_at, position",
            columns()
        );
        let rows = sqlx::query(&sql)
            .bind(&ids)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        for row in &rows {
            let owner: String = row.try_get("endpoint_id").map_err(classify)?;
            let owner = EndpointId::parse(&owner).map_err(|error| {
                ForwardStoreError::Invalid(format!("stored endpoint id {owner:?}: {error}"))
            })?;
            grouped.entry(owner).or_default().push(rule_from_row(row)?);
        }
        Ok(grouped)
    }

    pub async fn create_forward_rule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        rule: &NewForwardRule,
    ) -> Result<CreateRuleOutcome, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(CreateRuleOutcome::EndpointGone);
        }
        let Some(position) = free_position(&mut tx, endpoint).await? else {
            return Ok(CreateRuleOutcome::LimitReached);
        };
        let sql = format!(
            "INSERT INTO forward_rules \
             (id, endpoint_id, position, enabled, target_url, method, content_type, \
              header_allowlist, append_path) \
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8) RETURNING {}",
            columns()
        );
        let row = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(position)
            .bind(rule.enabled)
            .bind(rule.target_url.as_str())
            .bind(rule.method.as_deref())
            .bind(rule.content_type.as_deref())
            .bind(&rule.header_allowlist)
            .bind(rule.append_path)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let created = rule_from_row(&row)?;
        notify_endpoint_updated(&mut tx, endpoint).await?;
        tx.commit().await.map_err(classify)?;
        Ok(CreateRuleOutcome::Created(created))
    }

    pub async fn update_forward_rule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        rule_id: Uuid,
        patch: &ForwardRulePatch,
    ) -> Result<UpdateRuleOutcome, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(UpdateRuleOutcome::EndpointGone);
        }
        let select = format!(
            "SELECT {} FROM forward_rules WHERE id = $1 AND endpoint_id = $2 FOR UPDATE",
            columns()
        );
        let Some(row) = sqlx::query(&select)
            .bind(rule_id)
            .bind(endpoint.unpack())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?
        else {
            return Ok(UpdateRuleOutcome::RuleNotFound);
        };
        let merged = merge(rule_from_row(&row)?, patch);
        let update = format!(
            "UPDATE forward_rules SET enabled = $3, target_url = $4, method = $5, \
             content_type = $6, header_allowlist = $7, append_path = $8 \
             WHERE id = $1 AND endpoint_id = $2 RETURNING {}",
            columns()
        );
        let row = sqlx::query(&update)
            .bind(rule_id)
            .bind(endpoint.unpack())
            .bind(merged.enabled)
            .bind(merged.target_url.as_str())
            .bind(merged.method.as_deref())
            .bind(merged.content_type.as_deref())
            .bind(&merged.header_allowlist)
            .bind(merged.append_path)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let updated = rule_from_row(&row)?;
        notify_endpoint_updated(&mut tx, endpoint).await?;
        tx.commit().await.map_err(classify)?;
        Ok(UpdateRuleOutcome::Updated(updated))
    }

    pub async fn delete_forward_rule(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        rule_id: Uuid,
    ) -> Result<DeleteRuleOutcome, ForwardStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if !lock_live_endpoint(&mut tx, endpoint, retention).await? {
            return Ok(DeleteRuleOutcome::EndpointGone);
        }
        let deleted = sqlx::query("DELETE FROM forward_rules WHERE id = $1 AND endpoint_id = $2")
            .bind(rule_id)
            .bind(endpoint.unpack())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        if deleted.rows_affected() == 0 {
            return Ok(DeleteRuleOutcome::RuleNotFound);
        }
        notify_endpoint_updated(&mut tx, endpoint).await?;
        tx.commit().await.map_err(classify)?;
        Ok(DeleteRuleOutcome::Deleted)
    }
}

fn merge(mut rule: ForwardRule, patch: &ForwardRulePatch) -> ForwardRule {
    if let Some(enabled) = patch.enabled {
        rule.enabled = enabled;
    }
    if let Some(target_url) = &patch.target_url {
        rule.target_url.clone_from(target_url);
    }
    if let Some(method) = &patch.method {
        rule.method.clone_from(method);
    }
    if let Some(content_type) = &patch.content_type {
        rule.content_type.clone_from(content_type);
    }
    if let Some(header_allowlist) = &patch.header_allowlist {
        rule.header_allowlist.clone_from(header_allowlist);
    }
    if let Some(append_path) = patch.append_path {
        rule.append_path = append_path;
    }
    rule
}

pub(crate) async fn insert_rule_with_id(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    endpoint: &EndpointId,
    id: Uuid,
    position: i16,
    rule: &NewForwardRule,
) -> Result<(), ForwardStoreError> {
    sqlx::query(
        "INSERT INTO forward_rules \
         (id, endpoint_id, position, enabled, target_url, method, content_type, \
          header_allowlist, append_path) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(endpoint.unpack())
    .bind(position)
    .bind(rule.enabled)
    .bind(rule.target_url.as_str())
    .bind(rule.method.as_deref())
    .bind(rule.content_type.as_deref())
    .bind(&rule.header_allowlist)
    .bind(rule.append_path)
    .execute(&mut **tx)
    .await
    .map_err(classify)?;
    Ok(())
}

async fn free_position(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    endpoint: &EndpointId,
) -> Result<Option<i16>, ForwardStoreError> {
    let taken: Vec<i16> =
        sqlx::query_scalar("SELECT position FROM forward_rules WHERE endpoint_id = $1")
            .bind(endpoint.unpack())
            .fetch_all(&mut **tx)
            .await
            .map_err(classify)?;
    let limit = i16::try_from(MAX_RULES_PER_ENDPOINT)
        .map_err(|_| ForwardStoreError::Invalid("the rule limit exceeds i16".to_owned()))?;
    Ok((1..=limit).find(|position| !taken.contains(position)))
}

fn rule_from_row(row: &PgRow) -> Result<ForwardRule, ForwardStoreError> {
    Ok(ForwardRule {
        id: row.try_get("id").map_err(classify)?,
        enabled: row.try_get("enabled").map_err(classify)?,
        target_url: row.try_get("target_url").map_err(classify)?,
        method: row.try_get("method").map_err(classify)?,
        content_type: row.try_get("content_type").map_err(classify)?,
        header_allowlist: row.try_get("header_allowlist").map_err(classify)?,
        append_path: row.try_get("append_path").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
    })
}
