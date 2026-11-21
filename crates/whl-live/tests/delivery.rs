mod support;

use std::time::{Duration, Instant};

use support::{
    RETENTION, TestApp, WAIT, capture_payload, insert_captures, insert_endpoint,
    insert_forward_job, notify, padded_capture_payload, pipe, serve_http1,
    terminate_listener_backend,
};
use whl_live::{Hub, ListenerSettings, StreamSettings, spawn_listener_with};
use whl_store::Store;
use whl_store::stream::{endpoint_updated_notification, forward_result_notification};
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
    let settings = StreamSettings::new(RETENTION).with_instance("https://hooks.test/", 123);
    let app = TestApp::start_with(&db, 256, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 1).await;
    let (capture, job) = insert_forward_job(db.pool(), &endpoint, 1).await;
    sqlx::query("UPDATE endpoints SET title = 'orders', cors = true WHERE id = $1")
        .bind(endpoint.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint updates");
    let mut client = app.open(&endpoint).await;
    let mut second = app.open(&endpoint).await;
    client.expect_event("ready").await;
    second.expect_event("ready").await;

    let plain = [
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
    ];
    for (name, payload, data) in &plain {
        notify(db.pool(), payload).await;
        let frame = client.expect_event(name).await;
        assert_eq!(frame.id, None);
        assert_eq!(frame.data, *data);
        second.expect_event(name).await;
    }

    let updated = endpoint_updated_notification(&endpoint).expect("the payload encodes");
    assert!(updated.len() < 100);
    notify(db.pool(), &updated).await;
    let frame = client.expect_event("endpoint_updated").await;
    let shared = second.expect_event("endpoint_updated").await;
    assert_eq!(frame.data, shared.data);
    let view = frame.json();
    assert_eq!(view["id"], endpoint.unpack());
    assert_eq!(view["url"], format!("https://hooks.test/{endpoint}"));
    assert_eq!(view["title"], "orders");
    assert_eq!(view["cors"], true);
    assert_eq!(view["max_captures"], 123);
    assert_eq!(view["last_seq"], 1);
    assert_eq!(view["response"]["status"], 200);
    assert_eq!(view["forward_rules"], serde_json::json!([]));
    let keys = view.as_object().expect("the view is an object");
    assert!(!keys.contains_key("owner"));
    assert!(!keys.contains_key("my_role"));

    let result =
        forward_result_notification(&endpoint, &capture, &job).expect("the payload encodes");
    notify(db.pool(), &result).await;
    let frame = client.expect_event("forward_result").await;
    assert_eq!(frame.id, None);
    let data = frame.json();
    assert_eq!(data["capture_id"], capture.to_string());
    assert_eq!(data["job"]["id"], job.to_string());
    assert_eq!(data["job"]["state"], "failed");
    assert_eq!(data["job"]["attempts"][0]["status"], 502);
    assert_eq!(
        data["job"]["attempts"][0]["response_body"]["data"],
        "bad gateway"
    );
    assert_eq!(
        data["job"]["attempts"][0]["response_content_type"],
        "text/plain"
    );
    second.expect_event("forward_result").await;

    notify(
        db.pool(),
        &format!(r#"{{"t":"endpoint_deleted","e":"{endpoint}"}}"#),
    )
    .await;
    assert_eq!(client.expect_event("endpoint_deleted").await.data, "{}");
}

#[tokio::test]
async fn thin_events_whose_subject_is_gone_are_dropped() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let endpoint = insert_endpoint(db.pool()).await;
    insert_captures(db.pool(), &endpoint, 1, 1).await;
    let (capture, job) = insert_forward_job(db.pool(), &endpoint, 1).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    sqlx::query("DELETE FROM forward_jobs WHERE id = $1")
        .bind(job)
        .execute(db.pool())
        .await
        .expect("the job is deleted");
    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(endpoint.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint is marked deleted");

    notify(
        db.pool(),
        &forward_result_notification(&endpoint, &capture, &job).expect("the payload encodes"),
    )
    .await;
    notify(
        db.pool(),
        &endpoint_updated_notification(&endpoint).expect("the payload encodes"),
    )
    .await;
    notify(db.pool(), &capture_payload(&endpoint, 2)).await;

    client.expect_capture(2).await;
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
    const EVENTS: i64 = 80;
    const PADDING: usize = 6000;
    const HUB_CAPACITY: usize = 4;

    let db = TestDb::new().await;
    let mut settings = StreamSettings::new(RETENTION);
    settings.write_timeout = Duration::from_secs(120);
    let app = TestApp::start_with(&db, HUB_CAPACITY, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let (slow_io, server_io) = pipe();
    tokio::spawn(serve_http1(server_io, app.live.clone()));
    let mut slow = app.open_over(slow_io, &endpoint).await;
    let mut fast = app.open(&endpoint).await;
    slow.expect_event("ready").await;
    fast.expect_event("ready").await;

    for seq in 1..=EVENTS {
        notify(db.pool(), &padded_capture_payload(&endpoint, seq, PADDING)).await;
        let frame = fast.expect_capture(seq).await;
        assert_eq!(frame.id.as_deref(), Some(seq.to_string().as_str()));
    }
    notify(db.pool(), &capture_payload(&endpoint, EVENTS + 1)).await;
    fast.expect_capture(EVENTS + 1).await;

    let mut saw_resync = false;
    loop {
        let frame = slow.next_event().await.expect("the slow viewer stays open");
        if frame.is_event("resync") {
            assert_eq!(frame.json(), serde_json::json!({"reason": "lagged"}));
            saw_resync = true;
        }
        if frame.is_event("capture") && frame.id.as_deref() == Some("81") {
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
async fn a_flapping_listener_connection_backs_off_instead_of_reconnecting_at_once() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let listening = ListenerSettings {
        initial_backoff: Duration::from_millis(200),
        max_backoff: Duration::from_secs(5),
        stable_after: Duration::from_secs(3600),
        ..ListenerSettings::default()
    };
    let app = TestApp::start_on(store, 256, StreamSettings::new(RETENTION), listening).await;

    let started = Instant::now();
    for establishments in 2..=4 {
        assert_eq!(terminate_listener_backend(db.pool()).await, 1);
        app.listener
            .wait_for_establishments(establishments)
            .await
            .expect("the listener reconnects");
    }

    assert!(
        started.elapsed() >= Duration::from_millis(1000),
        "the reconnects were too quick: {:?}",
        started.elapsed()
    );
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
        r#"{"t":"heartbeat"}"#.to_owned(),
        r#"{"t":"capture","e":"short","seq":1,"s":{}}"#.to_owned(),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":1}}"#),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":"one","s":{{}}}}"#),
        format!(r#"{{"t":"capture","e":"{endpoint}","seq":1,"s":[]}}"#),
        format!(r#"{{"t":"forward_result","e":"{endpoint}","d":{{"capture_id":"x"}}}}"#),
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

#[tokio::test]
async fn the_liveness_signal_reports_a_listener_that_ended_or_panicked() {
    let db = TestDb::new().await;
    let app = TestApp::start(&db).await;
    let running = app.listener.liveness();
    assert!(running.is_alive());

    let store = Store::from_pool(db.pool().clone());
    let (detonate, detonated) = tokio::sync::oneshot::channel::<()>();
    let doomed = spawn_listener_with(store, Hub::new(4), ListenerSettings::default(), async {
        let _ = detonated.await;
        panic!("the listener task is made to fail");
    });
    let liveness = doomed.liveness();
    assert!(liveness.is_alive());
    doomed
        .wait_listening()
        .await
        .expect("the doomed listener starts listening");
    assert!(liveness.is_alive());
    detonate
        .send(())
        .expect("the doomed listener still waits for the signal");

    tokio::time::timeout(WAIT, liveness.stopped())
        .await
        .expect("the panic is reported");
    assert!(!liveness.is_alive());
    assert!(running.is_alive());
    doomed.join().await;
}
