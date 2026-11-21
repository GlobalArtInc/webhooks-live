mod common;

use axum::http::{Method, StatusCode, header};
use common::matrix::cast;
use common::tokens::mint;
use common::{bodiless, harness, send, sso_harness};

fn cache_control(reply: &common::Reply) -> &str {
    reply.headers[header::CACHE_CONTROL]
        .to_str()
        .expect("a text header")
}

#[tokio::test]
async fn error_and_fallback_responses_of_the_real_router_are_never_cached() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let minted = mint(&harness, &cast.owner, &["read"], None).await;

    let unauthenticated = harness
        .send_as(None, Method::GET, "/api/v1/usage", "")
        .await;
    assert_eq!(unauthenticated.status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&unauthenticated), "no-store");

    let forbidden = harness
        .send_bearer(&minted.token, Method::GET, "/api/v1/tokens", "")
        .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(cache_control(&forbidden), "no-store");

    let missing = harness
        .send_as(None, Method::GET, "/api/v1/no-such-route", "")
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&missing), "no-store");

    let refused = harness
        .send_as(None, Method::DELETE, "/api/v1/instance", "")
        .await;
    assert_eq!(refused.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(cache_control(&refused), "no-store");

    let document = harness
        .send_as(None, Method::GET, "/api/v1/openapi.json", "")
        .await;
    assert_eq!(document.status, StatusCode::OK);
    assert_eq!(cache_control(&document), "no-cache");
}

#[tokio::test]
async fn successful_json_responses_of_the_open_router_are_never_cached() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::GET, "/api/v1/instance")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(cache_control(&reply), "no-store");
}
