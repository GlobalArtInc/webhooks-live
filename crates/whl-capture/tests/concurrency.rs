mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use axum::body::Body;
use common::{Harness, request};
use sqlx::postgres::PgListener;

#[tokio::test]
async fn fifty_concurrent_captures_get_a_gapless_sequence() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;

    let mut tasks = Vec::new();
    for index in 0..50 {
        let router = harness.router.clone();
        let uri = format!("/{id}/n/{index}");
        tasks.push(tokio::spawn(async move {
            common::send(
                &router,
                request("POST", &uri)
                    .body(Body::from(format!("payload-{index}")))
                    .expect("request builds"),
            )
            .await
        }));
    }
    for task in tasks {
        assert_eq!(task.await.expect("task joins").status, 200);
    }

    let rows = harness.rows(&id).await;
    let seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
    assert_eq!(seqs, (1..=50).collect::<Vec<_>>());
    let distinct: BTreeSet<_> = rows.iter().map(|row| row.id.clone()).collect();
    assert_eq!(distinct.len(), 50);

    let stored: i64 = rows.iter().map(|row| i64::from(row.stored_bytes)).sum();
    let counters = harness.counters(&id).await;
    assert_eq!(counters.capture_count, 50);
    assert_eq!(counters.last_seq, 50);
    assert_eq!(counters.bytes_used, stored);
    assert_eq!(harness.state.budget().in_use(), 0);
}

#[tokio::test]
async fn a_locked_endpoint_does_not_block_captures_to_another() {
    let harness = Harness::new().await;
    let busy = harness.endpoint().await;
    let free = harness.endpoint().await;

    let mut holder = harness.db.pool().begin().await.expect("transaction begins");
    sqlx::query("SELECT 1 FROM endpoints WHERE id = $1 FOR UPDATE")
        .bind(busy.unpack())
        .execute(&mut *holder)
        .await
        .expect("row locks");

    let router = harness.router.clone();
    let blocked = tokio::spawn({
        let uri = format!("/{busy}");
        async move {
            common::send(
                &router,
                request("POST", &uri)
                    .body(Body::from("blocked"))
                    .expect("request builds"),
            )
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!blocked.is_finished(), "the capture waits for the row lock");

    let started = std::time::Instant::now();
    let reply = harness.post(&format!("/{free}"), "free").await;
    assert_eq!(reply.status, 200);
    assert!(started.elapsed() < Duration::from_secs(1));

    holder.rollback().await.expect("lock releases");
    let released = blocked.await.expect("task joins");
    assert_eq!(released.status, 200);
    assert_eq!(harness.rows(&busy).await.len(), 1);
    assert_eq!(harness.rows(&free).await.len(), 1);
}

#[tokio::test]
async fn notification_carries_the_documented_summary_on_commit() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let mut listener = PgListener::connect(harness.db.url())
        .await
        .expect("listener connects");
    listener.listen("whl_events").await.expect("listens");

    let reply = harness
        .send(
            request("POST", &format!("/{id}/404/orders?x=1"))
                .header("content-type", "application/json")
                .body(Body::from("{\"order\":42}"))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 404);

    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("a notification arrives")
        .expect("listener stays up");
    assert_eq!(notification.channel(), "whl_events");
    let event: serde_json::Value =
        serde_json::from_str(notification.payload()).expect("payload is JSON");
    let row = &harness.rows(&id).await[0];
    assert_eq!(event["t"], "capture");
    assert_eq!(event["e"], id.unpack());
    assert_eq!(event["seq"], 1);
    let summary = &event["s"];
    assert_eq!(summary["id"], row.id.as_str());
    assert_eq!(summary["seq"], 1);
    assert_eq!(summary["endpoint_id"], id.unpack());
    assert_eq!(summary["method"], "POST");
    assert_eq!(summary["path"], "/404/orders");
    assert_eq!(summary["query_string"], "x=1");
    assert_eq!(summary["client_ip"], "198.51.100.10");
    assert_eq!(summary["content_type"], "application/json");
    assert_eq!(summary["body_size"], 12);
    assert_eq!(summary["status_override"], 404);
    assert_eq!(summary["response_status"], 404);
    assert!(summary["forward_state"].is_null());
    let received = summary["received_at"].as_str().expect("timestamp");
    assert_eq!(received.len(), 24);
    assert!(received.ends_with('Z'));
}

#[tokio::test]
async fn notification_stays_under_the_limit_for_a_path_near_the_uri_maximum() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let mut listener = PgListener::connect(harness.db.url())
        .await
        .expect("listener connects");
    listener.listen("whl_events").await.expect("listens");

    let path = format!("/{id}/{}", "p".repeat(60_000));
    let reply = harness.get(&path).await;
    assert_eq!(reply.status, 200);

    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("a notification arrives")
        .expect("listener stays up");
    assert!(notification.payload().len() < 8000);
    let event: serde_json::Value =
        serde_json::from_str(notification.payload()).expect("payload is JSON");
    assert_eq!(event["s"]["path"].as_str().expect("path").len(), 1024);
    let row = &harness.rows(&id).await[0];
    assert_eq!(row.path.len(), 60_001);
}

#[tokio::test]
async fn a_refused_capture_sends_no_notification() {
    let harness = Harness::new().await;
    let unknown = whl_core::endpoint_id::EndpointId::generate().expect("entropy");
    let mut listener = PgListener::connect(harness.db.url())
        .await
        .expect("listener connects");
    listener.listen("whl_events").await.expect("listens");

    assert_eq!(harness.get(&format!("/{unknown}")).await.status, 410);
    let waited = tokio::time::timeout(Duration::from_millis(300), listener.recv()).await;
    assert!(waited.is_err(), "no notification was expected");
}
