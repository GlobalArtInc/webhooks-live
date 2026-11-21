mod support;

use std::time::Duration;

use support::{
    ChannelSpec, SLACK_URL, limits, limits_with_global, loopback_guard, rig, rig_replying,
    wait_until,
};
use whl_notify::{Transport, VendorHosts};
use whl_store::notify::ChannelKind;

#[tokio::test]
async fn a_capture_creates_deliveries_only_for_enabled_capture_channels() {
    let rig = rig().await;
    rig.capture("POST", "/a", b"").await;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_deliveries")
        .fetch_one(rig.db.pool())
        .await
        .expect("the count is readable");
    assert_eq!(total, 0);

    let enabled = rig
        .channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    let disabled = rig
        .channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    let rule_only = rig
        .channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    sqlx::query("UPDATE notification_channels SET enabled = false WHERE id = $1")
        .bind(disabled)
        .execute(rig.db.pool())
        .await
        .expect("the channel is disabled");
    sqlx::query("UPDATE notification_channels SET trigger = 'rule' WHERE id = $1")
        .bind(rule_only)
        .execute(rig.db.pool())
        .await
        .expect("the trigger is changed");
    rig.capture("POST", "/b", b"").await;

    let channels: Vec<String> =
        sqlx::query_scalar("SELECT channel_id::text FROM notification_deliveries")
            .fetch_all(rig.db.pool())
            .await
            .expect("the rows are readable");
    assert_eq!(channels, vec![enabled.hyphenated().to_string()]);
}

#[tokio::test]
async fn the_endpoint_cap_suppresses_and_one_summary_follows() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    for path in ["/1", "/2", "/3", "/4"] {
        rig.capture("POST", path, b"").await;
    }
    let running = rig.start(limits(2, 300, 60));
    wait_until("two sent and two suppressed", || async {
        rig.count_in_state("succeeded").await == 2 && rig.count_in_state("suppressed").await == 2
    })
    .await;
    assert_eq!(rig.server.seen().len(), 2);

    sqlx::query("UPDATE notification_deliveries SET finished_at = now() - interval '2 hours' WHERE state = 'suppressed'")
        .execute(rig.db.pool())
        .await
        .expect("the window is closed");
    assert_eq!(rig.store.enqueue_summaries(48).await.expect("queued"), 1);
    assert_eq!(rig.store.enqueue_summaries(48).await.expect("queued"), 0);
    wait_until("the summary", || async { rig.server.seen().len() == 3 }).await;
    running.stop().await;

    let text = rig.server.seen()[2].json()["text"]
        .as_str()
        .expect("a text")
        .to_owned();
    assert!(text.starts_with("2 notifications"), "{text}");
}

#[tokio::test]
async fn the_network_cap_applies_to_anonymous_endpoints() {
    let rig = rig().await;
    let endpoint = rig.anonymous_endpoint("198.51.100.0/24").await;
    rig.channel_on(&endpoint, ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    rig.capture_on(&endpoint, "POST", "/1", b"").await;
    rig.capture_on(&endpoint, "POST", "/2", b"").await;
    let running = rig.start(limits(100, 300, 1));
    wait_until("one sent and one suppressed", || async {
        rig.count_in_state("succeeded").await == 1 && rig.count_in_state("suppressed").await == 1
    })
    .await;
    running.stop().await;
}

#[tokio::test]
async fn a_private_webhook_target_is_refused_without_a_request() {
    let rig = rig().await;
    let url = format!("http://{}/hook", rig.server.addr);
    rig.channel(ChannelSpec::url(ChannelKind::Webhook, &url))
        .await;
    rig.capture("POST", "/a", b"").await;
    let strict = Transport::new(Default::default(), None, VendorHosts::production())
        .expect("the transport is built");
    let running = rig.start_with(strict, limits(30, 300, 60));
    wait_until("the failure", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(rig.server.seen().is_empty());
    let error: String = sqlx::query_scalar("SELECT error FROM notification_deliveries")
        .fetch_one(rig.db.pool())
        .await
        .expect("the error is stored");
    assert!(error.starts_with("refused_target"), "{error}");
}

#[tokio::test]
async fn a_stored_slack_url_off_the_pattern_is_not_sent() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(
        ChannelKind::Slack,
        "https://hooks.slack.com/services/T0/B0?x=1",
    ))
    .await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the failure", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;
    assert!(rig.server.seen().is_empty());
}

#[tokio::test]
async fn a_client_error_answer_is_final_and_recorded() {
    let rig = rig_replying(400).await;
    rig.channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_with(
        {
            let mut hosts = VendorHosts::production();
            hosts = hosts.with_override("hooks.slack.com", rig.server.origin());
            Transport::new(loopback_guard(), None, hosts).expect("the transport is built")
        },
        limits(30, 300, 60),
    );
    wait_until("the failure", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    running.stop().await;

    assert_eq!(rig.server.seen().len(), 1);
    let (status, attempts): (i16, i16) =
        sqlx::query_as("SELECT status, attempts FROM notification_deliveries")
            .fetch_one(rig.db.pool())
            .await
            .expect("the row is readable");
    assert_eq!((status, attempts), (400, 1));
}

#[tokio::test]
async fn the_global_cap_bounds_endpoints_with_neither_owner_nor_network() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    rig.capture("POST", "/1", b"").await;
    rig.capture("POST", "/2", b"").await;
    let running = rig.start(limits_with_global(100, 300, 60, 1));
    wait_until("one sent and one suppressed", || async {
        rig.count_in_state("succeeded").await == 1 && rig.count_in_state("suppressed").await == 1
    })
    .await;
    running.stop().await;
}

#[tokio::test]
async fn a_capture_relayed_by_a_notification_creates_no_delivery() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    rig.capture_with_header("POST", "/relayed", "X-Webhooks-Live-Hop", "1")
        .await;
    rig.capture_with_header("POST", "/fresh", "X-Webhooks-Live-Hop", "0")
        .await;
    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT c.path FROM notification_deliveries d JOIN captures c ON c.id = d.capture_id",
    )
    .fetch_all(rig.db.pool())
    .await
    .expect("the rows are readable");
    assert_eq!(paths, vec!["/fresh".to_owned()]);
}

#[tokio::test]
async fn a_webhook_pointing_at_the_instance_is_refused_without_a_request() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(
        ChannelKind::Webhook,
        "https://hooks.example/abcdefghjkmnpqrstv012345",
    ))
    .await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the failure", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(rig.server.seen().is_empty());
    let error: String = sqlx::query_scalar("SELECT error FROM notification_deliveries")
        .fetch_one(rig.db.pool())
        .await
        .expect("the error is stored");
    assert!(error.contains("this instance"), "{error}");
}
