mod common;
mod served;

use std::time::{Duration, Instant};

use axum::body::Body;
use common::{Harness, request};
use serde_json::json;
use served::{Configured, Wire, endpoint_configured, make_templated, serve, set_var};
use whl_core::endpoint_id::EndpointId;

async fn templated(harness: &Harness, configured: Configured, status: Option<&str>) -> EndpointId {
    let id = endpoint_configured(harness, configured).await;
    make_templated(harness, &id, status).await;
    id
}

fn body_of(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

#[tokio::test]
async fn untemplated_endpoint_keeps_template_text_literal() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: body_of("{{ request.method }}"),
            headers: vec![("X-Echo", "{{ request.method }}")],
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.post(&format!("/{id}"), "x").await;

    assert_eq!(reply.body, "{{ request.method }}");
    assert_eq!(reply.header("x-echo"), Some("{{ request.method }}"));
    assert!(reply.header("x-webhooks-live-template").is_none());
    assert_eq!(harness.meta(&id, 1).await, json!({}));
}

#[tokio::test]
async fn every_root_renders_from_the_request() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of(
                "{{ request.method }}|{{ request.header.x-token }}|{{ request.query.q }}|\
                 {{ json(\"$.order.id\") }}|{{ request.segments[1] }}|{{ request.ip }}|\
                 {{ capture.seq }}|{{ counter.hits }}|{{ store.name }}|{{ endpoint.id }}",
            ),
            ..Configured::default()
        },
        None,
    )
    .await;
    set_var(&harness, &id, "hits", json!(7)).await;
    set_var(&harness, &id, "name", json!("Alice")).await;

    let reply = harness
        .send(
            request("POST", &format!("/{id}/a/b?q=hello"))
                .header("x-token", "abc")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"order":{"id":42}}"#))
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body,
        format!("POST|abc|hello|42|b|198.51.100.10|1|7|Alice|{id}")
    );
    assert!(reply.header("x-webhooks-live-template").is_none());
}

#[tokio::test]
async fn header_values_are_templates() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            headers: vec![
                ("X-Echo", "token={{ request.header.x-token }}"),
                ("X-Seq", "{{ capture.seq }}"),
                ("X-Static", "plain"),
            ],
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness
        .send(
            request("GET", &format!("/{id}"))
                .header("x-token", "abc")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.header("x-echo"), Some("token=abc"));
    assert_eq!(reply.header("x-seq"), Some("1"));
    assert_eq!(reply.header("x-static"), Some("plain"));
}

#[tokio::test]
async fn status_template_sets_the_status_and_the_stored_status() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            status: 202,
            ..Configured::default()
        },
        Some("{{ request.query.code | default(\"201\") }}"),
    )
    .await;

    let chosen = harness.get(&format!("/{id}?code=404")).await;
    let default = harness.get(&format!("/{id}")).await;

    assert_eq!(chosen.status, 404);
    assert_eq!(default.status, 201);
    for _ in 0..200 {
        if harness.stored_status(&id, 1).await == 404 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(harness.stored_status(&id, 1).await, 404);
}

#[tokio::test]
async fn status_override_in_the_path_wins_over_the_status_template() {
    let harness = Harness::new().await;
    let id = templated(&harness, Configured::default(), Some("{{ \"201\" }}")).await;

    let reply = harness.get(&format!("/{id}/418")).await;

    assert_eq!(reply.status, 418);
    assert!(reply.header("x-webhooks-live-template").is_none());
}

#[tokio::test]
async fn informational_rendered_status_is_substituted_like_a_configured_one() {
    let harness = Harness::new().await;
    let id = templated(&harness, Configured::default(), Some("{{ \"101\" }}")).await;

    let reply = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.header("x-webhooks-live-substituted-status"),
        Some("101")
    );
}

#[tokio::test]
async fn crlf_from_request_data_never_reaches_a_header() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("ok {{ request.method }}"),
            headers: vec![
                ("X-Echo", "{{ request.query.x }}"),
                ("X-Safe", "{{ request.query.y }}"),
            ],
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness
        .get(&format!("/{id}?x=a%0D%0AInjected:%201&y=fine"))
        .await;

    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "ok GET");
    assert!(reply.header("x-echo").is_none());
    assert!(reply.header("injected").is_none());
    assert_eq!(reply.header("x-safe"), Some("fine"));
    let meta = harness.wait_for_meta(&id, 1).await;
    assert_eq!(meta["template"]["outcome"], "header_dropped");
    assert_eq!(
        meta["template"]["diagnostics"][0]["field"],
        "response.headers.0.value"
    );
    assert_eq!(
        meta["template"]["diagnostics"][0]["code"],
        "forbidden_character"
    );
}

#[tokio::test]
async fn braces_in_request_data_are_data_and_never_evaluated() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("[{{ request.query.q }}][{{ request.body }}]"),
            ..Configured::default()
        },
        None,
    )
    .await;
    let nested = "{{ request.method }}{{ {{ counter.x }} }}";

    let reply = harness
        .send(
            request("POST", &format!("/{id}?q=%7B%7B%20request.method%20%7D%7D"))
                .body(Body::from(nested))
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.body, format!("[{{{{ request.method }}}}][{nested}]"));
}

#[tokio::test]
async fn huge_request_values_are_capped_by_the_body_text_window() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("{{ request.body }}"),
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .body(Body::from("a".repeat(200 * 1024)))
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.body.len(), 64 * 1024);
}

#[tokio::test]
async fn content_type_cors_and_delay_survive_templating() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            content_type: "application/json",
            body: body_of("{\"seq\":{{ capture.seq }}}"),
            delay_ms: 200,
            cors: true,
            ..Configured::default()
        },
        None,
    )
    .await;

    let started = Instant::now();
    let reply = harness.get(&format!("/{id}")).await;

    assert!(started.elapsed() >= Duration::from_millis(200));
    assert_eq!(reply.body, "{\"seq\":1}");
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(reply.header("access-control-allow-origin"), Some("*"));
    assert_eq!(reply.header("content-security-policy"), Some("sandbox"));
}

#[tokio::test]
async fn a_cors_preflight_is_answered_without_rendering() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("{{ request.query.x | base64_decode }}"),
            cors: true,
            ..Configured::default()
        },
        None,
    )
    .await;

    let reply = harness
        .send(
            request("OPTIONS", &format!("/{id}/hook?x=%21"))
                .header("origin", "https://app.example")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.status, 204);
    assert!(reply.header("x-webhooks-live-template").is_none());
    assert_eq!(reply.header("access-control-allow-methods"), Some("POST"));
}

#[tokio::test]
async fn head_and_bodyless_statuses_are_framed_as_for_static_responses() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("{{ request.method }}-body"),
            ..Configured::default()
        },
        None,
    )
    .await;

    let head = harness
        .send(
            request("HEAD", &format!("/{id}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    let no_content = harness.get(&format!("/{id}/204")).await;
    let not_modified = harness.get(&format!("/{id}/304")).await;

    assert_eq!(head.body, "");
    assert_eq!(head.header("content-length"), Some("9"));
    assert_eq!(no_content.status, 204);
    assert_eq!(no_content.body, "");
    assert!(no_content.header("content-length").is_none());
    assert_eq!(not_modified.status, 304);
    assert!(not_modified.header("content-type").is_none());
}

#[tokio::test]
async fn rendered_response_is_framed_by_content_length_over_a_real_socket() {
    let harness = Harness::new().await;
    let id = templated(
        &harness,
        Configured {
            body: body_of("hello {{ request.query.who | upper }}"),
            headers: vec![("X-Who", "{{ request.query.who }}")],
            ..Configured::default()
        },
        Some("{{ \"203\" }}"),
    )
    .await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    wire.write(format!("GET /{id}?who=bob HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
        .await;
    let head = wire.head().await;

    assert_eq!(head.status, 203);
    assert_eq!(head.header("content-length").as_deref(), Some("9"));
    assert_eq!(head.header("x-who").as_deref(), Some("bob"));
    assert_eq!(wire.body(9).await, b"hello BOB");
}
