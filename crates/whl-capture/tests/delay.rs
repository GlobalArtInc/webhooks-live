mod common;
mod served;

use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use common::{Harness, request};
use served::{Configured, Wire, endpoint_configured, harness_with_slots, serve, wait_until};
use tower::ServiceExt;
use whl_core::endpoint_id::EndpointId;

const DELAY_MS: i32 = 600;
const DELAY: Duration = Duration::from_millis(DELAY_MS as u64);
const TOLERANCE: Duration = Duration::from_millis(50);

async fn delayed(harness: &Harness, delay_ms: i32) -> EndpointId {
    endpoint_configured(
        harness,
        Configured {
            delay_ms,
            ..Configured::default()
        },
    )
    .await
}

async fn timed_get(harness: &Harness, id: &EndpointId) -> (u16, Duration) {
    let started = Instant::now();
    let reply = harness.get(&format!("/{id}")).await;
    (reply.status, started.elapsed())
}

async fn status_of(router: Router, path: String) -> u16 {
    let request = request("GET", &path)
        .body(Body::empty())
        .expect("request builds");
    router
        .oneshot(request)
        .await
        .expect("the router is infallible")
        .status()
        .as_u16()
}

#[tokio::test]
async fn the_capture_is_stored_before_the_delay_ends() {
    let harness = harness_with_slots(10).await;
    let id = delayed(&harness, DELAY_MS).await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    let started = Instant::now();
    wire.write(format!("GET /{id} HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
        .await;
    assert!(wait_until(|| harness.state.delays().in_use() == 1).await);
    assert!(started.elapsed() < DELAY);
    assert_eq!(harness.rows(&id).await.len(), 1);

    let head = wire.head().await;
    assert_eq!(head.status, 200);
    assert!(started.elapsed() >= DELAY - TOLERANCE);
    assert!(wait_until(|| harness.state.delays().in_use() == 0).await);
}

#[tokio::test]
async fn concurrent_delayed_responses_overlap_instead_of_adding_up() {
    let harness = harness_with_slots(100).await;
    let id = delayed(&harness, DELAY_MS).await;

    let started = Instant::now();
    let calls = (0..40).map(|_| timed_get(&harness, &id));
    let results = futures_util::future::join_all(calls).await;
    let total = started.elapsed();

    assert!(results.iter().all(|(status, _)| *status == 200));
    assert!(results.iter().all(|(_, took)| *took >= DELAY - TOLERANCE));
    assert!(total < DELAY * 3, "{total:?}");
    assert_eq!(harness.rows(&id).await.len(), 40);
    assert_eq!(harness.state.delays().in_use(), 0);
}

#[tokio::test]
async fn over_the_cap_the_response_is_sent_without_delay() {
    let harness = harness_with_slots(2).await;
    let id = delayed(&harness, 1500).await;

    let held: Vec<_> = (0..2)
        .map(|_| tokio::spawn(status_of(harness.router.clone(), format!("/{id}"))))
        .collect();
    assert!(wait_until(|| harness.state.delays().in_use() == 2).await);

    let (status, took) = timed_get(&harness, &id).await;
    assert_eq!(status, 200);
    assert!(took < Duration::from_millis(500), "{took:?}");
    assert_eq!(harness.state.delays().in_use(), 2);
    assert_eq!(harness.rows(&id).await.len(), 3);

    for task in held {
        assert_eq!(task.await.expect("the held request completes"), 200);
    }
    assert_eq!(harness.state.delays().in_use(), 0);
}

#[tokio::test]
async fn a_freed_slot_delays_the_next_response_again() {
    let harness = harness_with_slots(1).await;
    let id = delayed(&harness, DELAY_MS).await;

    for _ in 0..2 {
        let (status, took) = timed_get(&harness, &id).await;
        assert_eq!(status, 200);
        assert!(took >= DELAY - TOLERANCE, "{took:?}");
        assert_eq!(harness.state.delays().in_use(), 0);
    }
}

#[tokio::test]
async fn a_zero_cap_never_delays() {
    let harness = harness_with_slots(0).await;
    let id = delayed(&harness, 3000).await;

    let (status, took) = timed_get(&harness, &id).await;
    assert_eq!(status, 200);
    assert!(took < Duration::from_millis(500), "{took:?}");
}

#[tokio::test]
async fn a_client_disconnect_releases_the_slot() {
    let harness = harness_with_slots(1).await;
    let id = delayed(&harness, 8000).await;
    let addr = serve(&harness).await;

    let mut wire = Wire::connect(addr).await;
    wire.write(format!("GET /{id} HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
        .await;
    assert!(wait_until(|| harness.state.delays().in_use() == 1).await);
    drop(wire);

    assert!(
        wait_until(|| harness.state.delays().in_use() == 0).await,
        "the slot leaked after the client left"
    );
    assert_eq!(harness.rows(&id).await.len(), 1);
}
