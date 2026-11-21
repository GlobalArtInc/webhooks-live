mod support;

use support::{rig, wait_until};
use whl_notify::{EmailLimits, SinkMode, SmtpSink, verify_disable_token};

fn roomy() -> EmailLimits {
    EmailLimits {
        per_user_per_day: 50,
        per_address_per_day: 100,
    }
}

#[tokio::test]
async fn a_capture_mails_a_verified_address_with_one_click_unsubscribe_headers() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let user = rig.user("ann").await;
    let endpoint = rig.owned_endpoint(user).await;
    let address = rig.address(user, "ann@mail.example.test", true).await;
    let channel = rig.email_channel(&endpoint, user, address).await;
    let capture = rig.capture_on(&endpoint, "POST", "/hook", b"{}").await;

    let running = rig.start_mail(sink.url(), roomy());
    wait_until("the mail", || async { sink.mails().len() == 1 }).await;
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let mail = &sink.mails()[0];
    assert_eq!(mail.sender, "noreply@hooks.example.test");
    assert_eq!(mail.recipients, vec!["ann@mail.example.test".to_owned()]);
    assert_eq!(mail.header("to").as_deref(), Some("ann@mail.example.test"));
    assert_eq!(
        mail.header("from").as_deref(),
        Some("\"Hooks\" <noreply@hooks.example.test>")
    );
    assert_eq!(
        mail.header("list-unsubscribe-post").as_deref(),
        Some("List-Unsubscribe=One-Click")
    );
    let unsubscribe = mail
        .header("list-unsubscribe")
        .expect("an unsubscribe link");
    let prefix = "<https://hooks.example/api/v1/notifications/disable?token=";
    assert!(unsubscribe.starts_with(prefix), "{unsubscribe}");
    let token = unsubscribe
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix('>'))
        .expect("a bracketed link");
    assert_eq!(verify_disable_token(&rig.keyring, token), Ok(channel));
    assert!(
        mail.header("subject")
            .expect("a subject")
            .starts_with("[webhooks.live]")
    );
    let text = mail.text();
    assert!(
        text.contains(&format!(
            "https://hooks.example/e/{endpoint}/c/{}",
            capture.unpack()
        )),
        "{text}"
    );
    assert!(text.contains("/notifications/disable?token="));
}

#[tokio::test]
async fn an_unverified_address_is_never_mailed() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let user = rig.user("bob").await;
    let endpoint = rig.owned_endpoint(user).await;
    let address = rig.address(user, "bob@mail.example.test", false).await;
    rig.email_channel(&endpoint, user, address).await;
    rig.capture_on(&endpoint, "POST", "/hook", b"{}").await;

    let running = rig.start_mail(sink.url(), roomy());
    wait_until("the refusal", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(sink.mails().is_empty());
    assert!(sink.commands().is_empty());
    let error = rig.last_error().await.expect("an error");
    assert!(error.starts_with("recipient_unverified"), "{error}");
}

#[tokio::test]
async fn an_address_that_belongs_to_another_user_is_never_mailed() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let owner = rig.user("cyd").await;
    let other = rig.user("dee").await;
    let endpoint = rig.owned_endpoint(owner).await;
    let foreign = rig.address(other, "dee@mail.example.test", true).await;
    rig.email_channel(&endpoint, owner, foreign).await;
    rig.capture_on(&endpoint, "POST", "/hook", b"{}").await;

    let running = rig.start_mail(sink.url(), roomy());
    wait_until("the refusal", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(sink.mails().is_empty());
    let error = rig.last_error().await.expect("an error");
    assert!(error.starts_with("recipient_unverified"), "{error}");
}

#[tokio::test]
async fn an_endpoint_without_an_owner_never_sends_email() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let user = rig.user("eve").await;
    let address = rig.address(user, "eve@mail.example.test", true).await;
    rig.email_channel(&rig.endpoint, user, address).await;
    rig.capture("POST", "/hook", b"{}").await;

    let running = rig.start_mail(sink.url(), roomy());
    wait_until("the refusal", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(sink.mails().is_empty());
    let error = rig.last_error().await.expect("an error");
    assert!(error.starts_with("email_requires_owner"), "{error}");
}

#[tokio::test]
async fn the_daily_cap_per_user_suppresses_the_excess_and_no_summary_mail_follows() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let user = rig.user("fay").await;
    let endpoint = rig.owned_endpoint(user).await;
    let address = rig.address(user, "fay@mail.example.test", true).await;
    rig.email_channel(&endpoint, user, address).await;
    for path in ["/1", "/2", "/3"] {
        rig.capture_on(&endpoint, "POST", path, b"").await;
    }

    let running = rig.start_mail(
        sink.url(),
        EmailLimits {
            per_user_per_day: 2,
            per_address_per_day: 100,
        },
    );
    wait_until("two sent and one suppressed", || async {
        rig.count_in_state("succeeded").await == 2 && rig.count_in_state("suppressed").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(sink.mails().len(), 2);
    sqlx::query("UPDATE notification_deliveries SET finished_at = now() - interval '2 hours' WHERE state = 'suppressed'")
        .execute(rig.db.pool())
        .await
        .expect("the window is closed");
    assert_eq!(rig.store.enqueue_summaries(48).await.expect("queued"), 0);
}

#[tokio::test]
async fn the_daily_cap_per_address_holds_across_users() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let first = rig.user("gus").await;
    let second = rig.user("hal").await;
    for user in [first, second] {
        let endpoint = rig.owned_endpoint(user).await;
        let address = rig.address(user, "shared@mail.example.test", true).await;
        rig.email_channel(&endpoint, user, address).await;
        rig.capture_on(&endpoint, "POST", "/hook", b"").await;
    }

    let running = rig.start_mail(
        sink.url(),
        EmailLimits {
            per_user_per_day: 50,
            per_address_per_day: 1,
        },
    );
    wait_until("one sent and one suppressed", || async {
        rig.count_in_state("succeeded").await == 1 && rig.count_in_state("suppressed").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(sink.mails().len(), 1);
}

#[tokio::test]
async fn a_refused_recipient_fails_at_once_and_a_deferred_one_is_retried() {
    let rig = rig().await;
    let refusing = SmtpSink::start(SinkMode::RefuseRecipients).await;
    let user = rig.user("ivy").await;
    let endpoint = rig.owned_endpoint(user).await;
    let address = rig.address(user, "ivy@mail.example.test", true).await;
    rig.email_channel(&endpoint, user, address).await;
    rig.capture_on(&endpoint, "POST", "/hook", b"").await;

    let running = rig.start_mail(refusing.url(), roomy());
    wait_until("the refusal", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;
    let error = rig.last_error().await.expect("an error");
    assert!(error.contains("smtp_550"), "{error}");
    assert!(refusing.mails().is_empty());

    let deferring = SmtpSink::start(SinkMode::DeferRecipients).await;
    sqlx::query(
        "UPDATE notification_deliveries SET state = 'pending', attempts = 0, error = NULL, \
         finished_at = NULL, quota_checked = false, next_attempt_at = now()",
    )
    .execute(rig.db.pool())
    .await
    .expect("the delivery is rearmed");
    let running = rig.start_mail(deferring.url(), roomy());
    wait_until("the retry is scheduled", || async {
        let scheduled: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM notification_deliveries \
             WHERE state = 'pending' AND attempts = 1 AND next_attempt_at > now()",
        )
        .fetch_one(rig.db.pool())
        .await
        .expect("the count is readable");
        scheduled == 1
    })
    .await;
    running.stop().await;
    let error = rig.last_error().await.expect("an error");
    assert!(error.contains("smtp_451"), "{error}");
}

#[tokio::test]
async fn a_creator_who_lost_access_to_the_endpoint_is_never_mailed() {
    let rig = rig().await;
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let owner = rig.user("kim").await;
    let former = rig.user("lee").await;
    let endpoint = rig.owned_endpoint(owner).await;
    let address = rig.address(former, "lee@mail.example.test", true).await;
    rig.email_channel(&endpoint, former, address).await;
    rig.capture_on(&endpoint, "POST", "/hook", b"{}").await;

    let running = rig.start_mail(sink.url(), roomy());
    wait_until("the refusal", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(sink.mails().is_empty());
    let error = rig.last_error().await.expect("an error");
    assert!(error.starts_with("recipient_unverified"), "{error}");
}
