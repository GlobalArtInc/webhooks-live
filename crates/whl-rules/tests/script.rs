mod common;

use std::alloc::System;
use std::time::{Duration, Instant};

use common::{Recorder, compile, context, respond, rule, run_with, step_entries};
use serde_json::json;
use whl_rules::{FailureCode, Mode, Outcome, RunOutcome};
use whl_script::{CountingAlloc, PoolConfig, ScriptPool};

#[global_allocator]
static ALLOCATOR: CountingAlloc<System> = CountingAlloc::new(System);

fn pool() -> ScriptPool {
    ScriptPool::new(PoolConfig {
        threads: 1,
        ..PoolConfig::default()
    })
    .expect("the pool starts")
}

fn script_rule(source: &str) -> serde_json::Value {
    rule(
        "scripted",
        json!({}),
        vec![
            json!({"kind": "script", "source": source}),
            respond("200", "{{ var.score }}"),
        ],
    )
}

#[tokio::test]
async fn a_script_sets_variables_and_replaces_the_response() {
    let rules = compile(vec![rule(
        "scripted",
        json!({}),
        vec![
            json!({
                "kind": "script",
                "source": r#"#{ vars: #{ score: 7 }, response: #{ status: 418, body: "from " + request.method } }"#,
            }),
            json!({"kind": "delay", "ms": 0}),
        ],
    )]);
    let pool = pool();

    let finished = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::default(),
        Some(&pool),
    )
    .await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.vars["score"], 7);
    assert_eq!(step_entries(&finished)[0].outcome, Outcome::Ok);
    assert_eq!(finished.response.status, Some(418));
    assert_eq!(
        finished.response.body.as_deref(),
        Some(b"from POST".as_slice())
    );
}

#[tokio::test]
async fn a_script_result_is_visible_to_the_templates_after_it() {
    let rules = compile(vec![script_rule(
        r#"#{ vars: #{ score: request.headers["x-token"].len() } }"#,
    )]);
    let pool = pool();

    let finished = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::default(),
        Some(&pool),
    )
    .await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.response.body.as_deref(), Some(b"3".as_slice()));
}

#[tokio::test]
async fn a_script_error_fails_the_run_with_the_script_code() {
    let rules = compile(vec![script_rule(r#"throw "refused";"#)]);
    let pool = pool();

    let finished = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::default(),
        Some(&pool),
    )
    .await;

    match &finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Script),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}

#[tokio::test]
async fn an_endless_script_is_stopped_inside_the_budget() {
    let rules = compile(vec![script_rule("loop {}")]);
    let pool = pool();
    let started = Instant::now();

    let finished = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::default(),
        Some(&pool),
    )
    .await;

    assert!(started.elapsed() < Duration::from_secs(4));
    match &finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Script),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}

#[tokio::test]
async fn a_script_step_without_a_pool_fails_instead_of_being_skipped() {
    let rules = compile(vec![script_rule("#{ vars: #{ score: 1 } }")]);

    let finished = run_with(&rules, context(), Mode::Live, &Recorder::default(), None).await;

    match &finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Script),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}
