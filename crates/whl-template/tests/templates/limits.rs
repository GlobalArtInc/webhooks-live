use std::time::Duration;

use whl_template::{
    BODY_TEXT_BYTES, BodyData, ChaChaRandom, RandomSeed, RenderEnv, RenderErrorCode, RenderLimits,
    RenderTarget, SystemClock,
};

use crate::common::{
    FixedClock, SequenceRandom, SteppingClock, fixture, parse_in, render_in, render_template,
};

fn body_limits() -> RenderLimits {
    RenderLimits::for_target(RenderTarget::Body)
}

#[test]
fn defaults_match_the_specification() {
    let limits = body_limits();
    assert_eq!(limits.max_output_bytes, 1024 * 1024);
    assert_eq!(limits.deadline, Duration::from_millis(50));
    assert_eq!(
        RenderLimits::for_target(RenderTarget::HeaderValue).max_output_bytes,
        1024
    );
    assert_eq!(
        RenderLimits::for_target(RenderTarget::Status).max_output_bytes,
        3
    );
}

#[test]
fn output_limit_stops_the_render_and_keeps_the_prefix() {
    let template = parse_in("0123456789{{ var.n }}tail", RenderTarget::Body);
    let mut limits = body_limits();
    limits.max_output_bytes = 10;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.output(), "0123456789");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::OutputLimitExceeded
    );
    assert!(!outcome.is_clean());
}

#[test]
fn output_limit_boundary_is_inclusive() {
    let template = parse_in("12345", RenderTarget::Body);
    let mut limits = body_limits();
    limits.max_output_bytes = 5;
    assert!(render_template(&template, &fixture(), &limits).is_clean());
    limits.max_output_bytes = 4;
    assert!(!render_template(&template, &fixture(), &limits).is_clean());
}

#[test]
fn repeated_large_substitutions_cannot_exceed_the_output_cap() {
    let mut context = fixture();
    context
        .vars
        .insert("big".to_owned(), serde_json::json!("x".repeat(100_000)));
    let source = "{{ var.big }}".repeat(200);
    let template = parse_in(&source, RenderTarget::Body);
    let outcome = render_template(&template, &context, &body_limits());
    assert!(outcome.output().len() <= 1024 * 1024);
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::OutputLimitExceeded
    );
    assert_eq!(outcome.output().len(), 10 * 100_000);
}

#[test]
fn intermediate_values_are_capped() {
    let mut context = fixture();
    context
        .vars
        .insert("a".to_owned(), serde_json::json!("a".repeat(10_000)));
    let blowup = "b".repeat(1000);
    let source = format!("[{{{{ var.a | replace(\"a\", \"{blowup}\") }}}}]");
    let template = parse_in(&source, RenderTarget::Body);
    let outcome = render_template(&template, &context, &body_limits());
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::ValueTooLarge);
}

#[test]
fn growing_filter_chains_are_stopped_by_the_value_cap() {
    let mut context = fixture();
    context
        .vars
        .insert("a".to_owned(), serde_json::json!("a".repeat(100_000)));
    let template = parse_in(
        "[{{ var.a | hex | hex | hex | hex | hex | hex }}]",
        RenderTarget::Body,
    );
    let outcome = render_template(&template, &context, &body_limits());
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::ValueTooLarge);
}

#[test]
fn a_recoverable_error_skips_one_expression_and_continues() {
    let template = parse_in(
        "a{{ \"***\" | base64_decode }}b{{ var.n }}",
        RenderTarget::Body,
    );
    let outcome = render_template(&template, &fixture(), &body_limits());
    assert_eq!(outcome.output(), "ab5");
    assert_eq!(outcome.errors().len(), 1);
}

#[test]
fn evaluation_limit_counts_expressions_and_filters() {
    let template = parse_in("{{ 1 }}{{ 2 }}{{ 3 }}{{ 4 }}", RenderTarget::Body);
    let mut limits = body_limits();
    limits.max_evaluations = 3;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.output(), "123");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::EvaluationLimitExceeded
    );

    let chained = parse_in("{{ 1 | upper | lower }}", RenderTarget::Body);
    limits.max_evaluations = 2;
    let outcome = render_template(&chained, &fixture(), &limits);
    assert_eq!(outcome.output(), "");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::EvaluationLimitExceeded
    );
    limits.max_evaluations = 3;
    assert!(render_template(&chained, &fixture(), &limits).is_clean());
}

#[test]
fn function_calls_count_as_evaluations() {
    let template = parse_in("{{ uuid() }}", RenderTarget::Body);
    let mut limits = body_limits();
    limits.max_evaluations = 1;
    assert!(!render_template(&template, &fixture(), &limits).is_clean());
    limits.max_evaluations = 2;
    assert!(render_template(&template, &fixture(), &limits).is_clean());
}

fn render_with_clock(
    source: &str,
    step: Duration,
    deadline: Duration,
) -> (String, Vec<RenderErrorCode>) {
    let template = parse_in(source, RenderTarget::Body);
    let mut limits = body_limits();
    limits.deadline = deadline;
    let mut random = SequenceRandom::counting();
    let clock = SteppingClock::new(step);
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(&fixture(), &limits, &mut env);
    let codes = outcome.errors().iter().map(|error| error.code).collect();
    (outcome.output().to_owned(), codes)
}

#[test]
fn deadline_is_checked_before_each_expression_with_the_injected_clock() {
    let (output, codes) = render_with_clock(
        &"{{ 1 }}".repeat(10),
        Duration::from_millis(10),
        Duration::from_millis(50),
    );
    assert_eq!(output, "1111");
    assert_eq!(codes, vec![RenderErrorCode::DeadlineExceeded]);
}

#[test]
fn deadline_is_checked_between_filters_too() {
    let (output, codes) = render_with_clock(
        "{{ 1 | upper | upper | upper | upper | upper }}",
        Duration::from_millis(10),
        Duration::from_millis(30),
    );
    assert_eq!(output, "");
    assert_eq!(codes, vec![RenderErrorCode::DeadlineExceeded]);
}

#[test]
fn a_zero_deadline_expires_immediately() {
    let (output, codes) = render_with_clock("a{{ 1 }}", Duration::ZERO, Duration::ZERO);
    assert_eq!(output, "a");
    assert_eq!(codes, vec![RenderErrorCode::DeadlineExceeded]);
}

#[test]
fn a_clock_that_never_advances_never_expires() {
    let (output, codes) = render_with_clock(
        &"{{ 1 }}".repeat(300),
        Duration::ZERO,
        Duration::from_millis(1),
    );
    assert_eq!(output.len(), 300);
    assert!(codes.is_empty());
}

#[test]
fn the_system_clock_and_seeded_generator_work_end_to_end() {
    let template = parse_in("{{ uuid() }}", RenderTarget::Body);
    let mut random = ChaChaRandom::from_seed(&RandomSeed::from_bytes(b"run-1"));
    let clock = SystemClock::new();
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(&fixture(), &body_limits(), &mut env);
    assert!(outcome.is_clean());
    assert_eq!(outcome.output().len(), 36);
}

#[test]
fn json_path_depth_limit_applies_to_variable_lookups() {
    let template = parse_in("[{{ var.user.tags[1] }}]", RenderTarget::Body);
    let mut limits = body_limits();
    limits.max_json_depth = 3;
    assert_eq!(
        render_template(&template, &fixture(), &limits).output(),
        "[y]"
    );
    limits.max_json_depth = 2;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::JsonDepthExceeded);
}

#[test]
fn json_lookup_limit_is_fatal() {
    let template = parse_in(
        "{{ var.user.tags[1] }}{{ var.user.tags[1] }}",
        RenderTarget::Body,
    );
    let mut limits = body_limits();
    limits.max_json_lookups = 3;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.output(), "y");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::JsonLookupLimitExceeded
    );
}

#[test]
fn json_queries_count_as_lookups() {
    let template = parse_in(
        "{{ json(\"$.name\") }}{{ json(\"$.name\") }}",
        RenderTarget::Body,
    );
    let mut limits = body_limits();
    limits.max_json_lookups = 1;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.output(), "Zoë");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::JsonLookupLimitExceeded
    );
}

#[test]
fn error_count_is_bounded() {
    let template = parse_in(
        &"{{ \"***\" | base64_decode }}".repeat(20),
        RenderTarget::Body,
    );
    let mut limits = body_limits();
    limits.max_errors = 2;
    let outcome = render_template(&template, &fixture(), &limits);
    assert_eq!(outcome.errors().len(), 3);
    assert_eq!(
        outcome.errors()[2].code,
        RenderErrorCode::ErrorLimitExceeded
    );
}

#[test]
fn header_values_are_capped_at_1024_bytes() {
    let mut context = fixture();
    context
        .vars
        .insert("v".to_owned(), serde_json::json!("a".repeat(1024)));
    let outcome = render_in("{{ var.v }}", RenderTarget::HeaderValue, &context);
    assert!(outcome.is_clean());
    context
        .vars
        .insert("v".to_owned(), serde_json::json!("a".repeat(1025)));
    let outcome = render_in("{{ var.v }}", RenderTarget::HeaderValue, &context);
    assert_eq!(outcome.output(), "");
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::OutputLimitExceeded
    );
}

#[test]
fn status_must_be_three_digits_between_100_and_599() {
    let cases = [
        ("200", true),
        ("{{ 404 }}", true),
        ("599", true),
        ("100", true),
        ("099", false),
        ("600", false),
        ("{{ capture.seq }}", false),
        ("", false),
        ("{{ var.n }}{{ var.n }}", false),
    ];
    for (source, valid) in cases {
        let outcome = render_in(source, RenderTarget::Status, &fixture());
        assert_eq!(outcome.is_clean(), valid, "{source:?}");
    }
    let outcome = render_in("20{{ var.n }}0", RenderTarget::Status, &fixture());
    assert_eq!(
        outcome.errors()[0].code,
        RenderErrorCode::OutputLimitExceeded
    );
    let outcome = render_in("{{ capture.seq }}", RenderTarget::Status, &fixture());
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidStatus);
}

#[test]
fn request_body_text_is_cut_at_64_kib() {
    let mut context = fixture();
    context.request.body = BodyData::new(vec![b'a'; BODY_TEXT_BYTES + 10]);
    let outcome = render_in("{{ request.body | length }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), BODY_TEXT_BYTES.to_string());
}

#[test]
fn invalid_utf8_in_the_body_is_replaced() {
    let mut context = fixture();
    context.request.body = BodyData::new(vec![b'f', 0xff, b'o', 0xc3]);
    let outcome = render_in("{{ request.body }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "f\u{fffd}o\u{fffd}");
}

#[test]
fn error_positions_count_characters_before_the_error() {
    let template = parse_in("héllo {{ \"***\" | base64_decode }}", RenderTarget::Body);
    let outcome = render_template(&template, &fixture(), &body_limits());
    let position = outcome.errors()[0].position;
    assert_eq!(position.line, 1);
    assert_eq!(position.column, 18);
    assert_eq!(position.length, 13);
    assert_eq!(position.offset, 18);
}

#[test]
fn a_fixed_clock_helper_never_trips_the_deadline() {
    let template = parse_in("{{ 1 }}", RenderTarget::Body);
    let mut random = SequenceRandom::counting();
    let clock = FixedClock;
    let mut env = RenderEnv::new(&mut random, &clock);
    assert!(
        template
            .render(&fixture(), &body_limits(), &mut env)
            .is_clean()
    );
}
