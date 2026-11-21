mod support;

use std::time::Duration;

use support::{
    RETENTION, TestApp, insert_endpoint, padded_event, pipe, serve_http1, serve_http1_until,
    wait_until,
};
use tokio::sync::oneshot;
use whl_live::{StreamSettings, WriteTimeout};
use whl_testkit::TestDb;

const FLOOD: i64 = 200;
const PADDING: usize = 6000;
const LIMIT: Duration = Duration::from_millis(300);

#[tokio::test]
async fn a_viewer_that_stops_reading_is_evicted_from_the_hub_within_the_write_timeout() {
    let db = TestDb::new().await;
    let mut settings = StreamSettings::new(RETENTION);
    settings.write_timeout = LIMIT;
    let app = TestApp::start_with(&db, 256, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let (client_io, server_io) = pipe();
    tokio::spawn(serve_http1(server_io, app.live.clone()));
    let mut silent = app.open_over(client_io, &endpoint).await;
    silent.expect_event("ready").await;
    assert_eq!(app.hub.subscriber_count(&endpoint), 1);

    for seq in 1..=FLOOD {
        app.hub
            .publish(&endpoint, padded_event(&endpoint, seq, PADDING));
    }
    wait_until("the silent viewer to leave the hub", || {
        app.hub.subscriber_count(&endpoint) == 0
    })
    .await;

    assert_eq!(app.hub.endpoint_count(), 0);
}

#[tokio::test]
async fn the_write_timeout_closes_a_connection_that_stops_reading_so_shutdown_is_not_delayed() {
    let db = TestDb::new().await;
    let mut settings = StreamSettings::new(RETENTION);
    settings.write_timeout = Duration::from_secs(120);
    let app = TestApp::start_with(&db, 256, settings).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let (client_io, server_io) = pipe();
    let (stop, stopped) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_http1_until(
        WriteTimeout::new(server_io, LIMIT),
        app.live.clone(),
        async move {
            let _ = stopped.await;
        },
    ));
    let mut silent = app.open_over(client_io, &endpoint).await;
    silent.expect_event("ready").await;

    for seq in 1..=FLOOD {
        app.hub
            .publish(&endpoint, padded_event(&endpoint, seq, PADDING));
    }
    app.live.close_streams();
    let _ = stop.send(());

    let finished = tokio::time::timeout(Duration::from_secs(30), server).await;

    assert!(finished.is_ok(), "the stalled connection delayed shutdown");
    assert_eq!(app.hub.subscriber_count(&endpoint), 0);
}
