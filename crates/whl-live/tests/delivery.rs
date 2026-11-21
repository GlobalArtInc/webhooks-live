mod support;

use std::time::Duration;

use support::{
    RETENTION, TestApp, capture_payload, insert_endpoint, notify, padded_capture_payload,
    terminate_listener_backend,
};
use whl_live::StreamSettings;
use whl_testkit::TestDb;

#[tokio::test]
async fn delivers_a_notified_capture_to_a_viewer() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let other = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;

    notify(db.pool(), &capture_payload(&other, 1)).await;
    notify(db.pool(), &capture_payload(&endpoint, 7)).await;

    let frame = client.expect_capture(7).await;
    assert_eq!(frame.json()["path"], "/live7");
}

#[tokio::test]
async fn delivers_every_event_type_with_its_data() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;

    let payloads = [
        (
            "capture_deleted",
            format!(
                r#"{{"t":"capture_deleted","e":"{endpoint}","d":{{"capture_id":"c1","seq":4}}}}"#
            ),
            r#"{"capture_id":"c1","seq":4}"#,
        ),
        (
            "captures_cleared",
            format!(r#"{{"t":"captures_cleared","e":"{endpoint}"}}"#),
            "{}",
        ),
        (
            "endpoint_updated",
            format!(
                r#"{{"t":"endpoint_updated","e":"{endpoint}","d":{{"id":"{endpoint}","cors":true}}}}"#
            ),
            &format!(r#"{{"cors":true,"id":"{endpoint}"}}"#),
        ),
        (
            "forward_result",
            format!(
                r#"{{"t":"forward_result","e":"{endpoint}","d":{{"capture_id":"c1","job":{{}}}}}}"#
            ),
            r#"{"capture_id":"c1","job":{}}"#,
        ),
        (
            "endpoint_deleted",
            format!(r#"{{"t":"endpoint_deleted","e":"{endpoint}"}}"#),
            "{}",
        ),
    ];
    for (name, payload, data) in &payloads {
        notify(db.pool(), payload).await;
        let frame = client.expect_event(name).await;
        assert_eq!(frame.id, None);
        assert_eq!(frame.data, *data);
    }
}

#[tokio::test]
async fn two_app_instances_on_one_database_both_deliver_a_notified_event() {
    let db = TestDb::new().await;
    let first = TestApp::start(&db).await;
    let second = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut on_first = first.open(&endpoint).await;
    let mut on_second = second.open(&endpoint).await;
    on_first.expect_event("ready").await;
    on_second.expect_event("ready").await;

    notify(db.pool(), &capture_payload(&endpoint, 1)).await;

    on_first.expect_capture(1).await;
    on_second.expect_capture(1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lagging_viewer_gets_a_resync_and_does_not_slow_anyone_else() {
    const EVENTS: i64 = 1500;
    const PADDING: usize = 6000;

    let db = TestDb::new().await;
    let app = TestApp::start_with(&db, 16, StreamSettings::new(RETENTION)).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut slow = app.open(&endpoint).await;
    let mut fast = app.open(&endpoint).await;
    slow.expect_event("ready").await;
    fast.expect_event("ready").await;

    let reader = tokio::spawn(async move {
        for seq in 1..=EVENTS {
            let frame = fast.next_event().await.expect("the fast viewer stays open");
            assert!(frame.is_event("capture"), "{frame:?}");
            assert_eq!(frame.id.as_deref(), Some(seq.to_string().as_str()));
        }
        fast
    });
    for seq in 1..=EVENTS {
        notify(db.pool(), &padded_capture_payload(&endpoint, seq, PADDING)).await;
    }
    let mut fast = tokio::time::timeout(Duration::from_secs(30), reader)
        .await
        .expect("the fast viewer receives every event in time")
        .expect("the reader task finishes");

    notify(db.pool(), &capture_payload(&endpoint, EVENTS + 1)).await;
    fast.expect_capture(EVENTS + 1).await;

    let mut saw_resync = false;
    loop {
        let frame = slow.next_event().await.expect("the slow viewer stays open");
        if frame.is_event("resync") {
            assert_eq!(frame.json(), serde_json::json!({"reason": "lagged"}));
            saw_resync = true;
        }
        if frame.is_event("capture") && frame.id.as_deref() == Some("1501") {
            break;
        }
    }
    assert!(saw_resync);
}

#[tokio::test]
async fn the_listener_resyncs_viewers_and_keeps_delivering_after_its_connection_dies() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    assert_eq!(app.listener.establishments(), 1);

    assert_eq!(terminate_listener_backend(db.pool()).await, 1);

    let resync = client.expect_event("resync").await;
    assert_eq!(
        resync.json(),
        serde_json::json!({"reason": "listener_reconnect"})
    );
    app.listener
        .wait_for_establishments(2)
        .await
        .expect("the listener establishes a second connection");
    notify(db.pool(), &capture_payload(&endpoint, 1)).await;
    client.expect_capture(1).await;
}

#[tokio::test]
async fn malformed_and_unknown_notifications_are_dropped_and_the_listener_survives() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;

    let bad = [
        "not json".to_owned(),
        "{}".to_owned(),
        r#"{"t":"capture","e":"short","seq":1,"s":{}}"#.to_owned(),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":1}}"#),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":"one","s":{{}}}}"#),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":1,"s":[]}}"#),
        format!(r#"{{"t":"from_the_future","e":"{endpoint}","d":{{}}}}"#),
        format!(r#"{{"t":"bye","e":"{endpoint}"}}"#),
    ];
    for payload in &bad {
        notify(db.pool(), payload).await;
    }
    notify(db.pool(), &capture_payload(&endpoint, 9)).await;

    client.expect_capture(9).await;
    assert_eq!(app.listener.establishments(), 1);
    assert!(!app.listener.is_finished());
}
