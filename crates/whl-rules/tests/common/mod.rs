#![allow(dead_code)]

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;
use whl_rules::{
    CompiledRules, DriveEnv, Entry, Finished, Machine, MachineInput, Mode, References, RuleSource,
    STATE_STEP_TIMEOUT, StateBackend, StateError, StoreCall, StoreValue, drive, validate_set,
};
use whl_script::{DEFAULT_MEMORY_BYTES, EngineLimits, ScriptCompiler, ScriptPool};
use whl_template::{BodyData, Context, NameValue, RandomSeed};

pub const BUDGET: Duration = Duration::from_secs(5);

pub fn compiler() -> ScriptCompiler {
    ScriptCompiler::new(EngineLimits::default()).expect("the default limits are valid")
}

pub fn rule_id(index: usize) -> Uuid {
    Uuid::from_u128(index as u128 + 1)
}

pub fn compile(inputs: Vec<Value>) -> Arc<CompiledRules> {
    let scripts = compiler();
    let valid =
        validate_set(&inputs, &References::default(), &scripts).expect("the rules are valid");
    let sources = valid
        .into_iter()
        .enumerate()
        .map(|(index, rule)| RuleSource {
            id: rule_id(index),
            rule,
        })
        .collect();
    Arc::new(CompiledRules::compile(sources, &scripts).expect("the rules compile"))
}

pub fn rule(name: &str, condition: Value, steps: Vec<Value>) -> Value {
    json!({"name": name, "condition": condition, "steps": steps})
}

pub fn respond(status: &str, body: &str) -> Value {
    json!({"kind": "respond", "status": status, "body": body})
}

pub fn context() -> Context {
    let mut context = Context::new(OffsetDateTime::UNIX_EPOCH);
    context.request.method = "POST".to_owned();
    context.request.path = "/orders/42".to_owned();
    context.request.segments = vec!["orders".to_owned(), "42".to_owned()];
    context.request.query = vec![NameValue::new("page", "3")];
    context.request.headers = vec![
        NameValue::new("X-Token", "abc"),
        NameValue::new("Content-Type", "application/json"),
    ];
    context.request.body = BodyData::from_text(r#"{"order":{"id":42,"tag":"gift"}}"#);
    context.request.ip = "203.0.113.9".to_owned();
    context.request.content_type = "application/json".to_owned();
    context.request.kind = "http".to_owned();
    context.endpoint.id = "ep".to_owned();
    context
}

#[derive(Default)]
pub struct Recorder {
    calls: Mutex<Vec<StoreCall>>,
    failure: Option<StateError>,
    counter: Mutex<i64>,
}

impl Recorder {
    pub fn failing(error: StateError) -> Self {
        Self {
            failure: Some(error),
            ..Self::default()
        }
    }

    pub fn calls(&self) -> Vec<StoreCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl StateBackend for Recorder {
    async fn write(&self, call: StoreCall) -> Result<StoreValue, StateError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call.clone());
        if let Some(error) = self.failure {
            return Err(error);
        }
        match call {
            StoreCall::Set { .. } => Ok(StoreValue::Written),
            StoreCall::Add { by, .. } => {
                let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
                *counter += by;
                Ok(StoreValue::Counter(*counter))
            }
        }
    }
}

pub fn machine(
    rules: &Arc<CompiledRules>,
    context: Context,
    mode: Mode,
    budget: Duration,
) -> Machine {
    Machine::new(MachineInput {
        rules: Arc::clone(rules),
        context,
        seed: RandomSeed::from_bytes(b"fixture seed"),
        budget,
        mode,
        tenant: "ep".to_owned(),
    })
}

pub async fn run_with<S: StateBackend>(
    rules: &Arc<CompiledRules>,
    context: Context,
    mode: Mode,
    state: &S,
    pool: Option<&ScriptPool>,
) -> Finished {
    let env = DriveEnv {
        pool,
        state,
        script_memory_bytes: DEFAULT_MEMORY_BYTES,
        state_timeout: STATE_STEP_TIMEOUT,
        slice_hook: None,
    };
    drive(machine(rules, context, mode, BUDGET), &env, BUDGET, ()).await
}

pub async fn run(rules: &Arc<CompiledRules>, context: Context) -> Finished {
    run_with(rules, context, Mode::Live, &Recorder::default(), None).await
}

pub fn step_entries(finished: &Finished) -> Vec<&whl_rules::StepEntry> {
    finished
        .log
        .iter()
        .filter_map(|entry| match entry {
            Entry::Step(step) => Some(step),
            Entry::Rule(_) => None,
        })
        .collect()
}

pub fn rule_entries(finished: &Finished) -> Vec<&whl_rules::RuleEntry> {
    finished
        .log
        .iter()
        .filter_map(|entry| match entry {
            Entry::Rule(rule) => Some(rule),
            Entry::Step(_) => None,
        })
        .collect()
}
