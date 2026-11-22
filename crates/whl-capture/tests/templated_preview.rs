mod common;
mod served;

use std::collections::BTreeMap;

use axum::body::Body;
use common::{Harness, PUBLIC_URL, request};
use serde_json::json;
use served::{Configured, endpoint_configured, make_templated, set_var};
use time::OffsetDateTime;
use whl_capture::{
    PreviewReport, PreviewSeverity, RuleFacts, TemplateDraft, render_preview, response_context,
};
use whl_core::capture_id::CaptureId;
use whl_store::Store;
use whl_template::{BodyData, Context, NameValue};

fn draft(body: &str) -> TemplateDraft {
    TemplateDraft {
        status: 200,
        status_template: None,
        content_type: "text/plain".to_owned(),
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn context() -> Context {
    let mut context = Context::new(OffsetDateTime::UNIX_EPOCH);
    context.request.method = "POST".to_owned();
    context.request.path = "/hook".to_owned();
    context.request.query = vec![NameValue::new("mode", "test")];
    context.request.body = BodyData::from_text(r#"{"id":"evt_1"}"#);
    context.capture.id = "capture".to_owned();
    context
}

fn report(draft: &TemplateDraft, context: &Context) -> PreviewReport {
    render_preview(draft, Some(context))
}

#[test]
fn a_clean_template_renders_status_headers_and_body() {
    let mut input = draft(r#"{"id":"{{ json("$.id") }}","m":"{{ request.method }}"}"#);
    input.status_template = Some("201".to_owned());
    input.headers = vec![NameValue::new("X-Mode", "{{ request.query.mode }}")];
    let output = report(&input, &context());
    let rendered = output.rendered.expect("the draft renders");
    assert_eq!(rendered.status, 201);
    assert_eq!(rendered.body, r#"{"id":"evt_1","m":"POST"}"#);
    assert_eq!(rendered.headers, vec![NameValue::new("X-Mode", "test")]);
    assert_eq!(rendered.content_type, "text/plain");
    assert!(output.diagnostics.is_empty());
}

#[test]
fn an_unknown_filter_is_an_error_with_its_position_and_nothing_renders() {
    let input = draft("line one\n  {{ now | isodate }}");
    let output = report(&input, &context());
    assert!(output.rendered.is_none());
    let diagnostic = output.diagnostics.first().expect("one diagnostic");
    assert_eq!(diagnostic.field, "response.body");
    assert_eq!(diagnostic.code, "template_invalid");
    assert_eq!(diagnostic.severity, PreviewSeverity::Error);
    assert_eq!((diagnostic.line, diagnostic.column), (2, 12));
    assert!(diagnostic.length >= 1);
}

#[test]
fn secrets_are_refused_in_response_templates() {
    let mut input = draft("{{ secret.TOKEN }}");
    input.headers = vec![NameValue::new("X-Key", "{{ secret.TOKEN }}")];
    let output = report(&input, &context());
    assert!(output.rendered.is_none());
    let fields: Vec<(&str, &str)> = output
        .diagnostics
        .iter()
        .map(|item| (item.field.as_str(), item.code.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("response.headers.0.value", "secret_not_allowed"),
            ("response.body", "secret_not_allowed")
        ]
    );
}

#[test]
fn a_value_the_capture_lacks_renders_empty_with_a_warning() {
    let output = report(&draft("a={{ request.query.missing }}."), &context());
    assert_eq!(output.rendered.expect("it renders").body, "a=.");
    let diagnostic = output.diagnostics.first().expect("a warning");
    assert_eq!(diagnostic.severity, PreviewSeverity::Warning);
    assert_eq!(diagnostic.code, "value_missing");
    assert_eq!((diagnostic.line, diagnostic.column), (1, 6));
}

#[test]
fn a_header_that_renders_a_line_break_is_dropped_as_the_live_responder_does() {
    let mut context = context();
    context.request.query = vec![NameValue::new("x", "a\nb")];
    let mut input = draft("ok");
    input.headers = vec![
        NameValue::new("X-Bad", "{{ request.query.x }}"),
        NameValue::new("X-Good", "fine"),
    ];
    let output = report(&input, &context);
    let rendered = output.rendered.expect("the response stands");
    assert_eq!(rendered.headers, vec![NameValue::new("X-Good", "fine")]);
    let diagnostic = output.diagnostics.first().expect("a warning");
    assert_eq!(diagnostic.field, "response.headers.0.value");
    assert_eq!(diagnostic.severity, PreviewSeverity::Warning);
}

#[test]
fn a_status_that_is_not_three_digits_in_range_is_an_error() {
    let mut input = draft("ok");
    input.status_template = Some("600".to_owned());
    let output = report(&input, &context());
    assert!(output.rendered.is_none());
    let diagnostic = output.diagnostics.first().expect("an error");
    assert_eq!(diagnostic.field, "response.status_template");
    assert_eq!(diagnostic.code, "invalid_status");
}

#[test]
fn output_over_the_body_limit_is_an_error_and_nothing_renders() {
    let mut context = context();
    context.request.body = BodyData::from_text(&"x".repeat(60 * 1024));
    let output = report(&draft(&"{{ request.body }}".repeat(30)), &context);
    assert!(output.rendered.is_none());
    let diagnostic = output.diagnostics.first().expect("an error");
    assert_eq!(diagnostic.code, "output_limit_exceeded");
    assert_eq!(diagnostic.severity, PreviewSeverity::Error);
}

#[test]
fn without_a_capture_the_draft_is_only_checked() {
    let valid = render_preview(&draft("{{ request.method }}"), None);
    assert!(valid.rendered.is_none());
    assert!(valid.diagnostics.is_empty());
    let invalid = render_preview(&draft("{{ nope }}"), None);
    assert_eq!(invalid.diagnostics.len(), 1);
}

#[tokio::test]
async fn the_preview_matches_what_the_live_responder_sends() {
    let harness = Harness::new().await;
    let body = "{{ request.method }}|{{ request.header.x-token }}|{{ request.query.q }}|\
                {{ json(\"$.order.id\") }}|{{ request.segments[1] }}|{{ request.ip }}|\
                {{ capture.seq }}|{{ counter.hits }}|{{ store.name }}|{{ endpoint.id }}|\
                {{ endpoint.url }}|{{ request.kind }}|{{ request.signature }}|\
                {{ request.content_type }}";
    let id = endpoint_configured(
        &harness,
        Configured {
            body: body.as_bytes().to_vec(),
            headers: vec![("X-Echo", "{{ request.query.q | upper }}")],
            ..Configured::default()
        },
    )
    .await;
    make_templated(&harness, &id, Some("{{ counter.hits }}01")).await;
    set_var(&harness, &id, "hits", json!(2)).await;
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
    assert_eq!(reply.status, 201);
    assert!(reply.header("x-webhooks-live-template").is_none());

    let store = Store::from_pool(harness.db.pool().clone());
    let stored = harness.rows(&id).await;
    let capture = CaptureId::parse(&stored[0].id).expect("a capture id");
    let detail = store
        .get_capture(&id, &capture)
        .await
        .expect("the capture reads")
        .expect("the capture exists");
    let facts = RuleFacts::from_detail(&detail, PUBLIC_URL, None).expect("the facts build");
    let persistent = BTreeMap::from([
        ("hits".to_owned(), json!(2)),
        ("name".to_owned(), json!("Alice")),
    ]);
    let context = response_context(facts, &persistent);
    let input = TemplateDraft {
        status: 200,
        status_template: Some("{{ counter.hits }}01".to_owned()),
        content_type: "text/plain".to_owned(),
        headers: vec![NameValue::new("X-Echo", "{{ request.query.q | upper }}")],
        body: body.as_bytes().to_vec(),
    };
    let output = render_preview(&input, Some(&context));
    let rendered = output.rendered.expect("the preview renders");
    assert_eq!(rendered.status, reply.status);
    assert_eq!(rendered.body, reply.body);
    assert_eq!(
        rendered.headers,
        vec![NameValue::new(
            "X-Echo",
            reply.header("x-echo").unwrap_or("")
        )]
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}
