use whl_template::{BodyData, RenderErrorCode, RenderTarget};

use crate::common::{fixture, pairs, render_clean, render_in};

fn with_query(value: &str) -> whl_template::Context {
    let mut context = fixture();
    context.request.query = pairs(&[("v", value)]);
    context
}

#[test]
fn substituted_template_syntax_is_output_literally() {
    let hostile = [
        "{{ var.n }}",
        "{{ secret.TOKEN }}",
        "}} {{",
        "{{",
        "{{ request.method | upper }}",
        "{{ \"x\" }}{{ uuid() }}",
        "{{{{ nested }}}}",
    ];
    for value in hostile {
        let outcome = render_in(
            "<{{ request.query.v }}>",
            RenderTarget::Body,
            &with_query(value),
        );
        assert!(outcome.is_clean(), "{value}");
        assert_eq!(outcome.output(), format!("<{value}>"));
    }
}

#[test]
fn substituted_body_text_is_never_evaluated() {
    let mut context = fixture();
    context.request.body = BodyData::from_text("{{ secret.TOKEN }}");
    let outcome = render_in("{{ request.body }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "{{ secret.TOKEN }}");
}

#[test]
fn substituted_json_values_are_never_evaluated() {
    let mut context = fixture();
    context.request.body = BodyData::from_text(r#"{"a":"{{ secret.TOKEN }}","b":["{{ x }}"]}"#);
    let outcome = render_in(
        "{{ json(\"$.a\") }}|{{ json(\"$.b\") }}",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(outcome.output(), r#"{{ secret.TOKEN }}|["{{ x }}"]"#);
}

#[test]
fn values_that_look_like_paths_stay_text() {
    for value in [
        "request.method",
        "$.order.id",
        "var.n",
        "secret.TOKEN",
        "step.lookup.body",
    ] {
        let outcome = render_in(
            "{{ request.query.v }}",
            RenderTarget::Body,
            &with_query(value),
        );
        assert_eq!(outcome.output(), value);
    }
}

#[test]
fn substituted_values_are_not_used_as_lookup_names() {
    let mut context = with_query("secret.TOKEN");
    context
        .vars
        .insert("secret.TOKEN".to_owned(), serde_json::json!("nope"));
    let outcome = render_in(
        "{{ var.user.name | default(request.query.v) }}",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(outcome.output(), "ann");
}

#[test]
fn literal_braces_in_strings_do_not_open_expressions() {
    assert_eq!(render_clean("{{ \"{{ var.n }}\" }}"), "{{ var.n }}");
    assert_eq!(render_clean("{{ \"a}}b\" | upper }}"), "A}}B");
}

#[test]
fn crlf_in_a_header_value_is_rejected_not_written() {
    let hostile = [
        "a\r\nSet-Cookie: x=1",
        "a\nb",
        "a\rb",
        "a\0b",
        "caf\u{e9}",
        "a\u{7f}b",
        "a\u{85}b",
        "a\u{2028}b",
    ];
    for value in hostile {
        let outcome = render_in(
            "pre-{{ request.query.v }}-post",
            RenderTarget::HeaderValue,
            &with_query(value),
        );
        assert!(!outcome.is_clean(), "{value:?}");
        assert_eq!(
            outcome.errors()[0].code,
            RenderErrorCode::ForbiddenCharacter
        );
        let output = outcome.output();
        assert_eq!(output, "pre--post");
        assert!(!output.contains(['\r', '\n', '\0']));
    }
}

#[test]
fn header_values_may_contain_visible_ascii_space_and_tab() {
    let outcome = render_in(
        "{{ request.query.v }}",
        RenderTarget::HeaderValue,
        &with_query("Bearer abc.def~ghi\t!#$%&'*+-^_`|"),
    );
    assert!(outcome.is_clean());
}

#[test]
fn encoding_filters_make_hostile_values_safe_for_headers() {
    let outcome = render_in(
        "{{ request.query.v | url }}",
        RenderTarget::HeaderValue,
        &with_query("a\r\nSet-Cookie: x=1\0é"),
    );
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "a%0D%0ASet-Cookie%3A%20x%3D1%00%C3%A9");
}

#[test]
fn json_filter_keeps_values_inside_a_json_document() {
    let hostile = "x\",\"admin\":true,\"y\":\"";
    let outcome = render_in(
        "{\"k\":{{ request.query.v | json }}}",
        RenderTarget::Body,
        &with_query(hostile),
    );
    let parsed: serde_json::Value = serde_json::from_str(outcome.output()).expect("valid json");
    assert_eq!(parsed.as_object().expect("object").len(), 1);
    assert_eq!(parsed["k"], hostile);
}

#[test]
fn json_string_target_escapes_every_substituted_value() {
    let hostile = "x\",\"admin\":true,\"y\":\"\n\\\u{1}\u{2028}";
    let outcome = render_in(
        "{\"k\":\"{{ request.query.v }}\",\"c\":\"{{ \"q\\\"\" }}\"}",
        RenderTarget::JsonString,
        &with_query(hostile),
    );
    assert!(outcome.is_clean());
    let parsed: serde_json::Value = serde_json::from_str(outcome.output()).expect("valid json");
    assert_eq!(parsed.as_object().expect("object").len(), 2);
    assert_eq!(parsed["k"], hostile);
    assert_eq!(parsed["c"], "q\"");
}

#[test]
fn json_string_target_does_not_escape_template_text() {
    let outcome = render_in("{\"a\": 1}", RenderTarget::JsonString, &fixture());
    assert_eq!(outcome.output(), "{\"a\": 1}");
}

#[test]
fn html_filter_neutralises_markup_in_substituted_values() {
    let outcome = render_in(
        "<p>{{ request.query.v | html }}</p>",
        RenderTarget::Body,
        &with_query("<script>alert(1)</script>"),
    );
    assert_eq!(
        outcome.output(),
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"
    );
}

#[test]
fn missing_notes_name_the_template_source_never_the_value() {
    let outcome = render_in("{{ secret.NOPE }}", RenderTarget::Body, &fixture());
    assert_eq!(outcome.missing()[0].reference, "secret.NOPE");
}
