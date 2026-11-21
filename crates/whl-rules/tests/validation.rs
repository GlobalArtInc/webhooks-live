use std::collections::BTreeSet;

use serde_json::{Value, json};
use uuid::Uuid;
use whl_rules::{References, Rule, RuleDiagnostic, RuleError, RuleErrorCode, validate_set};
use whl_script::{EngineLimits, ScriptCompiler};

const FORWARD_RULE: &str = "0190c3a2-7d1e-7000-8000-000000000001";
const CHANNEL: &str = "0190c3a2-7d1e-7000-8000-0000000000c1";

fn references() -> References {
    References {
        secrets: BTreeSet::from(["API_KEY".to_owned()]),
        forward_rules: BTreeSet::from([Uuid::parse_str(FORWARD_RULE).expect("a UUID")]),
        channels: BTreeSet::from([Uuid::parse_str(CHANNEL).expect("a UUID")]),
    }
}

fn check(rules: &[Value]) -> Result<Vec<Rule>, RuleError> {
    let scripts =
        ScriptCompiler::new(EngineLimits::default()).expect("the default limits are valid");
    validate_set(rules, &references(), &scripts)
}

fn first_error(rules: &[Value]) -> RuleDiagnostic {
    match check(rules) {
        Ok(_) => panic!("the rules were accepted"),
        Err(error) => error
            .diagnostics
            .into_iter()
            .next()
            .expect("at least one diagnostic"),
    }
}

fn expect_rejected(rules: Vec<Value>, field: &str, code: RuleErrorCode) -> RuleDiagnostic {
    let diagnostic = first_error(&rules);
    assert_eq!(diagnostic.field, field, "{}", diagnostic.message);
    assert_eq!(diagnostic.code, code, "{}", diagnostic.message);
    diagnostic
}

fn stop() -> Value {
    json!({"kind": "stop"})
}

fn with_steps(steps: Vec<Value>) -> Value {
    json!({"name": "rule", "steps": steps})
}

fn one_step(step: Value) -> Value {
    with_steps(vec![step])
}

fn repeated(value: &str, count: usize) -> String {
    value.repeat(count)
}

#[test]
fn accepts_a_rule_set_using_every_reference_kind() {
    let rules = vec![
        json!({
            "name": "match",
            "condition": {"all": [
                {"value": "request.header.x-real-ip", "op": "in_cidr", "arg": ["10.0.0.0/8"]},
                {"not": {"value": "request.query.token", "op": "exists"}}
            ]},
            "steps": [
                {"name": "lookup", "kind": "http", "method": "get", "url": "https://h.example/{{ secret.API_KEY }}"},
                {"kind": "forward", "forward_rule_id": FORWARD_RULE},
                {"kind": "store_s3", "bucket_url": "https://s3.example/b", "region": "eu-1",
                 "access_key_secret": "API_KEY", "secret_key_secret": "API_KEY", "content": "body"}
            ]
        }),
        json!({"name": "answer", "steps": [
            {"kind": "respond", "status": "202", "body": "{{ step.lookup.status }}"}
        ]}),
    ];
    let accepted = check(&rules).expect("the rule set is valid");
    assert_eq!(accepted.len(), 2);
    assert!(accepted[0].enabled);
}

#[test]
fn rejects_an_unknown_rule_field() {
    let mut rule = one_step(stop());
    rule["id"] = json!("x");
    expect_rejected(vec![rule], "rules.0.id", RuleErrorCode::NotAllowed);
}

#[test]
fn rejects_an_empty_rule_name() {
    let rule = json!({"name": "", "steps": [stop()]});
    expect_rejected(vec![rule], "rules.0.name", RuleErrorCode::Required);
}

#[test]
fn rejects_a_rule_name_over_one_hundred_characters() {
    let rule = json!({"name": repeated("n", 101), "steps": [stop()]});
    expect_rejected(vec![rule], "rules.0.name", RuleErrorCode::TooLong);
}

#[test]
fn rejects_a_rule_without_steps() {
    let rule = json!({"name": "rule"});
    expect_rejected(vec![rule], "rules.0.steps", RuleErrorCode::Required);
}

#[test]
fn rejects_more_than_ten_steps_in_one_rule() {
    let rule = with_steps(vec![stop(); 11]);
    expect_rejected(vec![rule], "rules.0.steps", RuleErrorCode::TooMany);
}

#[test]
fn rejects_an_unknown_step_kind() {
    let rule = one_step(json!({"kind": "teleport"}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.kind",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_step_name_outside_the_pattern() {
    let rule = one_step(json!({"name": "Bad-Name", "kind": "stop"}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.name",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_step_name_used_by_an_earlier_rule() {
    let rules = vec![
        one_step(json!({"name": "lookup", "kind": "delay", "ms": 0})),
        one_step(json!({"name": "lookup", "kind": "stop"})),
    ];
    expect_rejected(rules, "rules.1.steps.0.name", RuleErrorCode::Duplicate);
}

#[test]
fn rejects_a_template_reading_a_step_that_does_not_exist_yet() {
    let rule = with_steps(vec![
        json!({"kind": "respond", "status": "200", "body": "{{ step.later.body }}"}),
        json!({"name": "later", "kind": "stop"}),
    ]);
    let diagnostic = expect_rejected(
        vec![rule],
        "rules.0.steps.0.body",
        RuleErrorCode::InvalidFormat,
    );
    assert!(
        diagnostic.message.contains("step_unknown"),
        "{}",
        diagnostic.message
    );
}

#[test]
fn rejects_a_template_syntax_error_with_its_position() {
    let rule = one_step(json!({"kind": "set_header", "name": "x-a", "value": "{{ request.method"}));
    let diagnostic = expect_rejected(
        vec![rule],
        "rules.0.steps.0.value",
        RuleErrorCode::InvalidFormat,
    );
    let position = diagnostic.position.expect("the error has a position");
    assert_eq!((position.line, position.column), (1, 1));
}

#[test]
fn rejects_a_secret_in_a_response_template() {
    let rule =
        one_step(json!({"kind": "respond", "status": "200", "body": "{{ secret.API_KEY }}"}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.body",
        RuleErrorCode::NotAllowed,
    );
}

#[test]
fn rejects_a_secret_in_a_notify_message() {
    let rule = one_step(
        json!({"kind": "notify", "channel_id": CHANNEL, "message": "{{ secret.API_KEY }}"}),
    );
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.message",
        RuleErrorCode::NotAllowed,
    );
}

#[test]
fn rejects_a_secret_name_that_is_not_defined() {
    let rule = one_step(
        json!({"kind": "http", "method": "GET", "url": "https://h.example/{{ secret.OTHER }}"}),
    );
    let diagnostic = expect_rejected(
        vec![rule],
        "rules.0.steps.0.url",
        RuleErrorCode::InvalidFormat,
    );
    assert!(
        diagnostic.message.contains("OTHER"),
        "{}",
        diagnostic.message
    );
}

#[test]
fn rejects_more_than_twenty_response_headers() {
    let headers: Vec<Value> = (0..21)
        .map(|index| json!({"name": format!("x-{index}"), "value": "1"}))
        .collect();
    let rule = one_step(json!({"kind": "respond", "status": "200", "headers": headers}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.headers",
        RuleErrorCode::TooMany,
    );
}

#[test]
fn rejects_a_delay_over_ten_seconds() {
    let rule = one_step(json!({"kind": "delay", "ms": 10001}));
    expect_rejected(vec![rule], "rules.0.steps.0.ms", RuleErrorCode::OutOfRange);
}

#[test]
fn rejects_an_http_timeout_over_fifteen_seconds() {
    let rule = one_step(
        json!({"kind": "http", "method": "GET", "url": "https://h.example/", "timeout_ms": 15001}),
    );
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.timeout_ms",
        RuleErrorCode::OutOfRange,
    );
}

#[test]
fn rejects_a_script_that_does_not_compile_with_its_position() {
    let rule = one_step(json!({"kind": "script", "source": "let x = ;"}));
    let diagnostic = expect_rejected(
        vec![rule],
        "rules.0.steps.0.source",
        RuleErrorCode::InvalidFormat,
    );
    assert!(diagnostic.position.is_some());
}

#[test]
fn rejects_an_unknown_condition_operator() {
    let rule = json!({"name": "rule", "condition": {"value": "request.method", "op": "like", "arg": "P"}, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.op",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_condition_nested_deeper_than_four_levels() {
    let leaf = json!({"value": "request.method", "op": "exists"});
    let condition = json!({"not": {"not": {"not": {"not": leaf}}}});
    let rule = json!({"name": "rule", "condition": condition, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.not.not.not.not",
        RuleErrorCode::TooMany,
    );
}

#[test]
fn rejects_a_condition_with_more_than_thirty_two_leaves() {
    let leaves: Vec<Value> = (0..33)
        .map(|_| json!({"value": "request.method", "op": "exists"}))
        .collect();
    let rule = json!({"name": "rule", "condition": {"all": leaves}, "steps": [stop()]});
    expect_rejected(vec![rule], "rules.0.condition", RuleErrorCode::TooMany);
}

#[test]
fn rejects_an_argument_on_exists() {
    let rule = json!({"name": "rule", "condition": {"value": "request.method", "op": "exists", "arg": "GET"}, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.arg",
        RuleErrorCode::NotAllowed,
    );
}

#[test]
fn rejects_a_regex_that_does_not_compile() {
    let rule = json!({"name": "rule", "condition": {"value": "request.path", "op": "matches", "arg": "("}, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.arg",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_cidr_list_with_a_value_that_is_not_a_range() {
    let rule = json!({"name": "rule", "condition": {"value": "request.ip", "op": "in_cidr", "arg": ["not-a-range"]}, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.arg",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_number_comparison_against_text() {
    let rule = json!({"name": "rule", "condition": {"value": "request.method", "op": "gt", "arg": "5"}, "steps": [stop()]});
    expect_rejected(
        vec![rule],
        "rules.0.condition.arg",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_forward_rule_that_does_not_exist_on_the_endpoint() {
    let rule = one_step(
        json!({"kind": "forward", "forward_rule_id": "0190c3a2-7d1e-7000-8000-000000000002"}),
    );
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.forward_rule_id",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_notify_step_whose_channel_is_not_on_the_endpoint() {
    let rule =
        one_step(json!({"kind": "notify", "channel_id": "0190c3a2-7d1e-7000-8000-0000000000c2"}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.channel_id",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn accepts_a_notify_step_whose_channel_is_on_the_endpoint() {
    let rule = one_step(json!({"kind": "notify", "channel_id": CHANNEL}));
    assert!(check(&[rule]).is_ok());
}

#[test]
fn rejects_a_forward_step_with_both_a_rule_and_a_target() {
    let rule = one_step(
        json!({"kind": "forward", "forward_rule_id": FORWARD_RULE, "target_url": "https://h.example/"}),
    );
    expect_rejected(vec![rule], "rules.0.steps.0", RuleErrorCode::InvalidFormat);
}

#[test]
fn rejects_an_extract_with_both_a_json_path_and_a_regex() {
    let rule = one_step(
        json!({"kind": "extract", "from": "request.body", "json_path": "$.a", "regex": "a", "into": "a"}),
    );
    expect_rejected(vec![rule], "rules.0.steps.0", RuleErrorCode::InvalidFormat);
}

#[test]
fn rejects_a_json_path_that_does_not_parse() {
    let rule = one_step(
        json!({"kind": "extract", "from": "request.body", "json_path": "$[", "into": "a"}),
    );
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.json_path",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_sixth_rule_in_one_endpoint() {
    let rules: Vec<Value> = (0..6).map(|_| one_step(stop())).collect();
    expect_rejected(rules, "rules", RuleErrorCode::TooMany);
}

#[test]
fn rejects_a_rule_set_serialized_over_128_kib() {
    let body = repeated("a", 60 * 1024);
    let rules: Vec<Value> = (0..3)
        .map(|_| one_step(json!({"kind": "respond", "status": "200", "body": body})))
        .collect();
    expect_rejected(rules, "rules", RuleErrorCode::TooLong);
}

#[test]
fn rejects_a_store_s3_secret_that_is_not_defined() {
    let rule = one_step(json!({
        "kind": "store_s3", "bucket_url": "https://s3.example/b", "region": "eu-1",
        "access_key_secret": "MISSING", "secret_key_secret": "API_KEY", "content": "body"
    }));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.access_key_secret",
        RuleErrorCode::InvalidFormat,
    );
}

#[test]
fn rejects_a_header_name_with_a_space() {
    let rule = one_step(json!({"kind": "set_header", "name": "x bad", "value": "1"}));
    expect_rejected(
        vec![rule],
        "rules.0.steps.0.name",
        RuleErrorCode::InvalidFormat,
    );
}
