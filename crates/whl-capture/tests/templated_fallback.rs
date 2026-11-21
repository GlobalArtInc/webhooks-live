mod common;
mod served;

use std::time::{Duration, Instant};

use axum::body::Body;
use common::{Harness, Reply, request};
use serde_json::json;
use served::{Configured, endpoint_configured, make_templated};
use whl_capture::TemplateSettings;
use whl_core::endpoint_id::EndpointId;

async fn templated(harness: &Harness, configured: Configured, status: Option<&str>) -> EndpointId {
    let id = endpoint_configured(harness, configured).await;
    make_templated(harness, &id, status).await;
    id
}

fn assert_fallback(reply: &Reply, code: &str) {
    assert_eq!(reply.header("x-webhooks-live-template"), Some("error"));
    assert_eq!(reply.header("x-webhooks-live-template-error"), Some(code));
    assert_eq!(reply.body, "");
    assert!(reply.header("x-echo").is_none());
}

fn heavy_body() -> Vec<u8> {
    let expression = "{{ request.body | base64 | hex | sha256 | hex | base64 | hex }}";
    expression.repeat(400).into_bytes()
}

async fn heavy_request(harness: &Harness, id: &EndpointId) -> Reply {
    harness
        .send(
            request("POST", &format!("/{id}"))
                .body(Body::from("x".repeat(60 * 1024)))
                .expect("request builds"),
        )
        .await
}

#[tokio::test]
async fn a_slow_template_hits_the_budget_and_serves_the_static_fallback() {
    let harness = Harness::new().await.with_templates(TemplateSettings {
        response_budget: Duration::from_millis(1),
        ..TemplateSettings::default()
    });
    let id = templated(
        &harness,
        Configured {
            status: 202,
            content_type: "application/json",
            headers: vec![("X-Echo", "yes")],
            body: heavy_body(),
            ..Configured::default()
        },
        None,
    )
    .await;

    let started = Instant::now();
    let reply = heavy_request(&harness, &id).await;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(reply.status, 202);
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_fallback(&reply, "budget");
    let meta = harness.wait_for_meta(&id, 1).await;
    assert_eq!(meta["template"]["outcome"], "fallback");
    assert_eq!(meta["template"]["code"], "budget");
    assert_eq!(meta["template"]["diagnostics"][0]["field"], "response.body");
    assert_eq!(
        meta["template"]["diagnostics"][0]["code"],
        "budget_exhausted"
    );
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn the_per_render_deadline_also_falls_back() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: heavy_body(),
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = heavy_request(&harness, &id).await;

    let code = reply
        .header("x-webhooks-live-template-error")
        .expect("a fallback code");
    assert!(["deadline", "budget", "render"].contains(&code), "{code}");
    assert_eq!(reply.header("x-webhooks-live-template"), Some("error"));
}

#[tokio::test]
async fn a_render_error_in_the_body_serves_the_fallback_and_records_the_position() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: b"line one\n{{ request.query.x | base64_decode }}".to_vec(),
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness.get(&format!("/{id}?x=%21%21")).await;

    assert_fallback(&reply, "render");
    let meta = harness.wait_for_meta(&id, 1).await;
    assert_eq!(
        meta,
        json!({"template": {
            "outcome": "fallback",
            "code": "render",
            "diagnostics": [{"field": "response.body", "code": "invalid_base64", "line": 2, "column": 22}],
            "truncated": false
        }})
    );
}

#[tokio::test]
async fn an_invalid_rendered_status_falls_back_to_the_configured_status() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            status: 207,
            ..Configured::default()
        },
        Some("{{ request.query.code }}"),
    )
    .await;

    let reply = harness.get(&format!("/{id}?code=99")).await;

    assert_eq!(reply.status, 207);
    assert_fallback(&reply, "status");
    let meta = harness.wait_for_meta(&id, 1).await;
    assert_eq!(meta["template"]["code"], "status");
    assert_eq!(
        meta["template"]["diagnostics"][0]["field"],
        "response.status_template"
    );
    assert_eq!(harness.stored_status(&id, 1).await, 207);
}

#[tokio::test]
async fn a_header_that_overflows_its_limit_falls_back_instead_of_being_cut() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: b"body".to_vec(),
            headers: vec![("X-Echo", "{{ request.query.x }}")],
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness.get(&format!("/{id}?x={}", "a".repeat(2000))).await;

    assert_fallback(&reply, "render");
}

#[tokio::test]
async fn a_stored_template_that_no_longer_validates_falls_back_as_invalid() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: b"{{ nonsense.root }}".to_vec(),
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness.get(&format!("/{id}")).await;

    assert_fallback(&reply, "invalid");
}

#[tokio::test]
async fn a_full_render_pool_falls_back_as_overloaded() {
    let harness = Harness::new().await.with_templates(TemplateSettings {
        concurrency: 0,
        ..TemplateSettings::default()
    });
    let id = templated(&harness, Configured::default(), None).await;

    let reply = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.status, 200);
    assert_fallback(&reply, "overloaded");
}

#[tokio::test]
async fn fallback_keeps_cors_the_mandatory_headers_and_the_delay() {
    let harness = Harness::new().await.with_templates(TemplateSettings {
        concurrency: 0,
        ..TemplateSettings::default()
    });
    let id = templated(
        &harness,
        Configured {
            delay_ms: 150,
            cors: true,
            ..Configured::default()
        },
        None,
    )
    .await;

    let started = Instant::now();
    let reply = harness.get(&format!("/{id}")).await;

    assert!(started.elapsed() >= Duration::from_millis(150));
    assert_eq!(reply.header("access-control-allow-origin"), Some("*"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_fallback(&reply, "overloaded");
}

#[tokio::test]
async fn a_user_header_cannot_forge_the_template_state_header() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            headers: vec![("X-Webhooks-Live-Template", "ok")],
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness.get(&format!("/{id}")).await;

    assert!(reply.header("x-webhooks-live-template").is_none());
}

#[tokio::test]
async fn render_metrics_count_outcomes_fallbacks_and_durations() {
    whl_telemetry::metrics::install().expect("the recorder installs");
    let harness = Harness::new().await.with_templates(TemplateSettings {
        concurrency: 0,
        ..TemplateSettings::default()
    });
    let id = templated(&harness, Configured::default(), None).await;

    harness.get(&format!("/{id}")).await;

    let rendered = whl_telemetry::metrics::render();
    assert!(rendered.contains("webhooks_live_template_renders_total{outcome=\"fallback\"}"));
    assert!(rendered.contains("webhooks_live_template_fallbacks_total{reason=\"overloaded\"}"));
    assert!(rendered.contains("webhooks_live_template_render_duration_seconds"));
}

#[tokio::test]
async fn unreadable_variables_fall_back_without_losing_the_capture() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: b"{{ counter.hits }}".to_vec(),
            ..Configured::default()
        },
        None,
    )
    .await;
    sqlx::query("ALTER TABLE endpoint_vars RENAME TO endpoint_vars_away")
        .execute(harness.db.pool())
        .await
        .expect("the table is renamed");

    let reply = harness.get(&format!("/{id}")).await;

    assert_fallback(&reply, "variables");
    assert_eq!(harness.rows(&id).await.len(), 1);
}
