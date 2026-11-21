mod support;

use std::time::{Duration, Instant};

use support::{
    RETENTION, TestApp, capture_payload, insert_captures, insert_endpoint, new_endpoint_id, notify,
    stream_path,
};
use whl_live::StreamSettings;
use whl_testkit::TestDb;

#[tokio::test]
async fn ready_carries_the_capture_count_and_the_stream_headers() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 3).await;

    let mut client = app.open(&endpoint).await;

    assert_eq!(client.status, 200);
    assert_eq!(client.header("content-type"), Some("text/event-stream"));
    assert_eq!(client.header("cache-control"), Some("no-store"));
    assert_eq!(client.header("x-accel-buffering"), Some("no"));
    assert_eq!(client.header("x-content-type-options"), Some("nosniff"));
    let retry = client.next_frame().await.expect("the retry frame arrives");
    assert_eq!(retry.retry, Some(2000));
    let ready = client.expect_event("ready").await;
    assert_eq!(ready.id, None);
    assert_eq!(
        ready.json(),
        serde_json::json!({"endpoint_id": endpoint.unpack(), "last_seq": 3, "capture_count": 3})
    );
}

#[tokio::test]
async fn unknown_expired_and_malformed_endpoints_are_refused_before_a_stream_starts() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;

    let unknown = app.open(&new_endpoint_id()).await;
    assert_eq!(unknown.status, 410);
    assert_eq!(unknown.header("content-type"), Some("application/json"));
    let body: serde_json::Value =
        serde_json::from_str(&unknown.read_body().await).expect("the error body is JSON");
    assert_eq!(body["error"]["code"], "endpoint_gone");

    let expired = insert_endpoint(db.pool()).await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '2 hours' WHERE id = $1")
        .bind(expired.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint ages");
    assert_eq!(app.open(&expired).await.status, 410);

    let deleted = insert_endpoint(db.pool()).await;
    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(deleted.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint is marked deleted");
    assert_eq!(app.open(&deleted).await.status, 410);

    let malformed = app
        .open_with("/api/v1/endpoints/NOT-AN-ID/stream", &[])
        .await;
    assert_eq!(malformed.status, 404);
    let body: serde_json::Value =
        serde_json::from_str(&malformed.read_body().await).expect("the error body is JSON");
    assert_eq!(body["error"]["code"], "not_found");

    assert_eq!(app.hub.endpoint_count(), 0);
}

#[tokio::test]
async fn connecting_extends_the_endpoint_lifetime() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    sqlx::query(
        "UPDATE endpoints SET last_active_at = now() - interval '30 minutes' WHERE id = $1",
    )
    .bind(endpoint.unpack())
    .execute(db.pool())
    .await
    .expect("the endpoint ages");

    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;

    let extended: bool = sqlx::query_scalar(
        "SELECT last_active_at > now() - interval '1 minute' FROM endpoints WHERE id = $1",
    )
    .bind(endpoint.unpack())
    .fetch_one(db.pool())
    .await
    .expect("the endpoint is read");
    assert!(extended);
}

#[tokio::test]
async fn replays_missed_captures_in_order_and_skips_a_live_duplicate() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 5).await;

    let mut client = app
        .open_with(&stream_path(&endpoint), &[("Last-Event-ID", "2")])
        .await;
    client.expect_event("ready").await;
    for seq in 3..=5 {
        let frame = client.expect_capture(seq).await;
        let summary = frame.json();
        assert_eq!(summary["seq"], seq);
        assert_eq!(summary["endpoint_id"], endpoint.unpack());
        assert_eq!(summary["path"], format!("/p{seq}"));
        assert_eq!(summary["query_string"], "a=1");
        assert_eq!(summary["client_ip"], "10.0.0.1");
        assert_eq!(summary["method"], "POST");
        assert_eq!(summary["content_type"], "text/plain");
        assert_eq!(summary["forward_state"], serde_json::Value::Null);
    }

    notify(db.pool(), &capture_payload(&endpoint, 5)).await;
    notify(db.pool(), &capture_payload(&endpoint, 6)).await;
    client.expect_capture(6).await;
}

#[tokio::test]
async fn replays_exactly_the_limit_without_a_resync() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 100).await;

    let mut client = app
        .open_with(&stream_path(&endpoint), &[("Last-Event-ID", "0")])
        .await;
    client.expect_event("ready").await;
    for seq in 1..=100 {
        client.expect_capture(seq).await;
    }
    notify(db.pool(), &capture_payload(&endpoint, 101)).await;
    client.expect_capture(101).await;
}

#[tokio::test]
async fn more_than_the_limit_missing_sends_a_gap_resync_instead_of_captures() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 101).await;

    let mut client = app
        .open_with(&stream_path(&endpoint), &[("Last-Event-ID", "0")])
        .await;
    client.expect_event("ready").await;
    let resync = client.expect_event("resync").await;
    assert_eq!(resync.json(), serde_json::json!({"reason": "gap"}));
    notify(db.pool(), &capture_payload(&endpoint, 102)).await;
    client.expect_capture(102).await;
}

#[tokio::test]
async fn the_resume_point_takes_the_larger_of_header_and_query_and_ignores_a_malformed_one() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 5).await;
    let path = stream_path(&endpoint);

    let mut larger = app
        .open_with(
            &format!("{path}?last_event_id=4"),
            &[("Last-Event-ID", "1")],
        )
        .await;
    larger.expect_event("ready").await;
    larger.expect_capture(5).await;

    let mut malformed = app
        .open_with(
            &format!("{path}?last_event_id=abc"),
            &[("Last-Event-ID", "-3")],
        )
        .await;
    malformed.expect_event("ready").await;
    notify(db.pool(), &capture_payload(&endpoint, 6)).await;
    malformed.expect_capture(6).await;
}

#[tokio::test]
async fn sends_comment_pings_while_idle() {
    let db = TestDb::new().await;
    let mut settings = StreamSettings::new(RETENTION);
    settings.ping_interval = Duration::from_millis(50);
    let app = TestApp::start_with(&db, 256, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;

    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    for _ in 0..2 {
        let frame = client.next_frame().await.expect("a ping arrives");
        assert_eq!(frame.comment.as_deref(), Some("ping"));
        assert_eq!(frame.event, None);
    }
}

#[tokio::test]
async fn the_server_closes_the_stream_after_the_maximum_lifetime() {
    let db = TestDb::new().await;
    let mut settings = StreamSettings::new(RETENTION);
    settings.max_lifetime = Duration::from_millis(400);
    let app = TestApp::start_with(&db, 256, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;

    let started = Instant::now();
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    client.expect_end().await;

    assert!(started.elapsed() >= Duration::from_millis(350));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn closing_the_streams_sends_bye_to_open_and_new_viewers() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut open = app.open(&endpoint).await;
    open.expect_event("ready").await;

    app.live.close_streams();

    let bye = open.expect_event("bye").await;
    assert_eq!(bye.json(), serde_json::json!({}));
    open.expect_end().await;

    let mut late = app.open(&endpoint).await;
    late.expect_event("bye").await;
    late.expect_end().await;
}

#[tokio::test]
async fn a_disconnected_viewer_frees_its_hub_entry() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut first = app.open(&endpoint).await;
    let mut second = app.open(&endpoint).await;
    first.expect_event("ready").await;
    second.expect_event("ready").await;
    assert_eq!(app.hub.subscriber_count(&endpoint), 2);

    drop(first);
    wait_for(|| app.hub.subscriber_count(&endpoint) == 1).await;
    assert_eq!(app.hub.endpoint_count(), 1);

    drop(second);
    wait_for(|| app.hub.endpoint_count() == 0).await;
}

async fn wait_for(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "the condition never held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
