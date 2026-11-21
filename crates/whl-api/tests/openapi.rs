mod common;

use axum::http::{Method, StatusCode, header};
use common::matrix::cast;
use common::tokens::mint;
use common::{bodiless, harness, send, sso_harness};

const URI: &str = "/api/v1/openapi.json";

#[tokio::test]
async fn an_anonymous_caller_gets_the_document_with_the_version_as_etag() {
    let harness = sso_harness().await;
    let reply = harness.send_as(None, Method::GET, URI, "").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-cache");
    let document = reply.json();
    assert!(
        document["openapi"]
            .as_str()
            .is_some_and(|version| version.starts_with("3.1"))
    );
    let version = document["info"]["version"].as_str().expect("a version");
    assert_eq!(reply.headers[header::ETAG], format!("\"{version}\""));
    assert!(document["paths"]["/usage"]["get"].is_object());
}

#[tokio::test]
async fn a_matching_if_none_match_answers_304_without_a_body_and_another_tag_gets_the_document() {
    let harness = sso_harness().await;
    let first = harness.send_as(None, Method::GET, URI, "").await;
    let etag = first.headers[header::ETAG]
        .to_str()
        .expect("text")
        .to_owned();
    let cached = harness
        .send_with(
            None,
            None,
            &[("if-none-match", &etag)],
            Method::GET,
            URI,
            "",
        )
        .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.body.is_empty());
    assert_eq!(cached.headers[header::ETAG], etag.as_str());
    let weak = harness
        .send_with(
            None,
            None,
            &[("if-none-match", &format!("\"other\", W/{etag}"))],
            Method::GET,
            URI,
            "",
        )
        .await;
    assert_eq!(weak.status, StatusCode::NOT_MODIFIED);
    let stale = harness
        .send_with(
            None,
            None,
            &[("if-none-match", "\"other\"")],
            Method::GET,
            URI,
            "",
        )
        .await;
    assert_eq!(stale.status, StatusCode::OK);
    assert!(!stale.body.is_empty());
}

#[tokio::test]
async fn a_token_with_the_read_scope_fetches_the_document() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let minted = mint(&harness, &cast.owner, &["read"], None).await;
    let reply = harness
        .send_bearer(&minted.token, Method::GET, URI, "")
        .await;
    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn the_document_needs_no_login_in_open_mode_and_rejects_writes() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::GET, URI)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let write = send(&harness.app, bodiless(Method::POST, URI)).await;
    assert_eq!(write.status, StatusCode::METHOD_NOT_ALLOWED);
}
