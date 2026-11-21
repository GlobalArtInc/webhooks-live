use serde_json::{Map, Value, json};
use thiserror::Error;
use uuid::Uuid;
use whl_script::ScriptCompiler;

use crate::after::{DeferredStep, Snapshot};
use crate::compiled::RuleSource;
use crate::error::RuleError;
use crate::set::load_set;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRule {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub condition: Value,
    pub steps: Value,
}

#[derive(Debug, Error)]
pub enum PlanReadError {
    #[error("the plan has no rules array")]
    MissingRules,
    #[error("a rule of the plan has the wrong shape")]
    Malformed,
}

pub fn load_stored(
    records: &[StoredRule],
    scripts: &ScriptCompiler,
) -> Result<Vec<RuleSource>, RuleError> {
    let inputs: Vec<Value> = records
        .iter()
        .map(|record| {
            json!({
                "name": record.name,
                "enabled": record.enabled,
                "condition": record.condition,
                "steps": record.steps,
            })
        })
        .collect();
    let rules = load_set(&inputs, scripts)?;
    Ok(rules
        .into_iter()
        .zip(records)
        .map(|(rule, record)| RuleSource {
            id: record.id,
            rule,
        })
        .collect())
}

pub fn rules_from_plan(plan: &Value) -> Result<Vec<StoredRule>, PlanReadError> {
    let items = plan
        .get("rules")
        .and_then(Value::as_array)
        .ok_or(PlanReadError::MissingRules)?;
    items
        .iter()
        .map(|item| -> Result<StoredRule, PlanReadError> {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::try_parse(text).ok())
                .ok_or(PlanReadError::Malformed)?;
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or(PlanReadError::Malformed)?
                .to_owned();
            Ok(StoredRule {
                id,
                name,
                enabled: true,
                condition: item.get("condition").cloned().unwrap_or_else(|| json!({})),
                steps: item.get("steps").cloned().ok_or(PlanReadError::Malformed)?,
            })
        })
        .collect()
}

pub fn snapshots_from_plan(plan: &Value) -> Result<Vec<Snapshot>, PlanReadError> {
    let Some(items) = plan.get("snapshots") else {
        return Ok(Vec::new());
    };
    items
        .as_array()
        .ok_or(PlanReadError::Malformed)?
        .iter()
        .map(|item| -> Result<Snapshot, PlanReadError> {
            let vars: Map<String, Value> = match item.get("vars") {
                Some(Value::Object(vars)) => vars.clone(),
                None | Some(Value::Null) => Map::new(),
                Some(_) => return Err(PlanReadError::Malformed),
            };
            let payload = match item.get("payload") {
                Some(Value::String(text)) => Some(text.clone()),
                None | Some(Value::Null) => None,
                Some(_) => return Err(PlanReadError::Malformed),
            };
            Ok(Snapshot { vars, payload })
        })
        .collect()
}

pub fn deferred_from_plan(plan: &Value) -> Result<Vec<DeferredStep>, PlanReadError> {
    let Some(items) = plan.get("deferred") else {
        return Ok(Vec::new());
    };
    let items = items.as_array().ok_or(PlanReadError::Malformed)?;
    items
        .iter()
        .map(|item| -> Result<DeferredStep, PlanReadError> {
            let rule_id = item
                .get("rule_id")
                .and_then(Value::as_str)
                .and_then(|text| Uuid::try_parse(text).ok())
                .ok_or(PlanReadError::Malformed)?;
            let rule_name = item
                .get("rule_name")
                .and_then(Value::as_str)
                .ok_or(PlanReadError::Malformed)?
                .to_owned();
            let step = item
                .get("step")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .ok_or(PlanReadError::Malformed)?;
            let action = item
                .get("action")
                .filter(|action| action.is_object())
                .cloned()
                .ok_or(PlanReadError::Malformed)?;
            let snapshot = item
                .get("snapshot")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .ok_or(PlanReadError::Malformed)?;
            Ok(DeferredStep {
                rule_id,
                rule_name,
                step,
                action,
                snapshot,
            })
        })
        .collect()
}

pub fn plan_resume_step(plan: &Value) -> usize {
    plan.get("resume")
        .and_then(|resume| resume.get("step"))
        .and_then(Value::as_u64)
        .and_then(|step| usize::try_from(step).ok())
        .unwrap_or(0)
}

pub fn plan_payload(plan: &Value) -> Option<String> {
    plan.get("payload")
        .and_then(Value::as_str)
        .map(str::to_owned)
}
