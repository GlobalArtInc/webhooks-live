mod common;

use std::time::Duration;

use axum::body::Body;
use common::{Harness, request_from};
use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::{LimiterSettings, Limiters};
use whl_core::rate::RateSpec;

const OTHER_SENDER: &str = "198.51.100.77:4000";

fn spec(text: &str) -> RateSpec {
    text.parse().expect("valid rate spec")
}

fn limiters(capture_ip: &str, capture_endpoint: &str) -> Limiters {
    Limiters::new(&LimiterSettings {
        capture_ip: spec(capture_ip),
        capture_endpoint: spec(capture_endpoint),
        api_ip: spec("1000/s:1000"),
        create_ip: spec("1000/s:1000"),
        max_keys: 1000,
    })
    .expect("limiters build")
}

async fn hit(harness: &Harness, path: &str, peer: &str) -> common::Reply {
    harness
        .send(
            request_from("GET", path, peer)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
}

#[tokio::test]
async fn per_ip_limit_answers_429_with_retry_after_and_recovers() {
    let harness = Harness::new()
        .await
        .with_limiters(limiters("10/s:2", "1000/s:1000"));
    let id = harness.endpoint().await;
    let path = format!("/{id}");

    for _ in 0..2 {
        let stored = hit(&harness, &path, common::SENDER).await;
        assert_eq!(stored.status, 200);
        assert_eq!(stored.header("ratelimit-limit"), None);
        assert_eq!(stored.header("ratelimit-remaining"), None);
    }
    let limited = hit(&harness, &path, common::SENDER).await;
    assert_eq!(limited.status, 429);
    assert_eq!(limited.body, "Too many requests.");
    assert_eq!(
        limited.header("x-webhooks-live-error"),
        Some("rate_limited")
    );
    assert_eq!(limited.header("retry-after"), Some("1"));
    assert_eq!(limited.header("cache-control"), Some("no-store"));
    assert_eq!(harness.rows(&id).await.len(), 2);

    let other = hit(&harness, &path, OTHER_SENDER).await;
    assert_eq!(other.status, 200);

    tokio::time::sleep(Duration::from_millis(250)).await;
    let recovered = hit(&harness, &path, common::SENDER).await;
    assert_eq!(recovered.status, 200);
}

#[tokio::test]
async fn per_ip_limit_rejects_before_the_endpoint_lookup() {
    let harness = Harness::new()
        .await
        .with_limiters(limiters("1/m:1", "1000/s:1000"));
    let missing = EndpointId::generate().expect("entropy is available");
    let path = format!("/{missing}");

    let first = hit(&harness, &path, common::SENDER).await;
    assert_eq!(first.status, 410);
    let second = hit(&harness, &path, common::SENDER).await;
    assert_eq!(second.status, 429);
    assert_eq!(second.header("x-endpoint-id"), None);
    assert_eq!(second.header("retry-after"), Some("60"));
}

#[tokio::test]
async fn per_endpoint_limit_is_independent_of_other_endpoints() {
    let harness = Harness::new()
        .await
        .with_limiters(limiters("1000/s:1000", "10/s:2"));
    let noisy = harness.endpoint().await;
    let quiet = harness.endpoint().await;

    for _ in 0..2 {
        let stored = hit(&harness, &format!("/{noisy}"), common::SENDER).await;
        assert_eq!(stored.status, 200);
    }
    let limited = hit(&harness, &format!("/{noisy}"), OTHER_SENDER).await;
    assert_eq!(limited.status, 429);
    assert_eq!(
        limited.header("x-webhooks-live-error"),
        Some("rate_limited")
    );
    assert!(limited.header("retry-after").is_some());
    assert_eq!(harness.rows(&noisy).await.len(), 2);

    let unaffected = hit(&harness, &format!("/{quiet}"), common::SENDER).await;
    assert_eq!(unaffected.status, 200);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let recovered = hit(&harness, &format!("/{noisy}"), common::SENDER).await;
    assert_eq!(recovered.status, 200);
}

#[tokio::test]
async fn per_endpoint_limit_is_not_applied_to_endpoints_that_do_not_exist() {
    let harness = Harness::new()
        .await
        .with_limiters(limiters("1000/s:1000", "1/m:1"));
    let missing = EndpointId::generate().expect("entropy is available");
    for _ in 0..3 {
        let reply = hit(&harness, &format!("/{missing}"), common::SENDER).await;
        assert_eq!(reply.status, 410);
    }
}
