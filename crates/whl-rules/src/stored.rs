use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;
use whl_script::ScriptCompiler;

use crate::after::PLAN_VERSION;
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
    #[error("the plan has version {found:?}, this build reads version {PLAN_VERSION}")]
    Version { found: Option<u64> },
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
    let found = plan.get("version").and_then(Value::as_u64);
    if found != Some(u64::from(PLAN_VERSION)) {
        return Err(PlanReadError::Version { found });
    }
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
