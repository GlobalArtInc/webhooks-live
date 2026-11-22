mod common;

use std::collections::BTreeSet;

use common::{compile, compiler, context, respond, rule, rule_id, run};
use serde_json::{Value, json};
use uuid::Uuid;
use whl_rules::{
    CompiledRules, PlanReadError, References, Rule, StoredRule, load_stored, rules_from_plan,
    validate_set,
};

fn every_shape() -> Vec<Value> {
    vec![rule(
        "wide",
        json!({"all": [
            {"value": "request.method", "op": "eq", "arg": "POST"},
            {"not": {"value": "request.header.x-skip", "op": "exists"}},
        ]}),
        vec![
            json!({"kind": "extract", "from": "request.body", "json_path": "$.order.id", "into": "id"}),
            json!({"kind": "extract", "from": "request.path", "regex": "^/(.*)$", "into": "rest"}),
            respond("200", "{{ var.id }}"),
            json!({"kind": "http", "method": "GET", "url": "https://example.test/{{ var.id }}"}),
            json!({"kind": "forward", "target_url": "https://example.test/hook"}),
            json!({"kind": "notify", "channel_id": "0190c3a2-7d1e-7000-8000-000000000001"}),
        ],
    )]
}

fn validated(inputs: &[Value]) -> Vec<Rule> {
    let references = References {
        channels: BTreeSet::from([Uuid::from_u128(0x0190_c3a2_7d1e_7000_8000_0000_0000_0001)]),
        ..References::default()
    };
    validate_set(inputs, &references, &compiler()).expect("the rules are valid")
}

fn stored_of(rules: &[Rule]) -> Vec<StoredRule> {
    rules
        .iter()
        .enumerate()
        .map(|(index, rule)| StoredRule {
            id: rule_id(index),
            name: rule.name.clone(),
            enabled: rule.enabled,
            condition: serde_json::to_value(&rule.condition).expect("the condition encodes"),
            steps: serde_json::to_value(&rule.steps).expect("the steps encode"),
        })
        .collect()
}

#[test]
fn a_rule_set_survives_the_trip_through_its_stored_json() {
    let rules = validated(&every_shape());

    let loaded = load_stored(&stored_of(&rules), &compiler()).expect("the stored rules load");

    assert_eq!(
        loaded
            .into_iter()
            .map(|source| source.rule)
            .collect::<Vec<_>>(),
        rules
    );
}

#[tokio::test]
async fn the_plan_of_the_after_part_loads_back_into_the_same_remaining_rules() {
    let rules = compile(vec![
        rule(
            "first",
            json!({}),
            vec![
                json!({"kind": "extract", "from": "request.body", "json_path": "$.order.id", "into": "id"}),
                respond("200", "ok"),
                json!({"kind": "set_var", "into": "late", "value": "{{ var.id }}"}),
            ],
        ),
        rule(
            "second",
            json!({}),
            vec![json!({"kind": "set_var", "into": "other", "value": "2"})],
        ),
    ]);
    let mut request = context();
    request.request.body = whl_template::BodyData::from_text(r#"{"order":{"id":7}}"#);

    let finished = run(&rules, request).await;
    let plan = finished
        .after
        .plan(rules.rules())
        .expect("the plan encodes");
    let remaining = rules_from_plan(&plan).expect("the plan lists its rules");
    let loaded = load_stored(&remaining, &compiler()).expect("the remaining rules load");

    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0].id, rule_id(0));
    assert_eq!(
        loaded
            .iter()
            .map(|source| source.rule.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(plan["resume"], json!({"rule": 0, "step": 2}));
    let unknown = Uuid::nil();
    assert!(loaded.iter().all(|source| source.id != unknown));
}

#[test]
fn a_plan_of_another_version_is_refused_instead_of_misread() {
    let old = json!({"version": 1, "rules": [], "resume": null, "deferred": [], "payload": null});
    let none = json!({"rules": []});

    assert!(matches!(
        rules_from_plan(&old),
        Err(PlanReadError::Version { found: Some(1) })
    ));
    assert!(matches!(
        rules_from_plan(&none),
        Err(PlanReadError::Version { found: None })
    ));
}

#[test]
fn each_compiled_rule_lists_the_secrets_it_alone_reads() {
    let inputs = vec![
        rule(
            "calls",
            json!({}),
            vec![
                json!({"kind": "http", "method": "GET", "url": "https://h.example/{{ secret.API_KEY }}"}),
            ],
        ),
        rule(
            "stores",
            json!({}),
            vec![
                json!({"kind": "store_s3", "bucket_url": "https://s3.example/b", "region": "eu-1",
                "access_key_secret": "S3_ID", "secret_key_secret": "S3_KEY", "content": "body"}),
            ],
        ),
        rule(
            "plain",
            json!({}),
            vec![json!({"kind": "set_var", "into": "x", "value": "y"})],
        ),
    ];
    let references = References {
        secrets: ["API_KEY", "S3_ID", "S3_KEY"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ..References::default()
    };
    let scripts = compiler();
    let rules = validate_set(&inputs, &references, &scripts).expect("the rules are valid");
    let stored = stored_of(&rules);

    let sources = load_stored(&stored, &scripts).expect("the stored rules load");
    let compiled = CompiledRules::compile(sources, &scripts).expect("the rules compile");

    let names: Vec<Vec<String>> = compiled
        .rules()
        .iter()
        .map(|rule| rule.secret_names().iter().cloned().collect())
        .collect();
    assert_eq!(
        names,
        vec![
            vec!["API_KEY".to_owned()],
            vec!["S3_ID".to_owned(), "S3_KEY".to_owned()],
            Vec::<String>::new()
        ]
    );
    assert_eq!(compiled.secret_names().len(), 3);
}
