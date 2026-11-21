mod common;
mod ruled;
mod served;

use std::time::{Duration, Instant};

use axum::body::Body;
use common::{Harness, request};
use ruled::{add_rule, capture_rule_state, entries, relaxed, respond, rule, run_count, run_row};
use serde_json::json;
use served::{Configured, endpoint_configured, make_templated};

async fn ruled_harness() -> Harness {
    Harness::new().await.with_rules(relaxed(), None)
}

fn json_post(id: &str, path: &str) -> axum::http::Request<Body> {
    request("POST", &format!("/{id}{path}"))
        .header("content-type", "application/json")
        .header("x-token", "abc")
        .body(Body::from(r#"{"order":{"id":42}}"#))
        .expect("request builds")
}

#[tokio::test]
async fn an_endpoint_without_rules_answers_the_static_response_and_stores_no_run() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            headers: vec![("X-Static", "yes")],
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.status, 201);
    assert_eq!(reply.body, "static");
    assert_eq!(reply.header("x-static"), Some("yes"));
    assert!(reply.header("x-webhooks-live-rules").is_none());
    assert_eq!(run_count(&harness, &id).await, 0);
}

#[tokio::test]
async fn a_respond_rule_replaces_the_response_from_request_data_and_records_the_run() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            headers: vec![("X-Static", "yes")],
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "answer",
            json!({"value": "request.method", "op": "eq", "arg": "POST"}),
            vec![json!({
                "kind": "respond",
                "status": "202",
                "content_type": "application/json",
                "headers": [{"name": "X-Echo", "value": "{{ request.header.x-token }}"}],
                "body": "{\"id\": {{ json(\"$.order.id\") }}}",
            })],
        ),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.status, 202);
    assert_eq!(reply.body, r#"{"id": 42}"#);
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(reply.header("x-echo"), Some("abc"));
    assert!(reply.header("x-static").is_none());
    assert_eq!(reply.header("content-security-policy"), Some("sandbox"));
    assert!(reply.header("x-webhooks-live-rules").is_none());
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "succeeded");
    assert_eq!(run.trigger, "capture");
    assert_eq!(run.rules_version, 1);
    assert_eq!(run.response_status, Some(202));
    assert!(!run.leased);
    assert_eq!(entries(&run.log).len(), 2);
    assert_eq!(run.log[0]["matched"], true);
    assert_eq!(run.log[1]["outcome"], "ok");
    let (rule_state, stored_status) = capture_rule_state(&harness, &capture_id).await;
    assert_eq!(rule_state.as_deref(), Some("succeeded"));
    assert_eq!(stored_status, 202);
}

#[tokio::test]
async fn a_condition_that_does_not_match_leaves_the_static_response_alone() {
    let harness = ruled_harness().await;
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
            "never",
            json!({"value": "request.method", "op": "eq", "arg": "DELETE"}),
            vec![respond("500", "nope")],
        ),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.status, 201);
    assert_eq!(reply.body, "static");
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "succeeded");
    assert_eq!(run.log[0]["matched"], false);
    assert_eq!(run.response_status, None);
    assert_eq!(harness.stored_status(&id, 1).await, 201);
}

#[tokio::test]
async fn header_steps_edit_the_configured_headers_and_keep_the_configured_body() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            headers: vec![("X-Keep", "old"), ("X-Drop", "gone"), ("X-Other", "stays")],
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "headers",
            json!({}),
            vec![
                json!({"kind": "set_header", "name": "X-Keep", "value": "new"}),
                json!({"kind": "remove_header", "name": "X-Drop"}),
                json!({"kind": "set_header", "name": "X-Added", "value": "{{ request.method }}"}),
            ],
        ),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "static");
    assert_eq!(reply.header("x-keep"), Some("new"));
    assert!(reply.header("x-drop").is_none());
    assert_eq!(reply.header("x-other"), Some("stays"));
    assert_eq!(reply.header("x-added"), Some("POST"));
}

#[tokio::test]
async fn header_steps_overlay_a_templated_response_without_rendering_it_twice() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"hello {{ request.method }}".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    make_templated(&harness, &id, None).await;
    add_rule(
        &harness,
        &id,
        rule(
            "headers",
            json!({}),
            vec![json!({"kind": "set_header", "name": "X-Added", "value": "yes"})],
        ),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.body, "hello POST");
    assert_eq!(reply.header("x-added"), Some("yes"));
}

#[tokio::test]
async fn a_delay_step_adds_to_the_configured_delay() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            delay_ms: 40,
            ..Configured::default()
        },
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule("slow", json!({}), vec![json!({"kind": "delay", "ms": 160})]),
    )
    .await;
    let started = Instant::now();

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert_eq!(reply.status, 200);
    assert!(started.elapsed() >= Duration::from_millis(190));
}

#[tokio::test]
async fn the_status_override_of_the_path_still_wins_over_a_rule() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(&harness, Configured::default()).await;
    add_rule(
        &harness,
        &id,
        rule("answer", json!({}), vec![respond("202", "rule")]),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "/404")).await;

    assert_eq!(reply.status, 404);
    assert_eq!(reply.body, "rule");
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.response_status, Some(202));
    assert_eq!(harness.stored_status(&id, 1).await, 404);
}

#[tokio::test]
async fn a_rule_cannot_set_the_reserved_diagnostic_headers() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(&harness, Configured::default()).await;
    add_rule(
        &harness,
        &id,
        rule(
            "spoof",
            json!({}),
            vec![json!({
                "kind": "respond",
                "status": "200",
                "headers": [
                    {"name": "X-Webhooks-Live-Rule-Error", "value": "none"},
                    {"name": "X-Webhooks-Live-Rules", "value": "ok"},
                ],
                "body": "x",
            })],
        ),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;

    assert!(reply.header("x-webhooks-live-rule-error").is_none());
    assert!(reply.header("x-webhooks-live-rules").is_none());
}

#[tokio::test]
async fn the_status_a_rule_serves_is_not_overwritten_by_the_rendered_status_template() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"templated".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    make_templated(&harness, &id, Some("201")).await;
    add_rule(
        &harness,
        &id,
        rule("answer", json!({}), vec![respond("202", "rule")]),
    )
    .await;

    let reply = harness.send(json_post(id.unpack(), "")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(reply.status, 202);
    assert_eq!(harness.stored_status(&id, 1).await, 202);
}

#[tokio::test]
async fn a_preflight_is_not_recorded_with_the_status_a_rule_would_have_served() {
    let harness = ruled_harness().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            cors: true,
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

    let reply = harness
        .send(
            request("OPTIONS", &format!("/{id}"))
                .header("origin", "https://app.example.test")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.status, 204);
    assert_eq!(harness.stored_status(&id, 1).await, 200);
}
