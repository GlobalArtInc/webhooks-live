mod common;
mod ruled;
mod served;

use common::Harness;
use ruled::{add_rule, capture_rule_state, relaxed, respond, rule, run_row};
use serde_json::json;
use served::{Configured, endpoint_configured};
use whl_capture::RulesSettings;

#[tokio::test]
async fn work_after_the_response_is_left_in_a_queued_run_with_its_plan() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "first",
            json!({}),
            vec![
                json!({"kind": "set_var", "into": "who", "value": "{{ request.method }}"}),
                json!({"kind": "forward", "target_url": "https://example.test/hook"}),
                respond("202", "ok"),
                json!({"kind": "set_var", "into": "late", "value": "1"}),
            ],
        ),
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "second",
            json!({}),
            vec![json!({"kind": "set_var", "into": "other", "value": "2"})],
        ),
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_eq!(reply.status, 202);
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "queued");
    assert!(!run.leased);
    assert_eq!(run.vars, json!({"who": "POST"}));
    let plan = run.plan.expect("the after-response plan is stored");
    assert_eq!(plan["version"], 1);
    assert_eq!(plan["resume"], json!({"rule": 0, "step": 3}));
    assert_eq!(plan["rules"].as_array().map(Vec::len), Some(2));
    assert_eq!(plan["deferred"][0]["action"]["kind"], "forward");
    assert_eq!(plan["deferred"][0]["snapshot"], 0);
    assert_eq!(plan["snapshots"][0]["vars"], json!({"who": "POST"}));
    let (rule_state, status) = capture_rule_state(&harness, &capture_id).await;
    assert_eq!(rule_state.as_deref(), Some("running"));
    assert_eq!(status, 202);
}

#[tokio::test]
async fn a_rule_set_without_a_response_step_adds_no_latency_and_is_queued_whole() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "count",
            json!({}),
            vec![json!({"kind": "increment", "counter": "hits", "by": 1})],
        ),
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_eq!(reply.status, 201);
    assert_eq!(reply.body, "static");
    assert!(reply.header("x-webhooks-live-rules").is_none());
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "queued");
    assert!(!run.leased);
    let plan = run.plan.expect("the whole rule set is the plan");
    assert_eq!(plan["resume"], json!({"rule": 0, "step": 0}));
    assert_eq!(plan["rules"][0]["name"], "count");
    assert_eq!(
        capture_rule_state(&harness, &capture_id).await.0.as_deref(),
        Some("running")
    );
    let counted: Option<i64> =
        sqlx::query_scalar("SELECT count(*) FROM endpoint_vars WHERE endpoint_id = $1")
            .bind(id.unpack())
            .fetch_one(harness.db.pool())
            .await
            .ok();
    assert_eq!(counted, Some(0));
}

#[tokio::test]
async fn a_disabled_rules_engine_inserts_no_run_and_serves_the_static_response() {
    let harness = Harness::new().await.with_rules(
        RulesSettings {
            enabled: false,
            ..relaxed()
        },
        None,
    );
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule("answer", json!({}), vec![respond("202", "rule")]),
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_eq!(reply.status, 201);
    assert_eq!(reply.body, "static");
    assert!(reply.header("x-webhooks-live-rules").is_none());
    assert_eq!(ruled::run_count(&harness, &id).await, 0);
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    assert_eq!(capture_rule_state(&harness, &capture_id).await, (None, 201));
}
