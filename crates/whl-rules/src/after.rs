use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::compiled::CompiledRule;
use crate::model::{Condition, Step};

pub const PLAN_VERSION: u32 = 1;
pub const MAX_PLAN_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Resume {
    pub rule: usize,
    pub step: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub vars: Map<String, Value>,
    pub payload: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeferredStep {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub step: usize,
    pub action: Value,
    pub snapshot: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleSnapshot<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub condition: &'a Condition,
    pub steps: &'a [Step],
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("the plan cannot be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the plan is larger than {MAX_PLAN_BYTES} bytes")]
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct PlanDocument<'a> {
    version: u32,
    rules: Vec<RuleSnapshot<'a>>,
    resume: Option<Resume>,
    snapshots: &'a [Snapshot],
    deferred: &'a [DeferredStep],
    payload: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct AfterPart {
    pub snapshots: Vec<Snapshot>,
    pub deferred: Vec<DeferredStep>,
    pub resume: Option<Resume>,
    pub vars: Map<String, Value>,
    pub payload: Option<String>,
}

impl AfterPart {
    pub fn is_empty(&self) -> bool {
        self.deferred.is_empty() && self.resume.is_none()
    }

    pub fn plan(&self, rules: &[CompiledRule]) -> Result<Value, PlanError> {
        let (remaining, resume) = match self.resume {
            Some(resume) => (
                rules
                    .iter()
                    .skip(resume.rule)
                    .map(|rule| RuleSnapshot {
                        id: rule.id,
                        name: &rule.name,
                        condition: &rule.source.condition,
                        steps: &rule.source.steps,
                    })
                    .collect(),
                Some(Resume {
                    rule: 0,
                    step: resume.step,
                }),
            ),
            None => (Vec::new(), None),
        };
        let document = PlanDocument {
            version: PLAN_VERSION,
            rules: remaining,
            resume,
            snapshots: &self.snapshots,
            deferred: &self.deferred,
            payload: self.payload.as_deref(),
        };
        let encoded = serde_json::to_vec(&document)?;
        if encoded.len() > MAX_PLAN_BYTES {
            return Err(PlanError::TooLarge);
        }
        Ok(serde_json::to_value(&document)?)
    }
}
