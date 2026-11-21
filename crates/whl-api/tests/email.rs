mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{Actor, Reply, SsoHarness, mixed_harness_configured};
use serde_json::{Value, json};
use whl_api::{EmailApi, EmailApiLimits, NotifyApiSettings};
use whl_notify::{CapturedMail, MailSettings, Mailer, SinkMode, SmtpSink, issue_disable_token};
use whl_secrets::Keyring;
use whl_store::Uuid;
use whl_store::notify::{AddressLimits, AddressOutcome, AddressRequest};

const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const ADDRESSES: &str = "/api/v1/email-addresses";

fn keyring() -> Keyring {
    Keyring::parse(KEYS).expect("a valid key set")
}

async fn rig() -> (SsoHarness, SmtpSink) {
    rig_with(SinkMode::Accept, EmailApiLimits::default()).await
}

async fn rig_with(mode: SinkMode, limits: EmailApiLimits) -> (SsoHarness, SmtpSink) {
    let sink = SmtpSink::start(mode).await;
    let mailer = Mailer::new(&MailSettings::new(
        sink.url(),
        "noreply@hooks.example.test".to_owned(),
    ))
    .expect("the mailer is valid");
    let harness = mixed_harness_configured(|state| {
        state
            .with_secrets(Arc::new(keyring()))
            .with_notifications(NotifyApiSettings::enabled())
            .with_email(EmailApi::new(mailer, limits))
    })
    .await;
    (harness, sink)
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn token_in(mail: &CapturedMail) -> String {
    let text = mail.text();
    let start = text.find("token=").expect("a link with a token") + "token=".len();
    text[start..]
        .chars()
        .take_while(|character| !character.is_whitespace())
        .collect()
}

async fn add(harness: &SsoHarness, actor: Option<&Actor>, address: &str) -> Reply {
    harness
        .send_as(
            actor,
            Method::POST,
            ADDRESSES,
            &json!({ "address": address }).to_string(),
        )
        .await
}

async fn verify(harness: &SsoHarness, token: &str) -> Reply {
    harness
        .send_as(
            None,
            Method::POST,
            &format!("{ADDRESSES}/verify"),
            &json!({ "token": token }).to_string(),
        )
        .await
}

async fn verified_address(
    harness: &SsoHarness,
    sink: &SmtpSink,
    actor: &Actor,
    address: &str,
) -> String {
    let added = add(harness, Some(actor), address).await;
    assert_eq!(added.status, StatusCode::CREATED, "{:?}", added.json());
    let token = token_in(sink.mails().last().expect("a verification mail"));
    assert_eq!(verify(harness, &token).await.status, StatusCode::OK);
    added.json()["id"].as_str().expect("an id").to_owned()
}

async fn channel(harness: &SsoHarness, actor: &Actor, endpoint: &str, address_id: &str) -> Reply {
    harness
        .send_as(
            Some(actor),
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/notifications"),
            &json!({
                "kind": "email",
                "name": "mail",
                "config": { "address_id": address_id },
            })
            .to_string(),
        )
        .await
}

async fn enabled(harness: &SsoHarness, channel_id: &str) -> bool {
    sqlx::query_scalar("SELECT enabled FROM notification_channels WHERE id = $1::uuid")
        .bind(channel_id)
        .fetch_one(harness.db.pool())
        .await
        .expect("the channel row exists")
}

#[tokio::test]
async fn an_added_address_gets_a_token_mail_and_the_link_verifies_it_once() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;

    let added = add(&harness, Some(&ann), "Ann.Other@Mail.Example.Test").await;
    assert_eq!(added.status, StatusCode::CREATED);
    assert_eq!(added.json()["verified"], false);
    assert_eq!(added.json()["address"], "Ann.Other@mail.example.test");
    let mails = sink.mails();
    assert_eq!(mails.len(), 1);
    assert_eq!(
        mails[0].recipients,
        vec!["Ann.Other@mail.example.test".to_owned()]
    );
    let token = token_in(&mails[0]);

    let verified = verify(&harness, &token).await;
    assert_eq!(verified.status, StatusCode::OK);
    assert_eq!(verified.json()["verified"], true);
    let again = verify(&harness, &token).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&again), "invalid_token");

    let listed = harness
        .send_as(Some(&ann), Method::GET, ADDRESSES, "")
        .await
        .json();
    assert_eq!(listed["items"][0]["verified"], true);
}

#[tokio::test]
async fn an_expired_or_unknown_token_verifies_nothing() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    add(&harness, Some(&ann), "late@mail.example.test").await;
    let token = token_in(&sink.mails()[0]);
    sqlx::query("UPDATE email_addresses SET verify_expires_at = now() - interval '1 minute'")
        .execute(harness.db.pool())
        .await
        .expect("the token is aged");

    assert_eq!(code(&verify(&harness, &token).await), "invalid_token");
    assert_eq!(
        code(&verify(&harness, &"A".repeat(43)).await),
        "invalid_token"
    );
    assert_eq!(code(&verify(&harness, "short").await), "invalid_token");
}

#[tokio::test]
async fn a_resend_is_refused_inside_the_cooldown_and_replaces_the_token_after_it() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    assert_eq!(
        add(&harness, Some(&ann), "ann@mail.example.test")
            .await
            .status,
        StatusCode::CREATED
    );
    let first = token_in(&sink.mails()[0]);

    let too_soon = add(&harness, Some(&ann), "ann@mail.example.test").await;
    assert_eq!(too_soon.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sink.mails().len(), 1);

    sqlx::query("DELETE FROM notification_counters WHERE subject LIKE 'vc:%'")
        .execute(harness.db.pool())
        .await
        .expect("the cooldown is over");
    let resent = add(&harness, Some(&ann), "ann@mail.example.test").await;
    assert_eq!(resent.status, StatusCode::OK);
    assert_eq!(sink.mails().len(), 2);
    let second = token_in(&sink.mails()[1]);
    assert_ne!(first, second);
    assert_eq!(code(&verify(&harness, &first).await), "invalid_token");
    assert_eq!(verify(&harness, &second).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_user_has_at_most_five_addresses_and_five_verification_mails_a_day() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    for index in 1..=5 {
        let reply = add(&harness, Some(&ann), &format!("a{index}@mail.example.test")).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{index}");
    }
    assert_eq!(sink.mails().len(), 5);

    let sixth = add(&harness, Some(&ann), "a6@mail.example.test").await;
    assert_eq!(sixth.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(sixth.json()["error"]["limit"]["name"], "email_addresses");

    sqlx::query("DELETE FROM notification_counters WHERE subject LIKE 'vc:%'")
        .execute(harness.db.pool())
        .await
        .expect("the cooldown is over");
    let resend = add(&harness, Some(&ann), "a1@mail.example.test").await;
    assert_eq!(resend.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sink.mails().len(), 5);
}

#[tokio::test]
async fn verification_mails_to_one_address_are_capped_across_users() {
    let (harness, sink) = rig().await;
    for tag in ["u1", "u2", "u3"] {
        let actor = harness
            .actor(tag, &format!("{tag}@login.example.test"), true)
            .await;
        let reply = add(&harness, Some(&actor), "victim@mail.example.test").await;
        assert_eq!(reply.status, StatusCode::CREATED, "{tag}");
    }
    let fourth = harness.actor("u4", "u4@login.example.test", true).await;

    let refused = add(&harness, Some(&fourth), "victim@mail.example.test").await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sink.mails().len(), 3);
}

#[tokio::test]
async fn anonymous_callers_cannot_add_addresses_or_cause_mail() {
    let (harness, sink) = rig().await;

    let reply = add(&harness, None, "anyone@mail.example.test").await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&reply), "sign_in_required");
    assert!(sink.mails().is_empty());
    assert!(sink.commands().is_empty());
}

#[tokio::test]
async fn the_idp_verified_sign_in_address_counts_as_verified_without_a_mail() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let bob = harness.actor("bob", "bob@login.example.test", false).await;

    let own = add(&harness, Some(&ann), "ANN@login.example.test").await;
    assert_eq!(own.status, StatusCode::CREATED);
    assert_eq!(own.json()["verified"], true);
    assert!(sink.mails().is_empty());

    let unproven = add(&harness, Some(&bob), "bob@login.example.test").await;
    assert_eq!(unproven.status, StatusCode::CREATED);
    assert_eq!(unproven.json()["verified"], false);
    assert_eq!(sink.mails().len(), 1);
}

#[tokio::test]
async fn an_email_channel_needs_a_verified_address_of_the_caller() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let bob = harness.actor("bob", "bob@login.example.test", true).await;
    let endpoint = harness.endpoint_for(&ann).await;
    let unverified = add(&harness, Some(&ann), "pending@mail.example.test").await;
    let unverified_id = unverified.json()["id"].as_str().expect("an id").to_owned();
    let foreign = verified_address(&harness, &sink, &bob, "bob@mail.example.test").await;

    for refused in [unverified_id.as_str(), foreign.as_str()] {
        let reply = channel(&harness, &ann, &endpoint, refused).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert_eq!(
            common::field_codes(&reply.json()),
            vec![("config.address_id".to_owned(), "not_allowed".to_owned())]
        );
    }

    let own = verified_address(&harness, &sink, &ann, "ann@mail.example.test").await;
    let created = channel(&harness, &ann, &endpoint, &own).await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.json()["kind"], "email");
    assert_eq!(created.json()["config"]["address_id"], own.as_str());
    let created_by: Option<Uuid> =
        sqlx::query_scalar("SELECT created_by FROM notification_channels")
            .fetch_one(harness.db.pool())
            .await
            .expect("the channel row exists");
    assert_eq!(created_by, Some(ann.id));
}

#[tokio::test]
async fn an_endpoint_without_an_owner_cannot_have_an_email_channel() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let own = verified_address(&harness, &sink, &ann, "ann@mail.example.test").await;
    let ownerless = harness.anonymous_endpoint(None).await;

    let reply = channel(&harness, &ann, &ownerless, &own).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(common::matrix::error_code(&reply), "tier_not_allowed");
}

#[tokio::test]
async fn the_one_click_link_disables_the_channel_and_a_forged_token_does_not() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let endpoint = harness.endpoint_for(&ann).await;
    let own = verified_address(&harness, &sink, &ann, "ann@mail.example.test").await;
    let created = channel(&harness, &ann, &endpoint, &own).await.json();
    let channel_id = created["id"].as_str().expect("an id").to_owned();
    let token = issue_disable_token(
        &keyring(),
        Uuid::try_parse(&channel_id).expect("a channel id"),
    )
    .expect("a token");
    let last = if token.ends_with('A') { 'B' } else { 'A' };
    let forged = format!("{}{last}", &token[..token.len() - 1]);

    let refused = harness
        .send_as(
            None,
            Method::POST,
            "/api/v1/notifications/disable",
            &json!({ "token": forged }).to_string(),
        )
        .await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
    assert!(enabled(&harness, &channel_id).await);

    let one_click = harness
        .send_with(
            None,
            None,
            &[("content-type", "application/x-www-form-urlencoded")],
            Method::POST,
            &format!("/api/v1/notifications/disable?token={token}"),
            "List-Unsubscribe=One-Click",
        )
        .await;
    assert_eq!(one_click.status, StatusCode::NO_CONTENT);
    assert!(!enabled(&harness, &channel_id).await);
}

#[tokio::test]
async fn only_the_creator_can_re_enable_an_email_channel() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let bob = harness.actor("bob", "bob@login.example.test", true).await;
    let endpoint = harness.endpoint_for(&ann).await;
    let member = harness
        .send_as(
            Some(&ann),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            &json!({ "email": bob.email, "role": "editor" }).to_string(),
        )
        .await;
    assert_eq!(member.status, StatusCode::OK);
    let own = verified_address(&harness, &sink, &ann, "ann@mail.example.test").await;
    let created = channel(&harness, &ann, &endpoint, &own).await.json();
    let channel_id = created["id"].as_str().expect("an id").to_owned();
    sqlx::query("UPDATE notification_channels SET enabled = false")
        .execute(harness.db.pool())
        .await
        .expect("the channel is disabled");
    let uri = format!("/api/v1/endpoints/{endpoint}/notifications/{channel_id}");
    let enable = json!({ "enabled": true }).to_string();

    let by_member = harness
        .send_as(Some(&bob), Method::PATCH, &uri, &enable)
        .await;
    assert_eq!(by_member.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&by_member), "creator_only");
    assert!(!enabled(&harness, &channel_id).await);

    let by_creator = harness
        .send_as(Some(&ann), Method::PATCH, &uri, &enable)
        .await;
    assert_eq!(by_creator.status, StatusCode::OK);
    assert!(enabled(&harness, &channel_id).await);

    let rename = json!({ "name": "renamed" }).to_string();
    let renamed_by_member = harness
        .send_as(Some(&bob), Method::PATCH, &uri, &rename)
        .await;
    assert_eq!(renamed_by_member.status, StatusCode::FORBIDDEN);
    let disable = json!({ "enabled": false }).to_string();
    let disabled_by_member = harness
        .send_as(Some(&bob), Method::PATCH, &uri, &disable)
        .await;
    assert_eq!(disabled_by_member.status, StatusCode::OK);
    assert!(!enabled(&harness, &channel_id).await);
}

#[tokio::test]
async fn deleting_an_address_disables_the_channels_that_use_it() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let endpoint = harness.endpoint_for(&ann).await;
    let own = verified_address(&harness, &sink, &ann, "ann@mail.example.test").await;
    let created = channel(&harness, &ann, &endpoint, &own).await.json();
    let channel_id = created["id"].as_str().expect("an id").to_owned();
    let uri = format!("{ADDRESSES}/{own}");

    let deleted = harness.send_as(Some(&ann), Method::DELETE, &uri, "").await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(!enabled(&harness, &channel_id).await);
    let again = harness.send_as(Some(&ann), Method::DELETE, &uri, "").await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
    let listed: Value = harness
        .send_as(Some(&ann), Method::GET, ADDRESSES, "")
        .await
        .json();
    assert_eq!(listed["items"], json!([]));
}

#[tokio::test]
async fn removing_or_demoting_the_creator_disables_their_email_channels() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let bob = harness.actor("bob", "bob@login.example.test", true).await;
    let endpoint = harness.endpoint_for(&ann).await;
    let members = format!("/api/v1/endpoints/{endpoint}/members");
    let grant = |role: &str| json!({ "email": bob.email, "role": role }).to_string();
    harness
        .send_as(Some(&ann), Method::PUT, &members, &grant("editor"))
        .await;
    let address = verified_address(&harness, &sink, &bob, "bob@mail.example.test").await;
    let created = channel(&harness, &bob, &endpoint, &address).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let channel_id = created.json()["id"].as_str().expect("an id").to_owned();

    harness
        .send_as(Some(&ann), Method::PUT, &members, &grant("viewer"))
        .await;
    assert!(!enabled(&harness, &channel_id).await);

    sqlx::query("UPDATE notification_channels SET enabled = true")
        .execute(harness.db.pool())
        .await
        .expect("the channel is enabled");
    let removed = harness
        .send_as(
            Some(&ann),
            Method::DELETE,
            &format!("{members}/{}", bob.id.hyphenated()),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    assert!(!enabled(&harness, &channel_id).await);
}

#[tokio::test]
async fn plus_tags_and_gmail_dots_do_not_dodge_the_per_address_cap() {
    let (harness, sink) = rig().await;
    let variants = [
        "vic.tim+1@gmail.com",
        "victim+2@googlemail.com",
        "v.i.c.t.i.m+3@gmail.com",
    ];
    for (index, address) in variants.iter().enumerate() {
        let actor = harness
            .actor(
                &format!("u{index}"),
                &format!("u{index}@login.example.test"),
                true,
            )
            .await;
        let reply = add(&harness, Some(&actor), address).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{address}");
    }
    let fourth = harness.actor("u9", "u9@login.example.test", true).await;

    let refused = add(&harness, Some(&fourth), "victim+4@gmail.com").await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(sink.mails().len(), 3);
}

#[tokio::test]
async fn verification_mails_are_capped_per_domain_and_globally() {
    let limits = EmailApiLimits {
        verifications_per_domain_per_day: 2,
        verifications_per_day_global: 3,
        ..EmailApiLimits::default()
    };
    let (harness, sink) = rig_with(SinkMode::Accept, limits).await;
    let mut statuses = Vec::new();
    for (index, address) in [
        "a@one.example.test",
        "b@one.example.test",
        "c@one.example.test",
        "d@two.example.test",
        "e@three.example.test",
    ]
    .iter()
    .enumerate()
    {
        let actor = harness
            .actor(
                &format!("d{index}"),
                &format!("d{index}@login.example.test"),
                true,
            )
            .await;
        statuses.push(add(&harness, Some(&actor), address).await.status);
    }

    assert_eq!(
        statuses,
        vec![
            StatusCode::CREATED,
            StatusCode::CREATED,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::CREATED,
            StatusCode::TOO_MANY_REQUESTS,
        ]
    );
    assert_eq!(sink.mails().len(), 3);
}

#[tokio::test]
async fn a_failed_send_leaves_no_address_and_refunds_the_quota() {
    let (harness, sink) = rig_with(SinkMode::RefuseRecipients, EmailApiLimits::default()).await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;

    let reply = add(&harness, Some(&ann), "ann@mail.example.test").await;

    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code(&reply), "mail_unavailable");
    assert!(sink.mails().is_empty());
    let listed = harness
        .send_as(Some(&ann), Method::GET, ADDRESSES, "")
        .await
        .json();
    assert_eq!(listed["items"], json!([]));
    let charged: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(count), 0)::int8 FROM notification_counters WHERE subject ~ '^v'",
    )
    .fetch_one(harness.db.pool())
    .await
    .expect("the counters are readable");
    assert_eq!(charged, 0);
}

#[tokio::test]
async fn undoing_a_resend_restores_the_previous_token_and_the_quota() {
    let (harness, _sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    let limits = AddressLimits {
        max_addresses: 5,
        verifications_per_user_per_day: 5,
        verifications_per_address_per_day: 3,
        verifications_per_domain_per_day: 200,
        verifications_per_day_global: 2000,
        resend_cooldown_secs: 60,
    };
    let request = |hash: &'static [u8]| AddressRequest {
        id: Uuid::now_v7(),
        address: "ann@mail.example.test",
        key: "ann@mail.example.test",
        domain: "mail.example.test",
        token_hash: hash,
        lifetime: std::time::Duration::from_secs(3600),
    };
    static FIRST: [u8; 32] = [1_u8; 32];
    static SECOND: [u8; 32] = [2_u8; 32];
    let created = harness
        .store
        .request_email_address(ann.id, &request(&FIRST), &limits)
        .await
        .expect("the request is stored");
    assert!(matches!(created, AddressOutcome::Created { .. }));
    sqlx::query("DELETE FROM notification_counters WHERE subject LIKE 'vc:%'")
        .execute(harness.db.pool())
        .await
        .expect("the cooldown is over");
    let AddressOutcome::Resent { undo, .. } = harness
        .store
        .request_email_address(ann.id, &request(&SECOND), &limits)
        .await
        .expect("the resend is stored")
    else {
        panic!("expected a resend");
    };

    harness
        .store
        .undo_email_request(&undo)
        .await
        .expect("the resend is undone");

    let stored: Vec<u8> = sqlx::query_scalar("SELECT verify_token_hash FROM email_addresses")
        .fetch_one(harness.db.pool())
        .await
        .expect("the address row exists");
    assert_eq!(stored, FIRST.to_vec());
    let per_user: i32 =
        sqlx::query_scalar("SELECT count FROM notification_counters WHERE subject LIKE 'vu:%'")
            .fetch_one(harness.db.pool())
            .await
            .expect("the counter exists");
    assert_eq!(per_user, 1);
}

#[tokio::test]
async fn the_token_routes_accept_the_query_and_refuse_get() {
    let (harness, sink) = rig().await;
    let ann = harness.actor("ann", "ann@login.example.test", true).await;
    add(&harness, Some(&ann), "ann@mail.example.test").await;
    let token = token_in(&sink.mails()[0]);

    for uri in [
        format!("{ADDRESSES}/verify?token={token}"),
        format!("/api/v1/notifications/disable?token={token}"),
    ] {
        let reply = harness.send_as(None, Method::GET, &uri, "").await;
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{uri}");
    }
    let verified = harness
        .send_as(
            None,
            Method::POST,
            &format!("{ADDRESSES}/verify?token={token}"),
            "",
        )
        .await;
    assert_eq!(verified.status, StatusCode::OK);
    assert_eq!(verified.json()["verified"], true);
}
