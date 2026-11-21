mod common;
mod ruled;
mod served;

use std::alloc::System;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::Harness;
use ruled::{add_rule, entries, relaxed, rule, run_row};
use serde_json::json;
use served::{Configured, endpoint_configured};
use whl_script::{CountingAlloc, PoolConfig, ScriptPool};

#[global_allocator]
static ALLOCATOR: CountingAlloc<System> = CountingAlloc::new(System);

fn pool() -> Arc<ScriptPool> {
    Arc::new(
        ScriptPool::new(PoolConfig {
            threads: 1,
            ..PoolConfig::default()
        })
        .expect("the pool starts"),
    )
}

fn scripted(source: &str) -> serde_json::Value {
    rule(
        "scripted",
        json!({}),
        vec![
            json!({"kind": "script", "source": source}),
            json!({"kind": "delay", "ms": 0}),
        ],
    )
}

#[tokio::test]
async fn a_script_shapes_the_response_inside_the_sandbox_and_leaves_its_variables_in_the_run() {
    let harness = Harness::new().await.with_rules(relaxed(), Some(pool()));
    let id = endpoint_configured(&harness, Configured::default()).await;
    add_rule(
        &harness,
        &id,
        scripted(
            r#"#{ vars: #{ seen: request.method }, response: #{ status: 418, body: "tea for " + request.method } }"#,
        ),
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_eq!(reply.status, 418);
    assert_eq!(reply.body, "tea for POST");
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "succeeded");
    assert_eq!(run.vars, json!({"seen": "POST"}));
    assert_eq!(entries(&run.log)[1]["kind"], "script");
}

#[tokio::test]
async fn an_endless_script_is_stopped_and_the_static_response_is_served_with_the_script_code() {
    let harness = Harness::new().await.with_rules(relaxed(), Some(pool()));
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(&harness, &id, scripted("loop {}")).await;
    let started = Instant::now();

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "static");
    assert_eq!(reply.header("x-webhooks-live-rules"), Some("error"));
    assert_eq!(reply.header("x-webhooks-live-rule-error"), Some("script"));
}
