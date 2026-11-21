use whl_template::RenderTarget;

use crate::common::{fixture, render, render_clean, render_in};

#[test]
fn request_roots_render_their_values() {
    let cases = [
        ("{{ request.method }}", "POST"),
        ("{{ request.path }}", "/hook/orders"),
        (
            "{{ request.segments[0] }}-{{ request.segments[1] }}",
            "hook-orders",
        ),
        ("{{ request.query.a }}", "1"),
        ("{{ request.query[\"name\"] }}", "Zoë & co"),
        ("{{ request.header.x-token }}", "abc"),
        ("{{ request.header[\"X-TOKEN\"] }}", "abc"),
        ("{{ request.header.content-type }}", "application/json"),
        (
            "{{ request.headers }}",
            r#"{"content-type":"application/json","x-token":"abc, def"}"#,
        ),
        ("{{ request.form.f }}", "form value"),
        ("{{ request.ip }}", "203.0.113.9"),
        ("{{ request.content_type }}", "application/json"),
        ("{{ request.kind }}", "http"),
        ("{{ request.meta.k }}", "v"),
        ("{{ request.signature }}", "valid"),
    ];
    for (source, expected) in cases {
        assert_eq!(render_clean(source), expected, "{source}");
    }
}

#[test]
fn request_query_names_are_case_sensitive_and_headers_are_not() {
    let outcome = render("{{ request.query.NAME }}");
    assert_eq!(outcome.output(), "");
    assert_eq!(outcome.missing().len(), 1);
    assert_eq!(render_clean("{{ request.header.X-TOKEN }}"), "abc");
}

#[test]
fn request_body_is_text() {
    let output = render_clean("{{ request.body | truncate(9) }}");
    assert_eq!(output, r#"{"order":"#);
}

#[test]
fn capture_endpoint_and_now_roots() {
    let cases = [
        ("{{ capture.id }}", "cap_1"),
        ("{{ capture.seq }}", "7"),
        ("{{ capture.received_at }}", "2024-03-05T14:07:09Z"),
        ("{{ endpoint.id }}", "ep_1"),
        ("{{ endpoint.url }}", "https://webhooks.live/e/ep_1"),
        ("{{ endpoint.title }}", "Orders"),
        ("{{ now }}", "2024-03-05T14:07:09Z"),
    ];
    for (source, expected) in cases {
        assert_eq!(render_clean(source), expected, "{source}");
    }
}

#[test]
fn variable_roots_read_nested_json() {
    let cases = [
        ("{{ var.n }}", "5"),
        ("{{ var.user.name }}", "ann"),
        ("{{ var.user.tags[1] }}", "y"),
        ("{{ var.user[\"tags\"][0] }}", "x"),
        ("{{ var.user.n }}", "5"),
        ("{{ var.user.ok }}", "true"),
        ("{{ var.user.tags }}", r#"["x","y"]"#),
        ("{{ store.s }}", "stored"),
        ("{{ counter.hits }}", "3"),
    ];
    for (source, expected) in cases {
        assert_eq!(render_clean(source), expected, "{source}");
    }
}

#[test]
fn step_root_reads_earlier_step_results() {
    let cases = [
        ("{{ step.lookup.status }}", "201"),
        ("{{ step.lookup.header.x-id }}", "9"),
        ("{{ step.lookup.body }}", r#"{"data":{"v":1}}"#),
        ("{{ step.lookup.duration_ms }}", "12"),
    ];
    for (source, expected) in cases {
        assert_eq!(render_clean(source), expected, "{source}");
    }
    let outcome = render("{{ step.lookup.error }}|{{ step.other.status }}");
    assert_eq!(outcome.output(), "|");
    assert_eq!(outcome.missing().len(), 2);
}

#[test]
fn missing_values_render_empty_and_are_noted() {
    let outcome = render("a{{ var.absent }}b{{ var.user.nope }}c{{ var.nul }}d");
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "abcd");
    assert_eq!(outcome.missing().len(), 3);
    let first = &outcome.missing()[0];
    assert_eq!(first.reference, "var.absent");
    assert_eq!(first.position.column, 5);
}

#[test]
fn out_of_range_segment_is_missing() {
    let outcome = render("{{ request.segments[5] }}");
    assert_eq!(outcome.output(), "");
    assert_eq!(outcome.missing().len(), 1);
}

#[test]
fn secrets_render_only_when_the_policy_allowed_them() {
    let outcome = render_in("{{ secret.TOKEN }}", RenderTarget::Body, &fixture());
    assert_eq!(outcome.output(), "s3cret");
    let debug = format!("{:?}", fixture());
    assert!(!debug.contains("s3cret"));
    assert!(debug.contains("TOKEN"));
}
