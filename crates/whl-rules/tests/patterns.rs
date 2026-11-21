mod common;

use std::time::{Duration, Instant};

use common::{compile, compiler, context, machine, respond, rule, rule_entries, run};
use serde_json::{Value, json};
use whl_rules::{
    Advance, FailureCode, MAX_REGEX_REPETITION, Mode, Pattern, PatternError, References,
    RuleErrorCode, RunOutcome, bounded, largest_repetition, validate_set,
};
use whl_template::BodyData;

const HOSTILE: &str = r"[ab]*a[ab]{10000}\d";

fn ab_body(length: usize) -> String {
    let mut state: u32 = 0x1234_5678;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if state >> 16 & 1 == 0 { 'a' } else { 'b' }
        })
        .collect()
}

fn matching(pattern: &str) -> Value {
    rule(
        "probe",
        json!({"value": "request.body", "op": "matches", "arg": pattern}),
        vec![respond("200", "x")],
    )
}

fn extracting(pattern: &str) -> Value {
    rule(
        "probe",
        json!({}),
        vec![
            json!({"kind": "extract", "from": "request.body", "regex": pattern, "into": "found"}),
            respond("200", "x"),
        ],
    )
}

fn refusal(rule: Value) -> (String, RuleErrorCode) {
    let error = validate_set(&[rule], &References::default(), &compiler())
        .expect_err("the pattern is refused");
    let diagnostic = error.diagnostics.into_iter().next().expect("a diagnostic");
    (diagnostic.field, diagnostic.code)
}

#[test]
fn validation_refuses_the_pathological_pattern_in_a_condition_and_in_an_extract() {
    assert_eq!(
        refusal(matching(HOSTILE)),
        (
            "rules.0.condition.arg".to_owned(),
            RuleErrorCode::InvalidFormat
        )
    );
    assert_eq!(
        refusal(extracting(HOSTILE)),
        (
            "rules.0.steps.0.regex".to_owned(),
            RuleErrorCode::InvalidFormat
        )
    );
}

#[test]
fn compiling_refuses_what_validation_refuses() {
    assert_eq!(
        Pattern::compile(HOSTILE).expect_err("refused"),
        PatternError::Repetition
    );
}

#[test]
fn pattern_limits_cap_the_length_the_repetition_and_the_compiled_size() {
    let long = "a".repeat(513);
    assert_eq!(
        Pattern::compile(&long).expect_err("refused"),
        PatternError::TooLong
    );
    for pattern in [
        "a{201}",
        "a{1,500}",
        "(?x) a{ 300 }",
        "(a{200}){200}",
        r"\w{200}",
    ] {
        assert!(Pattern::compile(pattern).is_err(), "{pattern}");
    }
    for pattern in [
        "a{200}",
        "[{500}]",
        r"\x{1234}",
        r"\p{Greek}{3}",
        "a{2,200}",
    ] {
        assert!(Pattern::compile(pattern).is_ok(), "{pattern}");
    }
    assert_eq!(largest_repetition("a{3}b{7,9}"), 9);
    assert_eq!(MAX_REGEX_REPETITION, 200);
}

#[test]
fn matching_sees_only_the_first_four_kibibytes_of_a_value() {
    let tail_only = format!("{}z", "a".repeat(5000));

    assert_eq!(bounded(&tail_only).len(), 4096);
    assert!(Pattern::compile("z$").is_ok_and(|pattern| !pattern.is_match(&tail_only)));
    assert!(Pattern::compile("^a+").is_ok_and(|pattern| pattern.is_match(&tail_only)));
}

#[tokio::test]
async fn a_condition_regex_over_a_large_random_body_finishes_quickly() {
    let rules = compile(vec![matching(r"[ab]*a[ab]{50}\d")]);
    let mut request = context();
    request.request.body = BodyData::from_text(&ab_body(65_536));
    let started = Instant::now();

    let finished = run(&rules, request).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert!(!rule_entries(&finished)[0].matched);
    assert!(started.elapsed() < Duration::from_secs(2));
}

fn assert_fails_on_budget_before_matching(rule: Value) {
    let rules = compile(vec![rule]);
    let mut request = context();
    request.request.body = BodyData::from_text(&ab_body(65_536));
    let started = Instant::now();

    let mut running = machine(&rules, request, Mode::Live, Duration::from_millis(1));
    let finished = match running.advance() {
        Advance::Finished(finished) => finished,
        Advance::Effect(_) => panic!("no effect is expected"),
    };

    assert!(started.elapsed() < Duration::from_millis(50));
    match &finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Budget),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}

#[test]
fn a_condition_regex_that_would_not_finish_inside_the_remaining_budget_fails_before_it_runs() {
    assert_fails_on_budget_before_matching(matching(r"[ab]*a[ab]{190}[0-9a-f]"));
}

#[test]
fn an_extract_regex_that_would_not_finish_inside_the_remaining_budget_fails_before_it_runs() {
    assert_fails_on_budget_before_matching(extracting(r"[ab]*a[ab]{190}[0-9a-f]"));
}
