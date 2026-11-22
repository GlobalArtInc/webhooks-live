mod common;
mod ruled;
mod served;

use std::time::{Duration, Instant};

use axum::body::Body;
use common::{Harness, Reply, request, send};
use ruled::{
    add_rule, add_stored_rule, capture_rule_state, entries, relaxed, respond, rule, run_count,
    run_row,
};
use serde_json::{Value, json};
use served::{Configured, endpoint_configured};
use tokio::task::JoinSet;
use whl_capture::RulesSettings;
use whl_core::endpoint_id::EndpointId;

async fn static_endpoint(harness: &Harness) -> EndpointId {
    endpoint_configured(
        harness,
        Configured {
            status: 201,
            headers: vec![("X-Static", "yes")],
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await
}

fn assert_fallback(reply: &Reply, code: &str) {
    assert_eq!(reply.status, 201);
    assert_eq!(reply.body, "static");
    assert_eq!(reply.header("x-static"), Some("yes"));
    assert_eq!(reply.header("x-webhooks-live-rules"), Some("error"));
    assert_eq!(reply.header("x-webhooks-live-rule-error"), Some(code));
}

fn heavy_rule() -> Value {
    let body = "{{ request.body | base64 | hex | sha256 | hex | base64 | hex }}".repeat(400);
    rule("heavy", json!({}), vec![respond("202", &body)])
}

fn heavy_request(id: &str) -> axum::http::Request<Body> {
    request("POST", &format!("/{id}"))
        .body(Body::from("x".repeat(60 * 1024)))
        .expect("request builds")
}

#[tokio::test]
async fn a_render_error_serves_the_static_response_with_the_template_code_and_a_failed_run() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = static_endpoint(&harness).await;
    add_rule(
        &harness,
        &id,
        rule(
            "broken",
            json!({}),
            vec![respond("202", "{{ request.query.x | base64_decode }}")],
        ),
    )
    .await;

    let reply = harness.get(&format!("/{id}?x=%21%21")).await;

    assert_fallback(&reply, "template");
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "failed");
    assert!(
        run.error
            .as_deref()
            .is_some_and(|error| error.starts_with("template"))
    );
    assert!(run.plan.is_none());
    assert_eq!(run.response_status, None);
    assert_eq!(entries(&run.log)[1]["outcome"], "failed");
    let (rule_state, status) = capture_rule_state(&harness, &capture_id).await;
    assert_eq!(rule_state.as_deref(), Some("failed"));
    assert_eq!(status, 201);
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn a_slow_template_overruns_its_budget_and_falls_back_inside_the_deadline() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = static_endpoint(&harness).await;
    add_rule(&harness, &id, heavy_rule()).await;
    let started = Instant::now();

    let reply = harness.send(heavy_request(id.unpack())).await;

    assert!(started.elapsed() < Duration::from_secs(3));
    assert_fallback(&reply, "budget");
    let capture_id = reply
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "failed");
    assert!(
        run.error
            .as_deref()
            .is_some_and(|error| error.starts_with("budget"))
    );
}

#[tokio::test]
async fn a_stored_http_step_before_the_response_fails_the_run_with_the_http_code() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = static_endpoint(&harness).await;
    add_stored_rule(
        &harness,
        &id,
        rule(
            "fetch",
            json!({}),
            vec![
                json!({"kind": "http", "method": "GET", "url": "https://example.test/"}),
                respond("202", "never"),
            ],
        ),
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_fallback(&reply, "http");
}

#[tokio::test]
async fn a_state_step_that_cannot_apply_fails_the_run_unless_it_continues_on_error() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let strict = static_endpoint(&harness).await;
    let lenient = static_endpoint(&harness).await;
    let steps = |forgiven: bool| {
        vec![
            json!({"kind": "set_store", "name": "label", "value": "text"}),
            json!({"kind": "increment", "counter": "label", "by": 1, "continue_on_error": forgiven}),
            respond("202", "after"),
        ]
    };
    add_rule(&harness, &strict, rule("strict", json!({}), steps(false))).await;
    add_rule(&harness, &lenient, rule("lenient", json!({}), steps(true))).await;

    let refused = harness.post(&format!("/{strict}"), "x").await;
    let forgiven = harness.post(&format!("/{lenient}"), "x").await;

    assert_fallback(&refused, "template");
    assert_eq!(forgiven.status, 202);
    assert_eq!(forgiven.body, "after");
    let capture_id = forgiven
        .header("x-request-id")
        .expect("a request id")
        .to_owned();
    let run = run_row(&harness, &capture_id).await;
    assert_eq!(run.state, "succeeded");
    assert_eq!(entries(&run.log)[2]["outcome"], "failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_sync_concurrency_answers_the_static_response_and_queues_the_run() {
    let harness = Harness::new().await.with_rules(
        RulesSettings {
            sync_concurrency: 1,
            ..relaxed()
        },
        None,
    );
    let id = static_endpoint(&harness).await;
    add_rule(&harness, &id, heavy_rule()).await;

    let mut tasks = JoinSet::new();
    for _ in 0..12 {
        let router = harness.router.clone();
        let request = heavy_request(id.unpack());
        tasks.spawn(async move { send(&router, request).await });
    }
    let mut replies = Vec::new();
    while let Some(reply) = tasks.join_next().await {
        replies.push(reply.expect("the request task finishes"));
    }

    let overloaded: Vec<&Reply> = replies
        .iter()
        .filter(|reply| reply.header("x-webhooks-live-rule-error") == Some("overloaded"))
        .collect();
    assert!(!overloaded.is_empty());
    assert!(
        overloaded
            .iter()
            .all(|reply| reply.status == 201 && reply.body == "static")
    );
    assert_eq!(harness.rows(&id).await.len(), 12);
    assert_eq!(run_count(&harness, &id).await, 12);
    let mut settled = false;
    for _ in 0..100 {
        let leased: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE state = 'sync'")
            .fetch_one(harness.db.pool())
            .await
            .expect("the runs are countable");
        if leased == 0 {
            settled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(settled, "overloaded runs stayed in the sync state");
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE state = 'queued'")
        .fetch_one(harness.db.pool())
        .await
        .expect("the runs are countable");
    assert!(queued >= i64::try_from(overloaded.len()).unwrap_or(i64::MAX));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_endpoint_cannot_take_more_than_its_share_of_the_slots_and_others_are_unaffected() {
    let harness = Harness::new().await.with_rules(
        RulesSettings {
            per_endpoint: 1,
            ..relaxed()
        },
        None,
    );
    let busy = static_endpoint(&harness).await;
    let calm = static_endpoint(&harness).await;
    add_rule(&harness, &busy, heavy_rule()).await;
    add_rule(
        &harness,
        &calm,
        rule("answer", json!({}), vec![respond("202", "rule")]),
    )
    .await;

    let mut tasks = JoinSet::new();
    for _ in 0..8 {
        let router = harness.router.clone();
        let request = heavy_request(busy.unpack());
        tasks.spawn(async move { (true, send(&router, request).await) });
    }
    let router = harness.router.clone();
    let path = format!("/{calm}");
    tasks.spawn(async move {
        let request = request("POST", &path)
            .body(Body::from("x"))
            .expect("request builds");
        (false, send(&router, request).await)
    });
    let mut busy_replies = Vec::new();
    let mut calm_reply = None;
    while let Some(joined) = tasks.join_next().await {
        match joined.expect("the request task finishes") {
            (true, reply) => busy_replies.push(reply),
            (false, reply) => calm_reply = Some(reply),
        }
    }

    assert!(
        busy_replies
            .iter()
            .any(|reply| reply.header("x-webhooks-live-rule-error") == Some("overloaded"))
    );
    let calm_reply = calm_reply.expect("the calm endpoint answered");
    assert_eq!(calm_reply.status, 202);
    assert!(calm_reply.header("x-webhooks-live-rules").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_requests_all_get_the_rule_response_from_one_shared_compile() {
    const REQUESTS: usize = 12;
    let settings = RulesSettings {
        per_endpoint: REQUESTS,
        ..relaxed()
    };
    let harness = Harness::new().await.with_rules(settings, None);
    let id = static_endpoint(&harness).await;
    add_rule(
        &harness,
        &id,
        rule(
            "answer",
            json!({"value": "request.method", "op": "matches", "arg": "^PO"}),
            vec![respond("202", "rule")],
        ),
    )
    .await;

    let mut tasks = JoinSet::new();
    for _ in 0..REQUESTS {
        let router = harness.router.clone();
        let path = format!("/{id}");
        tasks.spawn(async move {
            let request = request("POST", &path)
                .body(Body::from("x"))
                .expect("request builds");
            send(&router, request).await
        });
    }
    let mut replies = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        replies.push(joined.expect("the request task finishes"));
    }

    assert!(
        replies
            .iter()
            .all(|reply| reply.status == 202 && reply.body == "rule")
    );
}
