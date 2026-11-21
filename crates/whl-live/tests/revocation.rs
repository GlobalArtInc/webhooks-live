mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use support::{TestApp, capture_payload, insert_endpoint, notify};
use whl_core::endpoint_id::EndpointId;
use whl_live::{StreamAuthorizer, StreamRequest, StreamSettings, StreamVerdict, VerdictFuture};
use whl_testkit::TestDb;

const ALLOWED: u8 = 0;
const DENIED: u8 = 1;
const UNAVAILABLE: u8 = 2;
const TICK: Duration = Duration::from_millis(150);

#[derive(Debug, Default)]
struct Switch {
    verdict: AtomicU8,
    asked: AtomicU8,
}

impl Switch {
    fn set(&self, verdict: u8) {
        self.verdict.store(verdict, Ordering::SeqCst);
    }
}

impl StreamAuthorizer for Switch {
    fn still_allowed<'a>(
        &'a self,
        _endpoint: &'a EndpointId,
        _request: &'a StreamRequest,
    ) -> VerdictFuture<'a> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let verdict = match self.verdict.load(Ordering::SeqCst) {
            ALLOWED => StreamVerdict::Allowed,
            DENIED => StreamVerdict::Denied,
            _ => StreamVerdict::Unavailable,
        };
        Box::pin(std::future::ready(verdict))
    }
}

fn settings(recheck: Duration) -> StreamSettings {
    let mut settings = StreamSettings::new(support::RETENTION);
    settings.recheck_interval = recheck;
    settings
}

async fn started(
    db: &TestDb,
    recheck: Duration,
) -> (TestApp, Arc<Switch>, EndpointId, support::SseClient) {
    let switch = Arc::new(Switch::default());
    let app = TestApp::start_authorized(db, settings(recheck), switch.clone()).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    (app, switch, endpoint, client)
}

#[tokio::test]
async fn a_viewer_who_lost_access_gets_bye_within_one_tick_and_no_later_capture() {
    let db = TestDb::new().await;
    let (_app, switch, endpoint, mut client) = started(&db, TICK).await;
    notify(db.pool(), &capture_payload(&endpoint, 1)).await;
    client.expect_capture(1).await;

    switch.set(DENIED);
    client.expect_event("bye").await;
    notify(db.pool(), &capture_payload(&endpoint, 2)).await;
    client.expect_end().await;
}

#[tokio::test]
async fn a_viewer_who_keeps_access_keeps_receiving_across_ticks() {
    let db = TestDb::new().await;
    let (_app, switch, endpoint, mut client) = started(&db, TICK).await;
    for seq in 1..=3 {
        tokio::time::sleep(TICK * 2).await;
        notify(db.pool(), &capture_payload(&endpoint, seq)).await;
        client.expect_capture(seq).await;
    }
    assert!(switch.asked.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn a_failed_access_check_closes_the_stream() {
    let db = TestDb::new().await;
    let (_app, switch, _endpoint, mut client) = started(&db, TICK).await;
    switch.set(UNAVAILABLE);
    client.expect_event("bye").await;
    client.expect_end().await;
}

#[tokio::test]
async fn an_access_change_notification_rechecks_at_once_without_waiting_for_a_tick() {
    let db = TestDb::new().await;
    let (_app, switch, endpoint, mut client) = started(&db, Duration::from_secs(3600)).await;
    switch.set(DENIED);
    notify(
        db.pool(),
        &format!(r#"{{"t":"access_changed","e":"{endpoint}"}}"#),
    )
    .await;
    client.expect_event("bye").await;
    client.expect_end().await;
}

#[tokio::test]
async fn an_access_change_for_a_viewer_who_is_still_allowed_leaves_the_stream_open() {
    let db = TestDb::new().await;
    let (_app, _switch, endpoint, mut client) = started(&db, Duration::from_secs(3600)).await;
    notify(
        db.pool(),
        &format!(r#"{{"t":"access_changed","e":"{endpoint}"}}"#),
    )
    .await;
    notify(db.pool(), &capture_payload(&endpoint, 1)).await;
    client.expect_capture(1).await;
}

#[tokio::test]
async fn a_stream_without_an_authorizer_is_never_closed_by_the_recheck() {
    let db = TestDb::new().await;
    let app = TestApp::start_with(&db, support::HUB_TEST_CAPACITY, settings(TICK)).await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    tokio::time::sleep(TICK * 4).await;
    notify(db.pool(), &capture_payload(&endpoint, 1)).await;
    client.expect_capture(1).await;
}
