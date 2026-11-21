mod common;

use axum::http::{Method, StatusCode};
use common::{bodiless, harness, send};

#[tokio::test]
async fn instance_returns_the_documented_shape() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::GET, "/api/v1/instance")).await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["auth_mode"], "open");
    assert!(body["user"].is_null());
    assert_eq!(body["public_url"], "https://whl.test");
    assert!(body["auth"]["provider_name"].is_null());
    assert_eq!(body["limits"]["max_captures"], 500);
    assert_eq!(body["limits"]["max_body_bytes"], 10_485_760);
    assert_eq!(body["limits"]["retention_seconds"], 604_800);
    assert_eq!(body["limits"]["max_delay_ms"], 10_000);
    assert_eq!(body["limits"]["max_forward_rules"], 5);
    assert_eq!(body["limits"]["max_response_body_bytes"], 65_536);
    assert_eq!(body["limits"]["max_response_headers"], 20);
    assert_eq!(body["features"]["ring_mode_allowed"], true);
    assert_eq!(body["features"]["forwarding"], true);
    assert_eq!(body["features"]["private_forward_targets_allowed"], false);
    assert!(body["version"].is_string());
}
