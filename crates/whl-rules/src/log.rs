use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleEntry {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub matched: bool,
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
