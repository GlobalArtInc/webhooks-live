use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use whl_core::endpoint_id::EndpointId;
use whl_forward::{TargetCheckError, vet_target};
use whl_rules::{References, Rule, RuleLimits, StepScope, outbound_targets, validate_set_with};
use whl_script::{EngineLimits, ScriptCompiler};
use whl_store::Uuid;
use whl_store::rules::{RuleChange, RuleRecord, RuleSet, RuleStoreError, WriteOutcome};

use super::dto::{RuleDto, RuleSetDto};
use super::input::{order_violation, parse_object, parse_order, violations};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const WRITE_ATTEMPTS: usize = 3;
const ORDER_PARSE_CEILING: usize = 1024;

enum Mutation {
    Create(Map<String, Value>),
    Replace(Uuid, Map<String, Value>),
    Remove(Uuid),
    Reorder(Vec<Uuid>),
}

enum Change {
    Create,
    Replace(Uuid),
    Remove(Uuid),
    Reorder(Vec<Uuid>),
}

struct Plan {
    inputs: Vec<Value>,
    candidate: Option<usize>,
    change: Change,
    limits: RuleLimits,
}

pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<RuleSetDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let set = state.store.list_rules(&endpoint).await?;
    Ok(Json(RuleSetDto::from(&set)))
}

pub async fn show(
    State(state): State<ApiState>,
    Path((raw_id, raw_rule)): Path<(String, String)>,
) -> Result<Json<RuleDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let rule_id = rule_id(&raw_rule)?;
    live_endpoint(&state, &endpoint).await?;
    let set = state.store.list_rules(&endpoint).await?;
    let rule = set
        .rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .ok_or(ApiError::RuleNotFound)?;
    Ok(Json(RuleDto::from(rule)))
}

pub async fn create(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let object = parse_object(&body)?;
    let set = commit(&state, &endpoint, Mutation::Create(object)).await?;
    let created = set.rules.last().ok_or_else(|| {
        ApiError::RuleStorage(RuleStoreError::Invalid(
            "the created rule is missing".to_owned(),
        ))
    })?;
    Ok((StatusCode::CREATED, Json(RuleDto::from(created))).into_response())
}

pub async fn update(
    State(state): State<ApiState>,
    Path((raw_id, raw_rule)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<RuleDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let rule_id = rule_id(&raw_rule)?;
    live_endpoint(&state, &endpoint).await?;
    let object = parse_object(&body)?;
    let set = commit(&state, &endpoint, Mutation::Replace(rule_id, object)).await?;
    let rule = set
        .rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .ok_or(ApiError::RuleNotFound)?;
    Ok(Json(RuleDto::from(rule)))
}

pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_rule)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let rule_id = rule_id(&raw_rule)?;
    live_endpoint(&state, &endpoint).await?;
    commit(&state, &endpoint, Mutation::Remove(rule_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<RuleSetDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let ids = parse_order(&body, ORDER_PARSE_CEILING)?;
    let set = commit(&state, &endpoint, Mutation::Reorder(ids)).await?;
    Ok(Json(RuleSetDto::from(&set)))
}

async fn commit(
    state: &ApiState,
    endpoint: &EndpointId,
    mutation: Mutation,
) -> Result<RuleSet, ApiError> {
    let scripts = ScriptCompiler::new(EngineLimits::default()).map_err(ApiError::RuleEngine)?;
    let limits = rule_limits(state, endpoint).await?;
    for _ in 0..WRITE_ATTEMPTS {
        let current = state.store.list_rules(endpoint).await?;
        let plan = plan(&current, &mutation, limits)?;
        let references = references(state, endpoint).await?;
        let inputs = plan.inputs.clone();
        let plan_limits = plan.limits;
        let rules = tokio::task::spawn_blocking(move || {
            validate_set_with(&inputs, &references, &scripts, plan_limits)
        })
        .await
        .map_err(|_| ApiError::InternalFault("the rule validation did not finish"))?
        .map_err(|error| ApiError::Validation(violations(error.diagnostics, plan.candidate)))?;
        vet_candidate(state, &rules, plan.candidate).await?;
        let change = change(&plan, &rules)?;
        match state
            .store
            .write_rules(endpoint, state.settings.retention, current.version, &change)
            .await?
        {
            WriteOutcome::Written(set) => return Ok(set),
            WriteOutcome::Stale => continue,
            WriteOutcome::EndpointGone => return Err(ApiError::EndpointGone),
            WriteOutcome::RuleNotFound => return Err(ApiError::RuleNotFound),
            WriteOutcome::OrderMismatch => return Err(order_violation()),
        }
    }
    Err(ApiError::StoreUnavailable)
}

async fn rule_limits(state: &ApiState, endpoint: &EndpointId) -> Result<RuleLimits, ApiError> {
    let tier = state.endpoint_tier(endpoint).await?;
    Ok(RuleLimits {
        max_rules: usize::try_from(tier.limits().rules_per_endpoint).unwrap_or(usize::MAX),
        max_steps: usize::try_from(tier.limits().steps_per_rule).unwrap_or(usize::MAX),
        steps: StepScope::Every,
    })
}

fn plan(current: &RuleSet, mutation: &Mutation, limits: RuleLimits) -> Result<Plan, ApiError> {
    let mut inputs: Vec<Value> = current.rules.iter().map(input_of).collect();
    match mutation {
        Mutation::Create(object) => {
            if current.rules.len() >= limits.max_rules {
                return Err(ApiError::RuleLimit {
                    max: limits.max_rules,
                });
            }
            inputs.push(Value::Object(object.clone()));
            let candidate = inputs.len() - 1;
            Ok(Plan {
                candidate: Some(candidate),
                inputs,
                change: Change::Create,
                limits: RuleLimits {
                    steps: StepScope::Only(candidate),
                    ..limits
                },
            })
        }
        Mutation::Replace(id, object) => {
            let index = index_of(current, *id)?;
            inputs[index] = merged(&inputs[index], object);
            let grown = step_count(&inputs[index]) > step_count(&input_of(&current.rules[index]));
            Ok(Plan {
                inputs,
                candidate: Some(index),
                change: Change::Replace(*id),
                limits: RuleLimits {
                    steps: if grown {
                        StepScope::Only(index)
                    } else {
                        StepScope::Nothing
                    },
                    ..RuleLimits::UNBOUNDED
                }
                .with_step_cap(limits.max_steps),
            })
        }
        Mutation::Remove(id) => {
            let index = index_of(current, *id)?;
            inputs.remove(index);
            Ok(Plan {
                inputs,
                candidate: None,
                change: Change::Remove(*id),
                limits: RuleLimits::UNBOUNDED,
            })
        }
        Mutation::Reorder(ids) => {
            let order = order_indexes(current, ids)?;
            let reordered = order.iter().map(|index| inputs[*index].clone()).collect();
            Ok(Plan {
                inputs: reordered,
                candidate: None,
                change: Change::Reorder(ids.clone()),
                limits: RuleLimits::UNBOUNDED,
            })
        }
    }
}

fn change(plan: &Plan, rules: &[Rule]) -> Result<RuleChange, ApiError> {
    let candidate = || {
        plan.candidate
            .and_then(|index| rules.get(index))
            .cloned()
            .ok_or_else(|| {
                ApiError::RuleStorage(RuleStoreError::Invalid(
                    "the changed rule is missing".to_owned(),
                ))
            })
    };
    Ok(match &plan.change {
        Change::Create => RuleChange::Create(candidate()?),
        Change::Replace(id) => RuleChange::Replace {
            id: *id,
            rule: candidate()?,
        },
        Change::Remove(id) => RuleChange::Remove(*id),
        Change::Reorder(ids) => RuleChange::Reorder(ids.clone()),
    })
}

fn step_count(input: &Value) -> usize {
    input
        .get("steps")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn input_of(rule: &RuleRecord) -> Value {
    json!({
        "name": rule.name,
        "enabled": rule.enabled,
        "condition": rule.condition,
        "steps": rule.steps,
    })
}

fn merged(base: &Value, patch: &Map<String, Value>) -> Value {
    let mut merged = match base {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    for (key, value) in patch {
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

fn index_of(current: &RuleSet, id: Uuid) -> Result<usize, ApiError> {
    current
        .rules
        .iter()
        .position(|rule| rule.id == id)
        .ok_or(ApiError::RuleNotFound)
}

fn order_indexes(current: &RuleSet, ids: &[Uuid]) -> Result<Vec<usize>, ApiError> {
    if ids.len() != current.rules.len() {
        return Err(order_violation());
    }
    let mut indexes = Vec::with_capacity(ids.len());
    for id in ids {
        let index = current
            .rules
            .iter()
            .position(|rule| rule.id == *id)
            .ok_or_else(order_violation)?;
        if indexes.contains(&index) {
            return Err(order_violation());
        }
        indexes.push(index);
    }
    Ok(indexes)
}

pub(super) async fn references(
    state: &ApiState,
    endpoint: &EndpointId,
) -> Result<References, ApiError> {
    let secrets = state
        .store
        .list_secrets(endpoint)
        .await?
        .into_iter()
        .map(|secret| secret.name.unpack().to_owned())
        .collect();
    let forward_rules = state
        .store
        .list_forward_rules(endpoint)
        .await?
        .into_iter()
        .map(|rule| rule.id)
        .collect();
    let channels = state
        .store
        .list_channels(endpoint)
        .await?
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    Ok(References {
        secrets,
        forward_rules,
        channels,
    })
}

async fn vet_candidate(
    state: &ApiState,
    rules: &[Rule],
    candidate: Option<usize>,
) -> Result<(), ApiError> {
    let Some(index) = candidate else {
        return Ok(());
    };
    vet_prefixed(state, rules, &format!("rules.{index}.")).await
}

pub(crate) async fn vet_all(state: &ApiState, rules: &[Rule]) -> Result<(), ApiError> {
    vet_prefixed(state, rules, "").await
}

async fn vet_prefixed(state: &ApiState, rules: &[Rule], prefix: &str) -> Result<(), ApiError> {
    let mut found = Vec::new();
    for target in outbound_targets(rules) {
        let Some(field) = target.field.strip_prefix(prefix) else {
            continue;
        };
        if let Err(error) = vet_target(
            &target.url,
            &state.forward.guard,
            state.forward.lookup.as_ref(),
        )
        .await
        {
            found.push(target_violation(field, &error));
        }
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(ApiError::Validation(found))
    }
}

fn target_violation(field: &str, error: &TargetCheckError) -> FieldViolation {
    match error {
        TargetCheckError::Refused(reason) => {
            FieldViolation::new(field, FieldCode::TargetRefused, reason.to_string())
        }
        TargetCheckError::Unresolvable(detail) => {
            FieldViolation::new(field, FieldCode::TargetUnresolvable, detail.clone())
        }
    }
}

pub(super) async fn live_endpoint(state: &ApiState, endpoint: &EndpointId) -> Result<(), ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .map(|_| ())
        .ok_or(ApiError::EndpointGone)
}

pub(super) fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

fn rule_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::NotFound)
}
