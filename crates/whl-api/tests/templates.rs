mod common;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{Reply, bodiless, create, field_codes, harness, request, send};
use serde_json::{Value, json};

async fn patch(app: &Router, id: &str, body: Value) -> Reply {
    send(
        app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            &body.to_string(),
        ),
    )
    .await
}

fn first_field(reply: &Reply) -> Value {
    reply.json()["error"]["fields"][0].clone()
}

#[tokio::test]
async fn templating_is_off_by_default_and_leaves_braces_literal() {
    let harness = harness().await;
    let created = create(
        &harness.app,
        r#"{"response":{"body":{"encoding":"utf8","data":"{{ not a template"}}}"#,
    )
    .await;

    assert_eq!(created["response"]["templated"], false);
    assert_eq!(created["response"]["status_template"], Value::Null);
    assert_eq!(created["response"]["body"]["data"], "{{ not a template");
}

#[tokio::test]
async fn a_valid_templated_response_is_stored_and_returned() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let reply = patch(
        &harness.app,
        &id,
        json!({"response": {
            "templated": true,
            "status_template": "{{ request.query.code | default(\"200\") }}",
            "headers": [{"name": "X-Echo", "value": "{{ request.header.x-token }}"}],
            "body": {"encoding": "utf8", "data": "{{ json(\"$.order.id\") }}"}
        }}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["response"]["templated"], true);
    assert_eq!(
        body["response"]["status_template"],
        "{{ request.query.code | default(\"200\") }}"
    );
}

#[tokio::test]
async fn parse_errors_name_the_field_the_code_and_the_position() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let reply = patch(
        &harness.app,
        &id,
        json!({"response": {
            "templated": true,
            "status_template": "{{ request.method",
            "headers": [
                {"name": "X-Ok", "value": "fine"},
                {"name": "X-Bad", "value": "a {{ nonsense.root }}"}
            ],
            "body": {"encoding": "utf8", "data": "line one\n  {{ request.query.q | nope }}"}
        }}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "validation_failed");
    assert_eq!(
        field_codes(&error),
        [
            (
                "response.status_template".to_owned(),
                "template_invalid".to_owned()
            ),
            (
                "response.headers.1.value".to_owned(),
                "template_invalid".to_owned()
            ),
            ("response.body".to_owned(), "template_invalid".to_owned()),
        ]
    );
    let fields = error["error"]["fields"].as_array().expect("fields");
    assert_eq!(fields[1]["line"], 1);
    assert_eq!(fields[1]["column"], 6);
    assert_eq!(fields[2]["line"], 2);
    assert_eq!(fields[2]["column"], 24);
}

#[tokio::test]
async fn secrets_and_json_path_and_step_errors_use_their_documented_codes() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let reply = patch(
        &harness.app,
        &id,
        json!({"response": {
            "templated": true,
            "headers": [
                {"name": "X-A", "value": "{{ secret.TOKEN }}"},
                {"name": "X-B", "value": "{{ json(\"$..[\") }}"},
                {"name": "X-C", "value": "{{ step.fetch.status }}"}
            ]
        }}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        [
            (
                "response.headers.0.value".to_owned(),
                "secret_not_allowed".to_owned()
            ),
            (
                "response.headers.1.value".to_owned(),
                "json_path_invalid".to_owned()
            ),
            (
                "response.headers.2.value".to_owned(),
                "step_unknown".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn a_literal_forbidden_character_in_a_header_template_is_refused() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let reply = patch(
        &harness.app,
        &id,
        json!({"response": {
            "templated": true,
            "headers": [{"name": "X-A", "value": "a\r\nInjected: 1"}]
        }}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_field(&reply)["field"], "response.headers.0.value");
}

#[tokio::test]
async fn a_non_utf8_body_cannot_be_a_template() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let reply = patch(
        &harness.app,
        &id,
        json!({"response": {
            "templated": true,
            "body": {"encoding": "base64", "data": "/w=="}
        }}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_field(&reply)["field"], "response.body");
    assert_eq!(first_field(&reply)["code"], "invalid_format");
}

#[tokio::test]
async fn turning_templating_on_validates_the_stored_response_and_off_does_not() {
    let harness = harness().await;
    let id = create(
        &harness.app,
        r#"{"response":{"body":{"encoding":"utf8","data":"{{ broken"}}}"#,
    )
    .await["id"]
        .as_str()
        .expect("id")
        .to_owned();

    let on = patch(&harness.app, &id, json!({"response": {"templated": true}})).await;
    let off = patch(&harness.app, &id, json!({"response": {"templated": false}})).await;

    assert_eq!(on.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_field(&on)["field"], "response.body");
    assert_eq!(off.status, StatusCode::OK);
}

#[tokio::test]
async fn variables_can_be_put_listed_and_deleted_with_their_limits() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/vars");

    let stored = send(
        &harness.app,
        request(
            Method::PUT,
            &format!("{uri}/orders"),
            r#"{"value":{"n":3}}"#,
        ),
    )
    .await;
    let listed = send(&harness.app, bodiless(Method::GET, &uri)).await;
    let bad_name = send(
        &harness.app,
        request(Method::PUT, &format!("{uri}/Bad-Name"), r#"{"value":1}"#),
    )
    .await;
    let too_big = send(
        &harness.app,
        request(
            Method::PUT,
            &format!("{uri}/big"),
            &json!({"value": "a".repeat(5000)}).to_string(),
        ),
    )
    .await;
    let deleted = send(
        &harness.app,
        bodiless(Method::DELETE, &format!("{uri}/orders")),
    )
    .await;
    let missing = send(
        &harness.app,
        bodiless(Method::DELETE, &format!("{uri}/orders")),
    )
    .await;

    assert_eq!(stored.status, StatusCode::OK);
    assert_eq!(stored.json()["value"], json!({"n": 3}));
    assert_eq!(listed.json()["items"][0]["name"], "orders");
    assert_eq!(bad_name.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(too_big.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json()["error"]["code"], "var_not_found");
}

#[tokio::test]
async fn the_fifty_first_variable_is_refused_with_the_documented_limit() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/vars");
    for index in 0..50 {
        let reply = send(
            &harness.app,
            request(Method::PUT, &format!("{uri}/v{index}"), r#"{"value":1}"#),
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK);
    }

    let reply = send(
        &harness.app,
        request(Method::PUT, &format!("{uri}/extra"), r#"{"value":1}"#),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.json()["error"]["code"], "limit_reached");
    assert_eq!(
        reply.json()["error"]["limit"],
        json!({"name": "vars", "max": 50})
    );
}

#[tokio::test]
async fn an_array_at_the_cap_and_a_nul_string_never_reach_a_server_error() {
    let harness = harness().await;
    let id = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/vars");
    let array = format!("{{\"value\":[{}1]}}", "1,".repeat(2046));

    let at_cap = send(
        &harness.app,
        request(Method::PUT, &format!("{uri}/big"), &array),
    )
    .await;
    let nul = send(
        &harness.app,
        request(
            Method::PUT,
            &format!("{uri}/nul"),
            r#"{"value":"a\u0000b"}"#,
        ),
    )
    .await;
    let template_nul = patch(
        &harness.app,
        &id,
        json!({"response": {"status_template": "a\u{0}b"}}),
    )
    .await;

    assert_eq!(at_cap.status, StatusCode::OK);
    assert_eq!(nul.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(template_nul.status, StatusCode::UNPROCESSABLE_ENTITY);
}
