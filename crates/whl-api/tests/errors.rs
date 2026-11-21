mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{bodiless, field_codes, harness, request, send};

#[tokio::test]
async fn invalid_json_is_a_400_error_body() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        request(Method::POST, "/api/v1/endpoints", r#"{"title":"#),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "invalid_json");
    assert!(error["error"]["message"].is_string());
    assert!(error["error"].get("fields").is_none());
}

#[tokio::test]
async fn unknown_field_is_a_422_with_its_path() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        request(Method::POST, "/api/v1/endpoints", r#"{"surprise":true}"#),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("surprise".to_owned(), "not_allowed".to_owned())]
    );
}

#[tokio::test]
async fn mistyped_field_is_a_422_with_its_path() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/endpoints",
            r#"{"response":{"status":"200"}}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("response.status".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn body_over_the_limit_is_a_413_error_body() {
    let harness = harness().await;
    let padding = "a".repeat(300 * 1024);
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/endpoints",
            &format!(r#"{{"title":"{padding}"}}"#),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(reply.json()["error"]["code"], "payload_too_large");
    assert!(
        reply.headers[header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("application/json")
    );
}

#[tokio::test]
async fn body_without_json_content_type_is_a_415() {
    let harness = harness().await;
    let plain = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/endpoints")
        .body(Body::from(r#"{"title":"x"}"#))
        .expect("the request is valid");
    let reply = send(&harness.app, plain).await;
    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(reply.json()["error"]["code"], "unsupported_media_type");
}

#[tokio::test]
async fn unknown_api_path_is_a_404_json_body() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::GET, "/api/v1/nothing")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "not_found");
    assert!(
        reply.headers[header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("application/json")
    );
}

#[tokio::test]
async fn wrong_method_is_a_405_json_body() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::PUT, "/api/v1/instance")).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.json()["error"]["code"], "method_not_allowed");
    assert!(
        reply.headers[header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("application/json")
    );
}

#[tokio::test]
async fn api_responses_carry_security_headers() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::GET, "/api/v1/instance")).await;
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(reply.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
}
