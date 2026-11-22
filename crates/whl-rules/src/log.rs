use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::kind::StepKind;

pub const MAX_DETAIL_BYTES: usize = 1024;
pub const MAX_OUTPUT_BYTES: usize = 4096;
pub const MAX_LOG_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Failed,
    Skipped,
    Deferred,
    NoOp,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Deferred => "deferred",
            Self::NoOp => "no_op",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "ok" => Self::Ok,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            "deferred" => Self::Deferred,
            "no_op" => Self::NoOp,
            _ => return None,
        })
    }
}

#[derive(Debug, Error)]
#[error("a stored run log entry has the wrong shape")]
pub struct LogReadError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleEntry {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepEntry {
    pub rule_id: Uuid,
    pub step: usize,
    pub name: Option<String>,
    pub kind: &'static str,
    pub outcome: Outcome,
    pub duration_ms: u64,
    pub detail: Option<String>,
    pub output: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Entry {
    Rule(RuleEntry),
    Step(StepEntry),
}

impl Entry {
    pub fn from_value(value: &Value) -> Result<Self, LogReadError> {
        let map = value.as_object().ok_or(LogReadError)?;
        let rule_id = map
            .get("rule_id")
            .and_then(Value::as_str)
            .and_then(|text| Uuid::try_parse(text).ok())
            .ok_or(LogReadError)?;
        if let Some(step) = map.get("step") {
            let kind = map
                .get("kind")
                .and_then(Value::as_str)
                .and_then(StepKind::parse)
                .ok_or(LogReadError)?;
            let outcome = map
                .get("outcome")
                .and_then(Value::as_str)
                .and_then(Outcome::parse)
                .ok_or(LogReadError)?;
            return Ok(Self::Step(StepEntry {
                rule_id,
                step: step
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or(LogReadError)?,
                name: map.get("name").and_then(Value::as_str).map(str::to_owned),
                kind: kind.as_str(),
                outcome,
                duration_ms: map
                    .get("duration_ms")
                    .and_then(Value::as_u64)
                    .ok_or(LogReadError)?,
                detail: map.get("detail").and_then(Value::as_str).map(str::to_owned),
                output: map
                    .get("output")
                    .filter(|output| !output.is_null())
                    .cloned(),
            }));
        }
        Ok(Self::Rule(RuleEntry {
            rule_id,
            rule_name: map
                .get("rule_name")
                .and_then(Value::as_str)
                .ok_or(LogReadError)?
                .to_owned(),
            matched: map
                .get("matched")
                .and_then(Value::as_bool)
                .ok_or(LogReadError)?,
            error: map.get("error").and_then(Value::as_str).map(str::to_owned),
        }))
    }
}

pub fn entries_from_value(value: &Value) -> Result<Vec<Entry>, LogReadError> {
    value
        .as_array()
        .ok_or(LogReadError)?
        .iter()
        .map(Entry::from_value)
        .collect()
}

pub fn bound_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

pub fn bound_output(value: Value) -> Value {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() <= MAX_OUTPUT_BYTES => value,
        _ => json!({"truncated": true}),
    }
}

fn encoded_size(entries: &[Entry]) -> usize {
    serde_json::to_vec(entries).map_or(usize::MAX, |bytes| bytes.len())
}

pub fn fit_entries(mut entries: Vec<Entry>, max: usize) -> Vec<Entry> {
    if encoded_size(&entries) <= max {
        return entries;
    }
    for entry in &mut entries {
        if let Entry::Step(step) = entry {
            step.output = None;
        }
    }
    if encoded_size(&entries) <= max {
        return entries;
    }
    for entry in &mut entries {
        if let Entry::Step(step) = entry {
            step.detail = None;
        }
    }
    while encoded_size(&entries) > max && entries.pop().is_some() {}
    entries
}
