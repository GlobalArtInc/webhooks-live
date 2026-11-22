mod common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{bodiless, create, field_codes, harness, request, send};
use serde_json::{Value, json};
use std::time::Duration;
use whl_core::endpoint_id::EndpointId;
use whl_rules::load_set;
use whl_script::{EngineLimits, ScriptCompiler};
use whl_store::Store;
use whl_store::rules::{RuleChange, WriteOutcome};

async fn endpoint(app: &Router) -> String {
    create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

fn stop_rule(name: &str) -> String {
    json!({"name": name, "steps": [{"kind": "stop"}]}).to_string()
}

async fn add(app: &Router, endpoint: &str, body: &str) -> Value {
    let reply = send(
        app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            body,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.json());
    reply.json()
}

async fn listing(app: &Router, endpoint: &str) -> Value {
    let reply = send(
        app,
        bodiless(Method::GET, &format!("/api/v1/endpoints/{endpoint}/rules")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json()
}

#[tokio::test]
async fn create_appends_the_rule_and_bumps_the_version() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let created = add(&harness.app, &endpoint, &stop_rule("answer")).await;
    assert_eq!(created["name"], "answer");
    assert_eq!(created["position"], 1);
    let list = listing(&harness.app, &endpoint).await;
    assert_eq!(list["version"], 1);
    assert_eq!(list["items"][0]["id"], created["id"]);
}

#[tokio::test]
async fn an_invalid_rule_reports_its_field_path_and_code() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let body = json!({"name": "answer", "steps": [{"kind": "teleport"}]}).to_string();
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            &body,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("steps.0.kind".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn a_sixth_rule_is_refused_with_the_limit_named() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    for index in 0..5 {
        add(&harness.app, &endpoint, &stop_rule(&format!("rule{index}"))).await;
    }
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            &stop_rule("sixth"),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "limit_reached");
    assert_eq!(error["error"]["limit"]["name"], "rules");
    assert_eq!(error["error"]["limit"]["max"], 5);
}

#[tokio::test]
async fn a_patch_merges_the_fields_and_revalidates_the_steps() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let created = add(&harness.app, &endpoint, &stop_rule("answer")).await;
    let uri = format!(
        "/api/v1/endpoints/{endpoint}/rules/{}",
        created["id"].as_str().unwrap_or_default()
    );
    let disabled = send(
        &harness.app,
        request(Method::PATCH, &uri, r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::OK);
    assert_eq!(disabled.json()["enabled"], false);
    assert_eq!(disabled.json()["name"], "answer");
    let broken = send(
        &harness.app,
        request(Method::PATCH, &uri, r#"{"steps":[{"kind":"teleport"}]}"#),
    )
    .await;
    assert_eq!(broken.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&broken.json()),
        vec![("steps.0.kind".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn a_deleted_rule_is_not_found_afterwards() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let created = add(&harness.app, &endpoint, &stop_rule("answer")).await;
    let uri = format!(
        "/api/v1/endpoints/{endpoint}/rules/{}",
        created["id"].as_str().unwrap_or_default()
    );
    let deleted = send(&harness.app, bodiless(Method::DELETE, &uri)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let shown = send(&harness.app, bodiless(Method::GET, &uri)).await;
    assert_eq!(shown.status, StatusCode::NOT_FOUND);
    assert_eq!(shown.json()["error"]["code"], "rule_not_found");
}

#[tokio::test]
async fn an_unknown_rule_id_is_not_found() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let uri = format!("/api/v1/endpoints/{endpoint}/rules/00000000-0000-0000-0000-000000000000");
    let reply = send(
        &harness.app,
        request(Method::PATCH, &uri, r#"{"enabled":false}"#),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "rule_not_found");
}

#[tokio::test]
async fn reorder_applies_the_order_and_refuses_a_partial_list() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let first = add(&harness.app, &endpoint, &stop_rule("first")).await;
    let second = add(&harness.app, &endpoint, &stop_rule("second")).await;
    let order = format!(
        r#"{{"ids":["{}","{}"]}}"#,
        second["id"].as_str().unwrap_or_default(),
        first["id"].as_str().unwrap_or_default()
    );
    let uri = format!("/api/v1/endpoints/{endpoint}/rules/order");
    let reordered = send(&harness.app, request(Method::PUT, &uri, &order)).await;
    assert_eq!(reordered.status, StatusCode::OK);
    let body = reordered.json();
    let names: Vec<&str> = body["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(names, ["second", "first"]);
    let partial = format!(
        r#"{{"ids":["{}"]}}"#,
        first["id"].as_str().unwrap_or_default()
    );
    let refused = send(&harness.app, request(Method::PUT, &uri, &partial)).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&refused.json()),
        vec![("ids".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn a_secret_referenced_by_a_rule_must_exist() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let body = json!({"name": "call", "steps": [{
        "kind": "http", "method": "GET", "url": "https://h.example/{{ secret.MISSING }}"
    }]})
    .to_string();
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            &body,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("steps.0.url".to_owned(), "invalid_format".to_owned())]
    );
}

fn fetch_before_response(name: &str) -> Value {
    json!({"name": name, "steps": [
        {"kind": "set_var", "into": "a", "value": "x"},
        {"kind": "http", "method": "GET", "url": "https://h.example/"},
        {"kind": "respond", "status": "200", "body": "x"},
    ]})
}

#[tokio::test]
async fn an_http_step_before_the_response_is_refused_on_create_and_update() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let uri = format!("/api/v1/endpoints/{endpoint}/rules");

    let created = send(
        &harness.app,
        request(
            Method::POST,
            &uri,
            &fetch_before_response("fetch").to_string(),
        ),
    )
    .await;
    let saved = add(
        &harness.app,
        &endpoint,
        &json!({"name": "later", "steps": [
            {"kind": "http", "method": "GET", "url": "https://h.example/"}
        ]})
        .to_string(),
    )
    .await;
    let patch = json!({"steps": fetch_before_response("x")["steps"].clone()}).to_string();
    let updated = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("{uri}/{}", saved["id"].as_str().unwrap_or_default()),
            &patch,
        ),
    )
    .await;

    for reply in [created, updated] {
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            field_codes(&reply.json()),
            vec![("steps.1.kind".to_owned(), "not_allowed".to_owned())]
        );
    }
}

#[tokio::test]
async fn a_new_response_rule_that_moves_the_cut_past_an_http_step_is_refused() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    add(
        &harness.app,
        &endpoint,
        &json!({"name": "later", "steps": [
            {"kind": "http", "method": "GET", "url": "https://h.example/"}
        ]})
        .to_string(),
    )
    .await;

    let reply = send(
        &harness.app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            &json!({"name": "answer", "steps": [
                {"kind": "respond", "status": "200", "body": "x"}
            ]})
            .to_string(),
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("rules.0.steps.0.kind".to_owned(), "not_allowed".to_owned())]
    );
}

fn legacy_fetch(name: &str) -> Value {
    json!({"name": name, "steps": [
        {"kind": "http", "method": "GET", "url": "https://h.example/"}
    ]})
}

async fn seed_legacy_set(harness: &common::Harness, endpoint: &str) -> Vec<String> {
    let store = Store::from_pool(harness.db.pool().clone());
    let scripts = ScriptCompiler::new(EngineLimits::default()).expect("the limits are valid");
    let id = EndpointId::parse(endpoint).expect("a valid endpoint id");
    let inputs = [
        legacy_fetch("a"),
        legacy_fetch("b"),
        json!({"name": "c", "steps": [{"kind": "respond", "status": "200", "body": "x"}]}),
    ];
    for input in inputs {
        let rule = load_set(&[input], &scripts)
            .expect("the stored rule loads")
            .remove(0);
        let version = store
            .list_rules(&id)
            .await
            .expect("the rules are listed")
            .version;
        let outcome = store
            .write_rules(
                &id,
                Duration::from_secs(3600),
                version,
                &RuleChange::Create(rule),
            )
            .await
            .expect("the write runs");
        assert!(matches!(outcome, WriteOutcome::Written(_)), "{outcome:?}");
    }
    listing(&harness.app, endpoint).await["items"]
        .as_array()
        .expect("the items are listed")
        .iter()
        .map(|item| item["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test]
async fn a_legacy_rule_can_be_deleted_while_another_one_still_has_http_before_the_response() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let ids = seed_legacy_set(&harness, &endpoint).await;
    let uri = |id: &str| format!("/api/v1/endpoints/{endpoint}/rules/{id}");

    let deleted_b = send(&harness.app, bodiless(Method::DELETE, &uri(&ids[1]))).await;
    let deleted_a = send(&harness.app, bodiless(Method::DELETE, &uri(&ids[0]))).await;

    assert_eq!(deleted_b.status, StatusCode::NO_CONTENT);
    assert_eq!(deleted_a.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_legacy_rule_can_be_repaired_and_the_set_reordered_while_another_one_is_not() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let ids = seed_legacy_set(&harness, &endpoint).await;

    let patched = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}/rules/{}", ids[0]),
            &json!({"steps": [{"kind": "set_var", "into": "v", "value": "x"}]}).to_string(),
        ),
    )
    .await;
    let reordered = send(
        &harness.app,
        request(
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/rules/order"),
            &json!({"ids": [ids[1], ids[2], ids[0]]}).to_string(),
        ),
    )
    .await;

    assert_eq!(patched.status, StatusCode::OK, "{}", patched.json());
    assert_eq!(reordered.status, StatusCode::OK, "{}", reordered.json());
}

#[tokio::test]
async fn a_legacy_violation_does_not_excuse_a_new_one() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let ids = seed_legacy_set(&harness, &endpoint).await;

    let touched = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}/rules/{}", ids[0]),
            &json!({"name": "renamed"}).to_string(),
        ),
    )
    .await;
    let created = send(
        &harness.app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/rules"),
            &fetch_before_response("new").to_string(),
        ),
    )
    .await;

    assert_eq!(touched.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&touched.json()),
        vec![("steps.0.kind".to_owned(), "not_allowed".to_owned())]
    );
    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        field_codes(&created.json())
            .contains(&("steps.1.kind".to_owned(), "not_allowed".to_owned()))
    );
}

#[tokio::test]
async fn a_reorder_that_moves_the_cut_past_an_http_step_is_refused_with_the_new_position() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let first = add(
        &harness.app,
        &endpoint,
        &json!({"name": "answer", "steps": [{"kind": "respond", "status": "200", "body": "x"}]})
            .to_string(),
    )
    .await;
    let second = add(&harness.app, &endpoint, &legacy_fetch("fetch").to_string()).await;

    let reply = send(
        &harness.app,
        request(
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/rules/order"),
            &json!({"ids": [second["id"], first["id"]]}).to_string(),
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("rules.0.steps.0.kind".to_owned(), "not_allowed".to_owned())]
    );
}

#[tokio::test]
async fn a_stored_rule_that_no_longer_loads_can_still_be_deleted() {
    let harness = harness().await;
    let endpoint = endpoint(&harness.app).await;
    let id: whl_store::Uuid = sqlx::query_scalar(
        "INSERT INTO rules (id, endpoint_id, position, name, steps) \
         VALUES (gen_random_uuid(), $1, 1, 'broken', '[{\"kind\": \"teleport\"}]'::jsonb) \
         RETURNING id",
    )
    .bind(&endpoint)
    .fetch_one(harness.db.pool())
    .await
    .expect("the broken rule is inserted");

    let reply = send(
        &harness.app,
        bodiless(
            Method::DELETE,
            &format!("/api/v1/endpoints/{endpoint}/rules/{id}"),
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(listing(&harness.app, &endpoint).await["items"], json!([]));
}
