mod common;

use common::{compile, context, respond, rule, rule_entries, run};
use serde_json::{Value, json};
use whl_template::NameValue;

fn leaf(value: &str, op: &str, arg: Value) -> Value {
    json!({"value": value, "op": op, "arg": arg})
}

fn exists(value: &str) -> Value {
    json!({"value": value, "op": "exists"})
}

async fn matched(condition: Value) -> bool {
    matched_in(condition, context()).await
}

async fn matched_in(condition: Value, request: whl_template::Context) -> bool {
    let rules = compile(vec![rule("probe", condition, vec![respond("200", "x")])]);
    let finished = run(&rules, request).await;
    rule_entries(&finished)[0].matched
}

#[tokio::test]
async fn every_operator_decides_as_documented_on_a_matching_and_a_refusing_value() {
    let cases = [
        (leaf("request.method", "eq", json!("POST")), true),
        (leaf("request.method", "eq", json!("GET")), false),
        (leaf("request.method", "ne", json!("GET")), true),
        (leaf("request.method", "ne", json!("POST")), false),
        (leaf("request.path", "contains", json!("orders")), true),
        (leaf("request.path", "contains", json!("zzz")), false),
        (leaf("request.path", "starts_with", json!("/orders")), true),
        (leaf("request.path", "starts_with", json!("orders")), false),
        (leaf("request.path", "ends_with", json!("42")), true),
        (leaf("request.path", "ends_with", json!("43")), false),
        (
            leaf("request.path", "matches", json!("^/orders/[0-9]+$")),
            true,
        ),
        (leaf("request.path", "matches", json!("^/items/")), false),
        (exists("request.header.x-token"), true),
        (exists("request.header.x-absent"), false),
        (leaf("request.query.page", "gt", json!(2)), true),
        (leaf("request.query.page", "gt", json!(3)), false),
        (leaf("request.query.page", "lt", json!(4)), true),
        (leaf("request.query.page", "lt", json!(3)), false),
        (leaf("request.query.page", "eq", json!(3)), true),
        (leaf("request.method", "in", json!(["POST", "PUT"])), true),
        (leaf("request.method", "in", json!(["GET", "PUT"])), false),
        (
            leaf("request.ip", "in_cidr", json!(["203.0.113.0/24"])),
            true,
        ),
        (leaf("request.ip", "in_cidr", json!(["10.0.0.0/8"])), false),
        (leaf("request.body", "contains", json!("gift")), true),
    ];

    for (condition, expected) in cases {
        assert_eq!(matched(condition.clone()).await, expected, "{condition}");
    }
}

#[tokio::test]
async fn an_empty_condition_always_matches_and_the_combinators_combine_their_leaves() {
    let yes = leaf("request.method", "eq", json!("POST"));
    let no = leaf("request.method", "eq", json!("GET"));

    assert!(matched(json!({})).await);
    assert!(matched(json!({"all": [yes.clone(), yes.clone()]})).await);
    assert!(!matched(json!({"all": [yes.clone(), no.clone()]})).await);
    assert!(matched(json!({"any": [no.clone(), yes.clone()]})).await);
    assert!(!matched(json!({"any": [no.clone(), no.clone()]})).await);
    assert!(matched(json!({"not": no})).await);
    assert!(!matched(json!({"not": yes})).await);
}

#[tokio::test]
async fn a_missing_variable_fails_every_comparison_except_not_equals() {
    assert!(!matched(leaf("request.header.x-absent", "eq", json!(""))).await);
    assert!(!matched(leaf("request.header.x-absent", "contains", json!(""))).await);
    assert!(!matched(leaf("request.header.x-absent", "gt", json!(0))).await);
    assert!(matched(leaf("request.header.x-absent", "ne", json!("anything"))).await);
}

#[tokio::test]
async fn a_non_numeric_value_never_satisfies_an_ordering() {
    assert!(!matched(leaf("request.method", "gt", json!(1))).await);
    assert!(!matched(leaf("request.method", "lt", json!(1))).await);
}

#[tokio::test]
async fn a_later_rule_sees_the_variables_an_earlier_rule_set() {
    let rules = compile(vec![
        rule(
            "setter",
            json!({}),
            vec![json!({"kind": "set_var", "into": "flag", "value": "yes"})],
        ),
        rule(
            "reader",
            leaf("var.flag", "eq", json!("yes")),
            vec![respond("200", "x")],
        ),
    ]);

    let finished = run(&rules, context()).await;

    let entries = rule_entries(&finished);
    assert!(entries[0].matched);
    assert!(entries[1].matched);
}

#[tokio::test]
async fn the_trigger_kind_and_its_meta_are_subjects_of_a_condition() {
    let mut email = context();
    email.request.kind = "email".to_owned();
    email.request.meta = vec![NameValue::new("subject", "Invoice")];
    let condition = json!({"all": [
        leaf("request.kind", "eq", json!("email")),
        leaf("request.meta.subject", "starts_with", json!("Inv")),
    ]});

    assert!(matched_in(condition.clone(), email).await);
    assert!(!matched_in(condition, context()).await);
}
