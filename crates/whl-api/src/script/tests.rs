use std::collections::BTreeSet;

use serde_json::Value;
use time::OffsetDateTime;
use whl_rules::request_json;
use whl_script::{MAX_SOURCE_BYTES, VARIABLES};
use whl_template::Context;

use super::input::{MAX_VALIDATE_BODY_BYTES, parse_validate};
use crate::error::ApiError;

fn violations(error: ApiError) -> Vec<(String, &'static str)> {
    match error {
        ApiError::Validation(items) => items
            .iter()
            .map(|item| (item.field().to_owned(), item.code().as_str()))
            .collect(),
        other => panic!("expected a validation error, got {other:?}"),
    }
}

fn object_keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("the value is an object")
        .keys()
        .cloned()
        .collect()
}

fn documented_children(parent: &str) -> BTreeSet<String> {
    let prefix = format!("{parent}.");
    VARIABLES
        .iter()
        .filter_map(|variable| variable.name.strip_prefix(&prefix))
        .filter(|rest| !rest.contains('.'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_source_with_an_optional_phase_is_accepted() {
    let plain = parse_validate(br#"{"source":"let a = 1;"}"#).expect("the body is valid");
    assert_eq!(plain.source, "let a = 1;");
    for phase in ["before", "after", "null"] {
        let quoted = if phase == "null" {
            phase.to_owned()
        } else {
            format!("\"{phase}\"")
        };
        let body = format!(r#"{{"source":"","phase":{quoted}}}"#);
        assert!(parse_validate(body.as_bytes()).is_ok(), "{phase}");
    }
}

#[test]
fn a_source_at_the_limit_passes_and_one_byte_over_is_too_long() {
    let at = format!(r#"{{"source":"{}"}}"#, "a".repeat(MAX_SOURCE_BYTES));
    assert!(parse_validate(at.as_bytes()).is_ok());
    let over = format!(r#"{{"source":"{}"}}"#, "a".repeat(MAX_SOURCE_BYTES + 1));
    let error = parse_validate(over.as_bytes()).expect_err("the source is too long");
    assert_eq!(violations(error), [("source".to_owned(), "too_long")]);
}

#[test]
fn a_multibyte_source_is_measured_in_bytes() {
    let source = "\u{e9}".repeat(MAX_SOURCE_BYTES / 2 + 1);
    let body = serde_json::json!({ "source": source }).to_string();
    let error = parse_validate(body.as_bytes()).expect_err("the source is too long");
    assert_eq!(violations(error), [("source".to_owned(), "too_long")]);
}

#[test]
fn the_worst_case_escaped_source_still_fits_the_route_limit() {
    let source = "\u{1}".repeat(MAX_SOURCE_BYTES);
    let body = serde_json::json!({ "source": source }).to_string();
    assert!(body.len() <= MAX_VALIDATE_BODY_BYTES, "{}", body.len());
    assert!(parse_validate(body.as_bytes()).is_ok());
}

#[test]
fn a_malformed_body_or_field_is_refused_with_its_field() {
    for body in ["", "[]", "{", "\"x\""] {
        assert!(
            matches!(
                parse_validate(body.as_bytes()),
                Err(ApiError::InvalidJson(_))
            ),
            "{body:?}"
        );
    }
    let cases = [
        (r#"{}"#, "source", "required"),
        (r#"{"source":null}"#, "source", "required"),
        (r#"{"source":3}"#, "source", "invalid_format"),
        (
            r#"{"source":"","phase":"during"}"#,
            "phase",
            "invalid_format",
        ),
        (r#"{"source":"","phase":1}"#, "phase", "invalid_format"),
        (r#"{"source":"","mode":"x"}"#, "mode", "not_allowed"),
    ];
    for (body, field, code) in cases {
        let error = parse_validate(body.as_bytes()).expect_err(body);
        assert_eq!(violations(error), [(field.to_owned(), code)], "{body}");
    }
}

#[test]
fn the_documented_request_fields_are_the_ones_the_script_receives() {
    let context = Context::new(OffsetDateTime::UNIX_EPOCH);
    let request = request_json(&context);
    assert_eq!(object_keys(&request), documented_children("request"));
    assert_eq!(
        object_keys(&request["capture"]),
        documented_children("request.capture")
    );
    assert_eq!(
        object_keys(&request["endpoint"]),
        documented_children("request.endpoint")
    );
}
