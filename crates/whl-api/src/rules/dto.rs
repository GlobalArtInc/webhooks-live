use serde::Serialize;
use serde_json::Value;
use whl_store::rules::{RuleRecord, RuleSet};

#[derive(Serialize)]
pub struct RuleDto {
    id: String,
    position: i16,
    name: String,
    enabled: bool,
    condition: Value,
    steps: Value,
    created_at: String,
    updated_at: String,
}

impl From<&RuleRecord> for RuleDto {
    fn from(rule: &RuleRecord) -> Self {
        Self {
            id: rule.id.hyphenated().to_string(),
            position: rule.position,
            name: rule.name.clone(),
            enabled: rule.enabled,
            condition: rule.condition.clone(),
            steps: rule.steps.clone(),
            created_at: rule.created_at.clone(),
            updated_at: rule.updated_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct RuleSetDto {
    items: Vec<RuleDto>,
    version: i32,
}

impl From<&RuleSet> for RuleSetDto {
    fn from(set: &RuleSet) -> Self {
        Self {
            items: set.rules.iter().map(RuleDto::from).collect(),
            version: set.version,
        }
    }
}
