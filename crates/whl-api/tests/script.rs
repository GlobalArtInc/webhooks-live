mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode, header};
use common::matrix::cast;
use common::{api_settings, harness, mixed_harness, request, send, sso_harness};
use serde_json::{Value, json};
use whl_api::ApiState;
use whl_store::Store;
use whl_testkit::TestDb;

const META: &str = "/api/v1/script/meta";
const VALIDATE: &str = "/api/v1/script/validate";

async fn validate(app: &axum::Router, source: &str) -> Value {
    let body = json!({ "source": source }).to_string();
    let reply = send(app, request(Method::POST, VALIDATE, &body)).await;
    assert_eq!(reply.status, StatusCode::OK, "{source:?}");
    reply.json()
}

#[tokio::test]
async fn the_catalogue_lists_functions_variables_keywords_and_the_engine_limits() {
    let harness = harness().await;
    let reply = send(&harness.app, common::bodiless(Method::GET, META)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let meta = reply.json();
    assert_eq!(meta["name"], "rhai");
    assert!(meta["keywords"].as_array().expect("keywords").len() > 10);
    let functions = meta["functions"].as_array().expect("functions");
    let sha = functions
        .iter()
        .find(|function| function["name"] == "sha256")
        .expect("sha256 is listed");
    assert_eq!(sha["signature"], "sha256(text: string) -> string");
    assert_eq!(
        sha["parameters"],
        json!([{"name": "text", "type": "string"}])
    );
    assert_eq!(sha["returns"], "string");
    let print = functions
        .iter()
        .find(|function| function["name"] == "print")
        .expect("print is listed");
    assert_eq!(print["returns"], Value::Null);
    let variables = meta["variables"].as_array().expect("variables");
    assert!(
        variables
            .iter()
            .any(|variable| variable["name"] == "request.method")
    );
    assert!(variables.iter().any(|variable| variable["name"] == "vars"));
    let limits = &meta["limits"];
    assert_eq!(limits["max_source_bytes"], 16384);
    assert_eq!(limits["memory_bytes"], 32 * 1024 * 1024);
    assert_eq!(
        limits["before_response"]["max_operations"],
        whl_script::BEFORE_RESPONSE_OPERATIONS
    );
    assert_eq!(
        limits["before_response"]["max_time_ms"],
        whl_script::BEFORE_RESPONSE_TIME.as_millis() as u64
    );

    assert_eq!(
        limits["after_response"]["max_operations"],
        whl_script::AFTER_RESPONSE_OPERATIONS
    );
    assert_eq!(
        limits["after_response"]["max_time_ms"],
        whl_script::AFTER_RESPONSE_TIME.as_millis() as u64
    );
    assert!(
        meta["reference_url"]
            .as_str()
            .is_some_and(|url| url.starts_with('/'))
    );
}

#[tokio::test]
async fn a_source_that_compiles_has_no_diagnostics() {
    let harness = harness().await;
    let answer = validate(
        &harness.app,
        "let a = request.method;\n#{ vars: #{ a: a } }",
    )
    .await;
    assert_eq!(answer, json!({ "diagnostics": [] }));
    let blank = validate(&harness.app, "").await;
    assert_eq!(blank, json!({ "diagnostics": [] }));
}

#[tokio::test]
async fn a_syntax_error_is_a_200_with_its_position_code_and_message() {
    let harness = harness().await;
    let answer = validate(&harness.app, "let a = 1;\nlet b = ;\n").await;
    let diagnostics = answer["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1);
    let found = &diagnostics[0];
    assert_eq!(found["severity"], "error");
    assert_eq!(found["code"], "syntax");
    assert_eq!(found["line"], 2);
    assert_eq!(found["column"], 9);
    assert!(found["length"].as_u64().is_some_and(|length| length >= 1));
    assert!(
        found["message"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    let ended = validate(&harness.app, "let a = \"abc").await;
    assert_eq!(ended["diagnostics"][0]["code"], "unexpected_end");
    let disabled = validate(&harness.app, "eval(\"1\")").await;
    assert_eq!(disabled["diagnostics"][0]["code"], "disabled_feature");
}

#[tokio::test]
async fn validation_only_compiles_so_an_endless_loop_answers_at_once() {
    let harness = harness().await;
    let started = Instant::now();
    let answer = validate(&harness.app, "loop { }").await;
    assert_eq!(answer, json!({ "diagnostics": [] }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn hostile_nesting_is_a_diagnostic_not_a_crash() {
    let harness = harness().await;
    let source = format!("{}1{}", "(".repeat(8000), ")".repeat(8000));
    let answer = validate(&harness.app, &source).await;
    assert_eq!(answer["diagnostics"][0]["code"], "expression_too_deep");
}

#[tokio::test]
async fn a_source_over_16_kib_is_refused_and_a_far_larger_body_is_cut_off() {
    let harness = harness().await;
    let over = json!({ "source": "a".repeat(16 * 1024 + 1) }).to_string();
    let reply = send(&harness.app, request(Method::POST, VALIDATE, &over)).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        common::field_codes(&reply.json()),
        [("source".to_owned(), "too_long".to_owned())]
    );
    let huge = json!({ "source": "a".repeat(200 * 1024) }).to_string();
    let reply = send(&harness.app, request(Method::POST, VALIDATE, &huge)).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_bad_body_is_a_400_or_a_422_and_other_media_are_refused() {
    let harness = harness().await;
    let empty = send(&harness.app, request(Method::POST, VALIDATE, "")).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    let missing = send(&harness.app, request(Method::POST, VALIDATE, "{}")).await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        common::field_codes(&missing.json()),
        [("source".to_owned(), "required".to_owned())]
    );
    let text = axum::http::Request::builder()
        .method(Method::POST)
        .uri(VALIDATE)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(axum::body::Body::from("let a = 1;"))
        .expect("the request is valid");
    let reply = send(&harness.app, text).await;
    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn a_busy_replica_answers_429_with_a_retry_after() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let app = whl_api::router(ApiState::new(store, api_settings()).with_script_check_slots(0));
    let body = json!({ "source": "1" }).to_string();
    let reply = send(&app, request(Method::POST, VALIDATE, &body)).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        reply
            .headers
            .get(header::RETRY_AFTER)
            .map(|value| value.as_bytes()),
        Some(&b"1"[..])
    );
    let meta = send(&app, common::bodiless(Method::GET, META)).await;
    assert_eq!(meta.status, StatusCode::OK);
}

#[tokio::test]
async fn in_sso_the_routes_need_a_session_and_in_mixed_an_anonymous_caller_may_use_them() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let body = json!({ "source": "let a = 1;" }).to_string();
    let meta = harness.send_as(None, Method::GET, META, "").await;
    assert_eq!(meta.status, StatusCode::UNAUTHORIZED);
    let check = harness.send_as(None, Method::POST, VALIDATE, &body).await;
    assert_eq!(check.status, StatusCode::UNAUTHORIZED);
    let meta = harness
        .send_as(Some(&cast.stranger), Method::GET, META, "")
        .await;
    assert_eq!(meta.status, StatusCode::OK);
    assert_eq!(
        meta.json()["limits"]["before_response"]["max_time_ms"],
        whl_script::BEFORE_RESPONSE_TIME.as_millis() as u64
    );
    let check = harness
        .send_as(Some(&cast.stranger), Method::POST, VALIDATE, &body)
        .await;
    assert_eq!(check.status, StatusCode::OK);

    let mixed = mixed_harness().await;
    let meta = mixed.send_as(None, Method::GET, META, "").await;
    assert_eq!(meta.status, StatusCode::OK);
    let check = mixed.send_as(None, Method::POST, VALIDATE, &body).await;
    assert_eq!(check.status, StatusCode::OK);
}
