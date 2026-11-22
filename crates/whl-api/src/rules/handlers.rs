use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use whl_core::endpoint_id::EndpointId;
use whl_forward::{TargetCheckError, vet_target};
use whl_rules::{
    CompiledRules, HTTP_BEFORE_CUT_MESSAGE, References, Rule, RuleLimits, RuleSource, StepScope,
    http_before_cut, load_set, outbound_targets, validate_set_shape,
};
use whl_script::{EngineLimits, ScriptCompiler};
use whl_store::Uuid;
use whl_store::rules::{RuleChange, RuleRecord, RuleSet, RuleStoreError, WriteOutcome};

use super::dto::{RuleDto, RuleInputDoc, RuleOrderInputDoc, RuleSetDto};
use super::input::{order_violation, parse_object, parse_order, violations};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

const WRITE_ATTEMPTS: usize = 3;
const ORDER_PARSE_CEILING: usize = 1024;

enum Mutation {
    Create(Map<String, Value>),
    Replace(Uuid, Map<String, Value>),
    Remove(Uuid),
    Reorder(Vec<Uuid>),
}

#[derive(Clone)]
enum Change {
    Create,
    Replace(Uuid),
    Remove(Uuid),
    Reorder(Vec<Uuid>),
}

#[derive(Clone)]
struct Plan {
    stored: Vec<Value>,
    stored_ids: Vec<Uuid>,
    ids: Vec<Option<Uuid>>,
    inputs: Vec<Value>,
    candidate: Option<usize>,
    change: Change,
    limits: RuleLimits,
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/rules",
    operation_id = "rules_list",
    tag = "Rules",
    summary = "List the rules of an endpoint",
    description = "The rules in execution order with the endpoint's `version`. Session only: a Bearer token answers 403 `session_required`. Needs a viewer on the endpoint.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    responses(
        (status = 200, description = "The rules", body = RuleSetDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<RuleSetDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let set = state.store.list_rules(&endpoint).await?;
    Ok(Json(RuleSetDto::from(&set)))
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/rules/{rule_id}",
    operation_id = "rules_show",
    tag = "Rules",
    summary = "Get one rule",
    description = "Session only: a Bearer token answers 403 `session_required`. Needs a viewer on the endpoint. A malformed rule id answers 404 `not_found`, an unknown one 404 `rule_not_found`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("rule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Rule id, lowercase hyphenated UUID")
    ),
    responses(
        (status = 200, description = "The rule", body = RuleDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint or rule id; `rule_not_found`: no such rule on this endpoint", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    post,
    path = "/endpoints/{id}/rules",
    operation_id = "rules_create",
    tag = "Rules",
    summary = "Append a rule",
    description = "The new rule is appended last. Editor role; session only. The number of rules is capped by the tier (`limit_reached`, `error.limit` = `{name: \"rules\", max}`). Every change emits `endpoint_updated` and bumps `version`.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    request_body(content = RuleInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The created rule", body = RuleDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`, field codes `required`, `invalid_format`, `too_long`, `out_of_range`, `too_many`, `duplicate`, `not_allowed`, `target_refused`, `target_unresolvable`); `limit_reached`: the endpoint already has the tier's rules", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    patch,
    path = "/endpoints/{id}/rules/{rule_id}",
    operation_id = "rules_update",
    tag = "Rules",
    summary = "Change a rule",
    description = "Partial: a field that is absent keeps its value; `condition` and `steps`, when given, replace the whole value. An empty body answers 400 `invalid_json`. Editor role; session only. Every change emits `endpoint_updated` and bumps `version`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("rule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Rule id, lowercase hyphenated UUID")
    ),
    request_body(content = RuleInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The changed rule", body = RuleDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint or rule id; `rule_not_found`: no such rule on this endpoint", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`, same field codes as create)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/rules/{rule_id}",
    operation_id = "rules_delete",
    tag = "Rules",
    summary = "Delete a rule",
    description = "Editor role; session only. The other rules keep their order. Every change emits `endpoint_updated` and bumps `version`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("rule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Rule id, lowercase hyphenated UUID")
    ),
    responses(
        (status = 204, description = "The rule is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint or rule id; `rule_not_found`: no such rule on this endpoint", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    put,
    path = "/endpoints/{id}/rules/order",
    operation_id = "rules_reorder",
    tag = "Rules",
    summary = "Reorder the rules",
    description = "`ids` must list every rule of the endpoint exactly once. Violations answer 422 `validation_failed` with `error.fields`: `required` when `ids` is absent, `too_many` above 1024 entries, `invalid_format` for a value that is not a lowercase UUID or a list that is not a permutation of the endpoint's rules, `duplicate` for a repeated id; unknown fields are `not_allowed`. Editor role; session only. Every change emits `endpoint_updated` and bumps `version`.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    request_body(content = RuleOrderInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The rules in the new order", body = RuleSetDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: the role is too low; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
        let owned = plan.clone();
        let rules = tokio::task::spawn_blocking(move || {
            let rules = validate_set_shape(&owned.inputs, &references, &scripts, owned.limits)
                .map_err(|error| {
                    ApiError::Validation(violations(error.diagnostics, owned.candidate))
                })?;
            refuse_new_http_before_cut(&owned, &rules, &scripts)?;
            Ok::<Vec<Rule>, ApiError>(rules)
        })
        .await
        .map_err(|_| ApiError::InternalFault("the rule validation did not finish"))??;
        vet_candidate(state, &rules, plan.candidate).await?;
        let required = candidate_secrets(&rules, plan.candidate, scripts).await?;
        let change = change(&plan, &rules)?;
        match state
            .store
            .write_rules_requiring(
                endpoint,
                state.settings.retention,
                current.version,
                &change,
                &required,
            )
            .await?
        {
            WriteOutcome::SecretsMissing(_) => continue,
            WriteOutcome::Written(set) => return Ok(set),
            WriteOutcome::Stale => continue,
            WriteOutcome::EndpointGone => return Err(ApiError::EndpointGone),
            WriteOutcome::RuleNotFound => return Err(ApiError::RuleNotFound),
            WriteOutcome::OrderMismatch => return Err(order_violation()),
        }
    }
    Err(ApiError::StoreUnavailable)
}

async fn candidate_secrets(
    rules: &[Rule],
    candidate: Option<usize>,
    scripts: ScriptCompiler,
) -> Result<BTreeSet<String>, ApiError> {
    let Some(index) = candidate else {
        return Ok(BTreeSet::new());
    };
    let sources: Vec<RuleSource> = rules
        .iter()
        .zip(1u128..)
        .map(|(rule, number)| RuleSource {
            id: Uuid::from_u128(number),
            rule: Rule {
                enabled: true,
                ..rule.clone()
            },
        })
        .collect();
    tokio::task::spawn_blocking(move || {
        let compiled = CompiledRules::compile(sources, &scripts)
            .map_err(|_| ApiError::InternalFault("the validated rule set did not compile"))?;
        compiled
            .rules()
            .get(index)
            .map(|rule| rule.secret_names().clone())
            .ok_or(ApiError::InternalFault("the changed rule is missing"))
    })
    .await
    .map_err(|_| ApiError::InternalFault("the rule compilation did not finish"))?
}

async fn rule_limits(state: &ApiState, endpoint: &EndpointId) -> Result<RuleLimits, ApiError> {
    let tier = state.endpoint_tier(endpoint).await?;
    Ok(RuleLimits {
        max_rules: usize::try_from(tier.limits().rules_per_endpoint).unwrap_or(usize::MAX),
        max_steps: usize::try_from(tier.limits().steps_per_rule).unwrap_or(usize::MAX),
        steps: StepScope::Every,
    })
}

fn refuse_new_http_before_cut(
    plan: &Plan,
    rules: &[Rule],
    scripts: &ScriptCompiler,
) -> Result<(), ApiError> {
    let stored = load_set(&plan.stored, scripts)
        .map_err(|_| ApiError::InternalFault("the stored rules could not be read"))?;
    let known: BTreeSet<(Uuid, usize)> = http_before_cut(&stored)
        .into_iter()
        .filter_map(|slot| Some((*plan.stored_ids.get(slot.rule)?, slot.step)))
        .collect();
    let found: Vec<FieldViolation> = http_before_cut(rules)
        .into_iter()
        .filter(|slot| {
            let tolerated = plan
                .ids
                .get(slot.rule)
                .copied()
                .flatten()
                .is_some_and(|id| known.contains(&(id, slot.step)));
            plan.candidate == Some(slot.rule) || !tolerated
        })
        .map(|slot| {
            let field = if plan.candidate == Some(slot.rule) {
                format!("steps.{}.kind", slot.step)
            } else {
                format!("rules.{}.steps.{}.kind", slot.rule, slot.step)
            };
            FieldViolation::new(field, FieldCode::NotAllowed, HTTP_BEFORE_CUT_MESSAGE)
        })
        .collect();
    if found.is_empty() {
        Ok(())
    } else {
        Err(ApiError::Validation(found))
    }
}

fn plan(current: &RuleSet, mutation: &Mutation, limits: RuleLimits) -> Result<Plan, ApiError> {
    let mut inputs: Vec<Value> = current.rules.iter().map(input_of).collect();
    let stored = inputs.clone();
    let stored_ids: Vec<Uuid> = current.rules.iter().map(|rule| rule.id).collect();
    let mut member_ids: Vec<Option<Uuid>> = stored_ids.iter().copied().map(Some).collect();
    match mutation {
        Mutation::Create(object) => {
            if current.rules.len() >= limits.max_rules {
                return Err(ApiError::RuleLimit {
                    max: limits.max_rules,
                });
            }
            inputs.push(Value::Object(object.clone()));
            member_ids.push(None);
            let candidate = inputs.len() - 1;
            Ok(Plan {
                stored,
                stored_ids,
                ids: member_ids,
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
                stored,
                stored_ids,
                ids: member_ids,
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
            member_ids.remove(index);
            Ok(Plan {
                stored,
                stored_ids,
                ids: member_ids,
                inputs,
                candidate: None,
                change: Change::Remove(*id),
                limits: RuleLimits::UNBOUNDED,
            })
        }
        Mutation::Reorder(ids) => {
            let order = order_indexes(current, ids)?;
            let reordered = order.iter().map(|index| inputs[*index].clone()).collect();
            let reordered_ids = order.iter().map(|index| member_ids[*index]).collect();
            Ok(Plan {
                stored,
                stored_ids,
                ids: reordered_ids,
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
