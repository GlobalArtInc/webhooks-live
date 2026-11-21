#![allow(dead_code)]

use std::time::Duration;

use serde_json::{Value, json};
use sqlx::Row;
use whl_capture::RulesSettings;
use whl_core::endpoint_id::EndpointId;
use whl_rules::{References, validate_set};
use whl_script::{EngineLimits, ScriptCompiler};
use whl_store::rules::{RuleChange, WriteOutcome};
use whl_store::{Store, Uuid};

use crate::common::Harness;

const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn relaxed() -> RulesSettings {
    RulesSettings {
        variables_deadline: Duration::from_secs(2),
        plan_deadline: Duration::from_secs(2),
        state_timeout: Duration::from_secs(2),
        ..RulesSettings::default()
    }
}

pub fn rule(name: &str, condition: Value, steps: Vec<Value>) -> Value {
    json!({"name": name, "condition": condition, "steps": steps})
}

pub fn respond(status: &str, body: &str) -> Value {
    json!({"kind": "respond", "status": status, "body": body})
}

pub async fn add_rule(harness: &Harness, endpoint: &EndpointId, input: Value) {
    let store = Store::from_pool(harness.db.pool().clone());
    let scripts = ScriptCompiler::new(EngineLimits::default()).expect("the limits are valid");
    let mut rules =
        validate_set(&[input], &References::default(), &scripts).expect("the rule is valid");
    let rule = rules.remove(0);
    let current = store
        .list_rules(endpoint)
        .await
        .expect("the rules are listed");
    let outcome = store
        .write_rules(
            endpoint,
            RETENTION,
            current.version,
            &RuleChange::Create(rule),
        )
        .await
        .expect("the write runs");
    assert!(matches!(outcome, WriteOutcome::Written(_)), "{outcome:?}");
}

#[derive(Debug)]
pub struct RunRow {
    pub state: String,
    pub trigger: String,
    pub error: Option<String>,
    pub rules_version: i32,
    pub plan: Option<Value>,
    pub log: Value,
    pub vars: Value,
    pub response_status: Option<i16>,
    pub leased: bool,
}

fn parsed(text: &str) -> Value {
    serde_json::from_str(text).expect("the column holds JSON")
}

pub async fn run_row(harness: &Harness, capture_id: &str) -> RunRow {
    let id = Uuid::parse_str(capture_id).expect("the request id is a capture id");
    let row = sqlx::query(
        "SELECT state, trigger, error, rules_version, plan::text AS plan, log::text AS log, \
         vars::text AS vars, response_status, locked_until IS NOT NULL AS leased \
         FROM rule_runs WHERE capture_id = $1",
    )
    .bind(id)
    .fetch_one(harness.db.pool())
    .await
    .expect("the run row is readable");
    RunRow {
        state: row.get("state"),
        trigger: row.get("trigger"),
        error: row.get("error"),
        rules_version: row.get("rules_version"),
        plan: row
            .get::<Option<String>, _>("plan")
            .map(|text| parsed(&text)),
        log: parsed(&row.get::<String, _>("log")),
        vars: parsed(&row.get::<String, _>("vars")),
        response_status: row.get("response_status"),
        leased: row.get("leased"),
    }
}

pub async fn run_count(harness: &Harness, endpoint: &EndpointId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE endpoint_id = $1")
        .bind(endpoint.unpack())
        .fetch_one(harness.db.pool())
        .await
        .expect("the runs are countable")
}

pub async fn capture_rule_state(harness: &Harness, capture_id: &str) -> (Option<String>, i16) {
    let id = Uuid::parse_str(capture_id).expect("the request id is a capture id");
    sqlx::query_as("SELECT rule_state, response_status FROM captures WHERE id = $1")
        .bind(id)
        .fetch_one(harness.db.pool())
        .await
        .expect("the capture is readable")
}

pub async fn counter(harness: &Harness, endpoint: &EndpointId, name: &str) -> Option<i64> {
    let text: Option<String> = sqlx::query_scalar(
        "SELECT value::text FROM endpoint_vars WHERE endpoint_id = $1 AND name = $2",
    )
    .bind(endpoint.unpack())
    .bind(name)
    .fetch_optional(harness.db.pool())
    .await
    .expect("the variable is readable");
    text.and_then(|text| text.parse().ok())
}

pub fn entries(log: &Value) -> &Vec<Value> {
    log.as_array().expect("the log is an array")
}
