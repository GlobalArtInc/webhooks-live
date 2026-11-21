use std::collections::BTreeSet;

use whl_template::{
    ErrorCode, MAX_DIAGNOSTICS, MAX_EXPRESSIONS, MAX_FILTERS_PER_EXPRESSION, MAX_PATH_SEGMENTS,
    MAX_SOURCE_BYTES, Policy, RenderTarget, StepPolicy, Template, validate,
};

use crate::common::{diagnostics_in, render_clean};

fn body_policy() -> Policy {
    Policy::outbound(RenderTarget::Body)
}

#[test]
fn accepts_valid_templates() {
    let sources = [
        "",
        "plain text",
        "{{ request.method }}",
        "{{request.method}}",
        "{{\trequest.method\n}}",
        "a {{ request.method }} b {{ request.path }} c",
        "{{ request.header.x-token }}",
        "{{ request.header[\"X-Token\"] | upper }}",
        "{{ request.segments[1] }}",
        "{{ var.user.tags[1] }}",
        "{{ var.user[\"name\"] }}",
        "{{ request.query.name | default(\"none\") | upper | truncate(5) }}",
        "{{ \"literal\" }}",
        "{{ 42 }}",
        "{{ -7 }}",
        "{{ uuid() }}",
        "{{ random_hex(8) }}",
        "{{ random_int(1, 6) }}",
        "{{ json(\"$.order.id\") }}",
        "{{ json(step.lookup.body, \"$.data.v\") }}",
        "{{ now | date(\"%Y-%m-%d\") }}",
        "{{ request.body | replace(\"a\", var.n) }}",
        "{{ request.body | hmac_sha256(secret.TOKEN) }}",
        "lone }} close is text",
        "{{ \"}}\" }}",
        "{{ \"{{\" }}",
    ];
    for source in sources {
        let result = validate(source, &body_policy());
        assert!(result.is_ok(), "{source:?} should parse: {result:?}");
    }
}

struct Case {
    source: &'static str,
    code: ErrorCode,
    line: u32,
    column: u32,
    length: usize,
}

const fn case(
    source: &'static str,
    code: ErrorCode,
    line: u32,
    column: u32,
    length: usize,
) -> Case {
    Case {
        source,
        code,
        line,
        column,
        length,
    }
}

#[test]
fn rejects_invalid_templates_with_positions() {
    let cases = [
        case("{{ nope.x }}", ErrorCode::UnknownRoot, 1, 4, 4),
        case("{{ request.nope }}", ErrorCode::UnknownField, 1, 11, 5),
        case("{{ request.query }}", ErrorCode::MissingKey, 1, 4, 13),
        case(
            "{{ request.method.x }}",
            ErrorCode::UnexpectedSegment,
            1,
            18,
            2,
        ),
        case(
            "{{ request.header[0] }}",
            ErrorCode::InvalidSegment,
            1,
            18,
            3,
        ),
        case(
            "{{ request.segments.a }}",
            ErrorCode::InvalidSegment,
            1,
            20,
            2,
        ),
        case("{{ request.segments }}", ErrorCode::MissingKey, 1, 4, 16),
        case("{{ capture.nope }}", ErrorCode::UnknownField, 1, 11, 5),
        case("{{ now.x }}", ErrorCode::UnexpectedSegment, 1, 7, 2),
        case(
            "{{ request.method | nofilter }}",
            ErrorCode::UnknownFilter,
            1,
            21,
            8,
        ),
        case(
            "{{ request.method | truncate }}",
            ErrorCode::WrongArgumentCount,
            1,
            21,
            8,
        ),
        case(
            "{{ request.method | upper(1) }}",
            ErrorCode::WrongArgumentCount,
            1,
            21,
            8,
        ),
        case(
            "{{ request.method | truncate(\"a\") }}",
            ErrorCode::WrongArgumentType,
            1,
            30,
            3,
        ),
        case(
            "{{ request.method | truncate(-1) }}",
            ErrorCode::ArgumentOutOfRange,
            1,
            30,
            2,
        ),
        case(
            "{{ request.method | replace(\"\", \"x\") }}",
            ErrorCode::ArgumentOutOfRange,
            1,
            29,
            2,
        ),
        case(
            "{{ request.method | replace(1, \"x\") }}",
            ErrorCode::WrongArgumentType,
            1,
            29,
            1,
        ),
        case(
            "{{ request.method | date(\"%Q\") }}",
            ErrorCode::InvalidDateFormat,
            1,
            26,
            4,
        ),
        case(
            "{{ request.method | date(\"%\") }}",
            ErrorCode::InvalidDateFormat,
            1,
            26,
            3,
        ),
        case(
            "{{ request.method | upper() }}",
            ErrorCode::UnexpectedCharacter,
            1,
            27,
            1,
        ),
        case("{{ \"abc }}", ErrorCode::UnterminatedString, 1, 4, 7),
        case("{{ \"bad \\q\" }}", ErrorCode::InvalidEscape, 1, 9, 2),
        case(
            "{{ request.method",
            ErrorCode::UnterminatedExpression,
            1,
            1,
            2,
        ),
        case(
            "{{ request.method !}}",
            ErrorCode::UnexpectedCharacter,
            1,
            19,
            1,
        ),
        case("{{ }}", ErrorCode::UnexpectedCharacter, 1, 4, 1),
        case("{{ request. }}", ErrorCode::UnexpectedCharacter, 1, 12, 1),
        case(
            "{{ request.query[ }}",
            ErrorCode::UnexpectedCharacter,
            1,
            19,
            1,
        ),
        case("{{ var.x[-1] }}", ErrorCode::InvalidNumber, 1, 10, 1),
        case(
            "{{ 12345678901234567890 }}",
            ErrorCode::InvalidNumber,
            1,
            4,
            20,
        ),
        case("{{ - }}", ErrorCode::InvalidNumber, 1, 4, 1),
        case("{{ nope() }}", ErrorCode::UnknownFunction, 1, 4, 4),
        case("{{ uuid(1) }}", ErrorCode::WrongArgumentCount, 1, 4, 7),
        case(
            "{{ random_hex(65) }}",
            ErrorCode::ArgumentOutOfRange,
            1,
            15,
            2,
        ),
        case(
            "{{ random_hex(0) }}",
            ErrorCode::ArgumentOutOfRange,
            1,
            15,
            1,
        ),
        case(
            "{{ random_int(5, 1) }}",
            ErrorCode::ArgumentOutOfRange,
            1,
            4,
            16,
        ),
        case(
            "{{ random_int(1) }}",
            ErrorCode::WrongArgumentCount,
            1,
            4,
            13,
        ),
        case("{{ json() }}", ErrorCode::WrongArgumentCount, 1, 4, 6),
        case(
            "{{ json(var.x, \"$\") }}",
            ErrorCode::WrongArgumentType,
            1,
            9,
            5,
        ),
        case(
            "{{ json(request.body) }}",
            ErrorCode::WrongArgumentType,
            1,
            9,
            12,
        ),
        case("{{ json(\"$[\") }}", ErrorCode::InvalidJsonPath, 1, 9, 4),
        case(
            "{{ json(\"$..a..b\") }}",
            ErrorCode::JsonPathTooComplex,
            1,
            9,
            9,
        ),
        case("héllo {{ nope }}", ErrorCode::UnknownRoot, 1, 10, 4),
        case(
            "é{{ \"é\" | nofilter }}",
            ErrorCode::UnknownFilter,
            1,
            11,
            8,
        ),
        case("line1\nline2 {{ nope }}", ErrorCode::UnknownRoot, 2, 10, 4),
        case(
            "日本語\n日本 {{ request.nope }}",
            ErrorCode::UnknownField,
            2,
            14,
            5,
        ),
    ];
    for item in cases {
        let found = diagnostics_in(item.source, &body_policy());
        assert!(
            !found.is_empty(),
            "{:?} should be rejected with {:?}",
            item.source,
            item.code
        );
        let first = &found[0];
        assert_eq!(first.code, item.code, "code for {:?}", item.source);
        assert_eq!(first.position.line, item.line, "line for {:?}", item.source);
        assert_eq!(
            first.position.column, item.column,
            "column for {:?}",
            item.source
        );
        assert_eq!(
            first.position.length, item.length,
            "length for {:?}",
            item.source
        );
    }
}

#[test]
fn reports_byte_offset_next_to_character_column() {
    let found = diagnostics_in("héllo {{ nope }}", &body_policy());
    assert_eq!(found[0].position.offset, 10);
    assert_eq!(found[0].position.column, 10);
}

#[test]
fn reports_every_error_in_one_pass() {
    let found = diagnostics_in(
        "{{ nope }} ok {{ request.bad }} {{ request.method | zzz }} {{ uuid(1) }}",
        &body_policy(),
    );
    let codes: Vec<ErrorCode> = found.iter().map(|diagnostic| diagnostic.code).collect();
    assert_eq!(
        codes,
        vec![
            ErrorCode::UnknownRoot,
            ErrorCode::UnknownField,
            ErrorCode::UnknownFilter,
            ErrorCode::WrongArgumentCount,
        ]
    );
}

#[test]
fn recovers_after_a_syntax_error_and_keeps_reporting() {
    let found = diagnostics_in("{{ ! }} text {{ nope }}", &body_policy());
    let codes: Vec<ErrorCode> = found.iter().map(|diagnostic| diagnostic.code).collect();
    assert_eq!(
        codes,
        vec![ErrorCode::UnexpectedCharacter, ErrorCode::UnknownRoot]
    );
}

#[test]
fn bounds_the_number_of_diagnostics() {
    let source = "{{ nope }}".repeat(MAX_DIAGNOSTICS * 3);
    let error = Template::parse(&source, &body_policy()).expect_err("must fail");
    assert_eq!(error.diagnostics.len(), MAX_DIAGNOSTICS);
    assert!(error.truncated);
}

#[test]
fn does_not_flag_truncation_at_exactly_the_cap() {
    let source = "{{ nope }}".repeat(MAX_DIAGNOSTICS);
    let error = Template::parse(&source, &body_policy()).expect_err("must fail");
    assert_eq!(error.diagnostics.len(), MAX_DIAGNOSTICS);
    assert!(!error.truncated);
}

#[test]
fn rejects_oversized_source_without_parsing() {
    let source = "a".repeat(MAX_SOURCE_BYTES + 1);
    let found = diagnostics_in(&source, &body_policy());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, ErrorCode::SourceTooLarge);
    assert!(validate(&"a".repeat(MAX_SOURCE_BYTES), &body_policy()).is_ok());
}

#[test]
fn limits_the_number_of_expressions() {
    let allowed = "{{ 1 }}".repeat(MAX_EXPRESSIONS);
    assert!(validate(&allowed, &body_policy()).is_ok());
    let found = diagnostics_in(&"{{ 1 }}".repeat(MAX_EXPRESSIONS + 1), &body_policy());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, ErrorCode::TooManyExpressions);
}

#[test]
fn limits_filters_per_expression() {
    let allowed = format!(
        "{{{{ request.method{} }}}}",
        " | upper".repeat(MAX_FILTERS_PER_EXPRESSION)
    );
    assert!(validate(&allowed, &body_policy()).is_ok());
    let rejected = format!(
        "{{{{ request.method{} }}}}",
        " | upper".repeat(MAX_FILTERS_PER_EXPRESSION + 1)
    );
    let found = diagnostics_in(&rejected, &body_policy());
    assert_eq!(found[0].code, ErrorCode::TooManyFilters);
}

#[test]
fn limits_path_depth() {
    let allowed = format!("{{{{ var.a{} }}}}", ".b".repeat(MAX_PATH_SEGMENTS - 1));
    assert!(validate(&allowed, &body_policy()).is_ok());
    let rejected = format!("{{{{ var.a{} }}}}", ".b".repeat(MAX_PATH_SEGMENTS));
    let found = diagnostics_in(&rejected, &body_policy());
    assert_eq!(found[0].code, ErrorCode::PathTooDeep);
}

#[test]
fn rejects_oversized_literals() {
    let long = "x".repeat(2000);
    let found = diagnostics_in(&format!("{{{{ \"{long}\" }}}}"), &body_policy());
    assert_eq!(found[0].code, ErrorCode::LiteralTooLarge);
    let identifier = "y".repeat(100);
    let found = diagnostics_in(&format!("{{{{ {identifier} }}}}"), &body_policy());
    assert_eq!(found[0].code, ErrorCode::LiteralTooLarge);
}

#[test]
fn refuses_secrets_under_the_response_policy() {
    let source = "{{ secret.TOKEN }}";
    let found = diagnostics_in(source, &Policy::response(RenderTarget::Body));
    assert_eq!(found[0].code, ErrorCode::SecretNotAllowed);
    assert_eq!(found[0].code.field_code(), "secret_not_allowed");
    assert!(validate(source, &Policy::outbound(RenderTarget::Body)).is_ok());
}

#[test]
fn checks_step_names_against_earlier_steps() {
    let mut policy = body_policy();
    policy.steps = StepPolicy::Only(BTreeSet::from(["first".to_owned()]));
    assert!(validate("{{ step.first.status }}", &policy).is_ok());
    let found = diagnostics_in("{{ step.second.status }}", &policy);
    assert_eq!(found[0].code, ErrorCode::StepUnknown);
    assert_eq!(found[0].code.field_code(), "step_unknown");
}

#[test]
fn maps_diagnostics_to_api_field_codes() {
    assert_eq!(ErrorCode::UnknownRoot.field_code(), "template_invalid");
    assert_eq!(ErrorCode::InvalidJsonPath.field_code(), "json_path_invalid");
}

#[test]
fn rejects_forbidden_literals_for_header_and_status_targets() {
    let header = Policy::outbound(RenderTarget::HeaderValue);
    let found = diagnostics_in("a\r\nSet-Cookie: x", &header);
    assert_eq!(found[0].code, ErrorCode::ForbiddenCharacter);
    assert_eq!(found[0].position.column, 2);
    let found = diagnostics_in("{{ \"a\\nb\" }}", &header);
    assert_eq!(found[0].code, ErrorCode::ForbiddenCharacter);
    assert!(validate("{{ \"a\\nb\" | url }}", &header).is_ok());
    let status = Policy::outbound(RenderTarget::Status);
    assert_eq!(
        diagnostics_in("20x", &status)[0].code,
        ErrorCode::ForbiddenCharacter
    );
    assert!(validate("200", &status).is_ok());
}

#[test]
fn rejects_literal_text_longer_than_the_target_limit() {
    let header = Policy::outbound(RenderTarget::HeaderValue);
    let found = diagnostics_in(&"a".repeat(1025), &header);
    assert_eq!(found[0].code, ErrorCode::OutputTooLarge);
    assert!(validate(&"a".repeat(1024), &header).is_ok());
}

#[test]
fn json_path_accepts_rfc9535_filters() {
    assert!(
        validate(
            "{{ json(\"$.items[?@.price < 10].name\") }}",
            &body_policy()
        )
        .is_ok()
    );
    let nested = diagnostics_in("{{ json(\"$..a[?@..b]\") }}", &body_policy());
    assert_eq!(nested[0].code, ErrorCode::JsonPathTooComplex);
}

#[test]
fn literal_braces_expression_renders_text() {
    assert_eq!(render_clean("{{ \"{{\" }}x{{ \"}}\" }}"), "{{x}}");
}
