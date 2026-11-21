use std::time::Duration;

use sqlx::types::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_rules::Rule;

use crate::endpoints::{
    EndpointError, EndpointRecord, NewEndpoint, classify, insert_in_transaction,
};
use crate::forward::{
    ForwardStoreError, MAX_RULES_PER_ENDPOINT, NewForwardRule, insert_rule_with_id,
    notify_endpoint_updated,
};
use crate::lifecycle::LifecycleError;
use crate::notify::{MAX_CHANNELS_PER_ENDPOINT, NewChannel, NotifyStoreError, insert_channel};
use crate::rules::{RuleStoreError, insert, refresh_flags};
use crate::schedules::{NewSchedule, ScheduleStoreError, count_for_owner, insert_schedule};
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClonedForwardRule {
    pub id: Uuid,
    pub rule: NewForwardRule,
}

#[derive(Debug, Clone, Default)]
pub struct EndpointParts {
    pub forward_rules: Vec<ClonedForwardRule>,
    pub rules: Vec<Rule>,
    pub channels: Vec<NewChannel>,
    pub schedules: Vec<NewSchedule>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewClonedEndpoint<'a> {
    pub id: &'a EndpointId,
    pub endpoint: NewEndpoint<'a>,
    pub parts: &'a EndpointParts,
    pub max_schedules_per_owner: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloneOutcome {
    Created(Box<EndpointRecord>),
    ScheduleLimitReached,
    PartLimitExceeded,
}

#[derive(Debug, thiserror::Error)]
pub enum CloneError {
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
    #[error(transparent)]
    Forward(#[from] ForwardStoreError),
    #[error(transparent)]
    Rule(#[from] RuleStoreError),
    #[error(transparent)]
    Notify(#[from] NotifyStoreError),
    #[error(transparent)]
    Schedule(#[from] ScheduleStoreError),
}

impl From<EndpointError> for CloneError {
    fn from(error: EndpointError) -> Self {
        Self::Lifecycle(LifecycleError::Endpoint(error))
    }
}

impl Store {
    pub async fn create_endpoint_with_parts(
        &self,
        new: &NewClonedEndpoint<'_>,
        retention: Duration,
    ) -> Result<CloneOutcome, CloneError> {
        let parts = new.parts;
        if parts.forward_rules.len() > MAX_RULES_PER_ENDPOINT
            || parts.rules.len() > whl_rules::MAX_RULES_PER_ENDPOINT
            || parts.channels.len() > MAX_CHANNELS_PER_ENDPOINT
        {
            return Ok(CloneOutcome::PartLimitExceeded);
        }
        let mut tx = self.pool().begin().await.map_err(classify)?;
        if let Some(owner) = new.endpoint.owner
            && !parts.schedules.is_empty()
        {
            sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
                .bind(owner)
                .fetch_optional(&mut *tx)
                .await
                .map_err(classify)?;
        }
        let record = insert_in_transaction(&mut tx, &new.endpoint, retention).await?;
        for (index, cloned) in parts.forward_rules.iter().enumerate() {
            let position = i16::try_from(index + 1).map_err(|_| {
                ForwardStoreError::Invalid("the forward rule position exceeds i16".to_owned())
            })?;
            insert_rule_with_id(&mut tx, new.id, cloned.id, position, &cloned.rule).await?;
        }
        if !parts.rules.is_empty() {
            for rule in &parts.rules {
                insert(&mut tx, new.id, rule).await?;
            }
            refresh_flags(&mut tx, new.id).await?;
        }
        for channel in &parts.channels {
            insert_channel(&mut tx, new.id, channel).await?;
        }
        if !parts.schedules.is_empty() {
            let Some(owner) = new.endpoint.owner else {
                return Ok(CloneOutcome::ScheduleLimitReached);
            };
            let held = count_for_owner(&mut tx, owner).await?;
            let wanted = i64::try_from(parts.schedules.len()).unwrap_or(i64::MAX);
            let ceiling = i64::try_from(new.max_schedules_per_owner).unwrap_or(i64::MAX);
            if held.saturating_add(wanted) > ceiling {
                return Ok(CloneOutcome::ScheduleLimitReached);
            }
            for schedule in &parts.schedules {
                insert_schedule(&mut tx, new.id, schedule).await?;
            }
        }
        if !parts.forward_rules.is_empty() || !parts.rules.is_empty() {
            notify_endpoint_updated(&mut tx, new.id).await?;
        }
        tx.commit().await.map_err(classify)?;
        Ok(CloneOutcome::Created(Box::new(record)))
    }
}
