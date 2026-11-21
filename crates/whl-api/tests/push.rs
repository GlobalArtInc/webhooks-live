mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::matrix::error_code;
use common::{Reply, SsoHarness, field_codes, mixed_harness, mixed_harness_configured};
use serde_json::{Value, json};
use whl_api::{ApiState, NotifyApiSettings};
use whl_notify::{PushHosts, PushService, Vapid, VapidSubject};
use whl_secrets::Keyring;
use whl_store::Uuid;

const POINT: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const SEED: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const OTHER_POINT: &str =
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
const OTHER_SEED: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const SUBSCRIPTIONS: &str = "/api/v1/push/subscriptions";
const PUBLIC_KEY: &str = "/api/v1/push/public-key";
const FCM_ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send/capability-path";

fn service() -> PushService {
    let subject = VapidSubject::parse("mailto:ops@hooks.example").expect("a subject");
    PushService::new(
        Vapid::generate(subject).expect("a key pair"),
        PushHosts::vendors(),
    )
}

fn with_notifications(state: ApiState) -> ApiState {
    state
        .with_secrets(Arc::new(Keyring::parse(KEYS).expect("a key set")))
        .with_notifications(NotifyApiSettings { enabled: true })
}

async fn harness_with(service: PushService) -> SsoHarness {
    mixed_harness_configured(|state| with_notifications(state).with_push(service)).await
}

fn subscription(endpoint: &str) -> String {
    json!({
        "endpoint": endpoint,
        "expirationTime": null,
        "keys": { "p256dh": POINT, "auth": SEED },
    })
    .to_string()
}

async fn items(harness: &SsoHarness, actor: &common::Actor) -> Vec<Value> {
    let reply = harness
        .send_as(Some(actor), Method::GET, SUBSCRIPTIONS, "")
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json()["items"]
        .as_array()
        .expect("an items array")
        .clone()
}

async fn stored(harness: &SsoHarness) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM push_subscriptions")
        .fetch_one(harness.db.pool())
        .await
        .expect("the count is readable")
}

#[tokio::test]
async fn the_public_key_is_published_to_anyone_and_the_instance_lists_it() {
    let service = service();
    let harness = harness_with(service.clone()).await;
    let reply = harness.send_as(None, Method::GET, PUBLIC_KEY, "").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["public_key"], service.public_key());
    let instance = harness
        .send_as(None, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(
        instance.json()["features"]["push_public_key"],
        service.public_key()
    );

    let off = mixed_harness().await;
    let reply = off.send_as(None, Method::GET, PUBLIC_KEY, "").await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_code(&reply), "feature_disabled");
    let instance = off.send_as(None, Method::GET, "/api/v1/instance", "").await;
    assert!(instance.json()["features"]["push_public_key"].is_null());
}

#[tokio::test]
async fn the_subscription_routes_need_a_signed_in_caller() {
    let harness = harness_with(service()).await;
    let one = format!("{SUBSCRIPTIONS}/{}", Uuid::now_v7());
    for (method, uri, body) in [
        (Method::GET, SUBSCRIPTIONS.to_owned(), String::new()),
        (
            Method::POST,
            SUBSCRIPTIONS.to_owned(),
            subscription(FCM_ENDPOINT),
        ),
        (Method::DELETE, one, String::new()),
    ] {
        let reply = harness.send_as(None, method.clone(), &uri, &body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(error_code(&reply), "sign_in_required");
    }
    assert_eq!(stored(&harness).await, 0);
}

#[tokio::test]
async fn a_subscription_is_stored_listed_without_its_secrets_and_deleted_by_its_owner_only() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    let bob = harness.actor("bob", "bob@hooks.test", true).await;
    let created = harness
        .send_with(
            Some(&alice),
            None,
            &[("user-agent", "Probe/1.0")],
            Method::POST,
            SUBSCRIPTIONS,
            &subscription(FCM_ENDPOINT),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let id = created.json()["id"].as_str().expect("an id").to_owned();
    assert!(created.json()["created_at"].is_string());

    let mine = items(&harness, &alice).await;
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["id"], id);
    assert_eq!(mine[0]["host"], "fcm.googleapis.com");
    assert_eq!(mine[0]["user_agent"], "Probe/1.0");
    let text = Value::Array(mine).to_string();
    for secret in ["capability-path", POINT, SEED] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert!(items(&harness, &bob).await.is_empty());

    let uri = format!("{SUBSCRIPTIONS}/{id}");
    let stolen = harness.send_as(Some(&bob), Method::DELETE, &uri, "").await;
    assert_eq!(stolen.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&stolen), "subscription_not_found");
    assert_eq!(items(&harness, &alice).await.len(), 1);

    let deleted = harness
        .send_as(Some(&alice), Method::DELETE, &uri, "")
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    for uri in [uri, format!("{SUBSCRIPTIONS}/not-an-id")] {
        let gone = harness
            .send_as(Some(&alice), Method::DELETE, &uri, "")
            .await;
        assert_eq!(gone.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(error_code(&gone), "subscription_not_found");
    }
}

#[tokio::test]
async fn a_foreign_host_or_a_malformed_subscription_is_refused_with_field_errors() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    let bad = json!({
        "endpoint": "https://evil.test/push",
        "keys": { "p256dh": "AAAA", "auth": "AAAA" },
    })
    .to_string();
    let reply = harness
        .send_as(Some(&alice), Method::POST, SUBSCRIPTIONS, &bad)
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = field_codes(&reply.json());
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(field, code)| (field.as_str(), code.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("endpoint", "host_not_allowed"),
            ("keys.p256dh", "invalid_format"),
            ("keys.auth", "invalid_format"),
        ]
    );
    for endpoint in [
        "http://fcm.googleapis.com/fcm/send/x",
        "https://fcm.googleapis.com:8443/fcm/send/x",
        "https://user@fcm.googleapis.com/fcm/send/x",
        "https://127.0.0.1/fcm/send/x",
        "https://fcm.googleapis.com.evil.test/fcm/send/x",
    ] {
        let reply = harness
            .send_as(
                Some(&alice),
                Method::POST,
                SUBSCRIPTIONS,
                &subscription(endpoint),
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{endpoint}");
        assert_eq!(field_codes(&reply.json())[0].0, "endpoint", "{endpoint}");
    }
    assert_eq!(stored(&harness).await, 0);
}

#[tokio::test]
async fn the_subscription_count_is_limited_per_user_but_an_existing_endpoint_can_be_renewed() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    for index in 0..10 {
        let reply = harness
            .send_as(
                Some(&alice),
                Method::POST,
                SUBSCRIPTIONS,
                &subscription(&format!("https://fcm.googleapis.com/fcm/send/{index}")),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{index}");
    }
    let eleventh = harness
        .send_as(
            Some(&alice),
            Method::POST,
            SUBSCRIPTIONS,
            &subscription("https://fcm.googleapis.com/fcm/send/eleven"),
        )
        .await;
    assert_eq!(eleventh.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&eleventh), "limit_reached");
    assert_eq!(
        eleventh.json()["error"]["limit"],
        json!({ "name": "push_subscriptions", "max": 10 })
    );
    let renewed = harness
        .send_as(
            Some(&alice),
            Method::POST,
            SUBSCRIPTIONS,
            &subscription("https://fcm.googleapis.com/fcm/send/3"),
        )
        .await;
    assert_eq!(renewed.status, StatusCode::CREATED);
    assert_eq!(stored(&harness).await, 10);
}

#[tokio::test]
async fn an_endpoint_known_to_another_user_moves_only_with_the_same_keys() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    let bob = harness.actor("bob", "bob@hooks.test", true).await;
    let first = harness
        .send_as(
            Some(&alice),
            Method::POST,
            SUBSCRIPTIONS,
            &subscription(FCM_ENDPOINT),
        )
        .await;
    assert_eq!(first.status, StatusCode::CREATED);
    sqlx::query("UPDATE push_subscriptions SET last_success_at = now()")
        .execute(harness.db.pool())
        .await
        .expect("the success time is set");

    let foreign_keys = json!({
        "endpoint": FCM_ENDPOINT,
        "keys": { "p256dh": OTHER_POINT, "auth": OTHER_SEED },
    })
    .to_string();
    let refused = harness
        .send_as(Some(&bob), Method::POST, SUBSCRIPTIONS, &foreign_keys)
        .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&refused), "already_registered");
    assert!(!String::from_utf8_lossy(&refused.body).contains("alice"));
    assert_eq!(items(&harness, &alice).await.len(), 1);
    assert!(items(&harness, &bob).await.is_empty());

    let moved = harness
        .send_as(
            Some(&bob),
            Method::POST,
            SUBSCRIPTIONS,
            &subscription(FCM_ENDPOINT),
        )
        .await;
    assert_eq!(moved.status, StatusCode::CREATED);
    assert_ne!(moved.json()["id"], first.json()["id"]);
    assert!(items(&harness, &alice).await.is_empty());
    let theirs = items(&harness, &bob).await;
    assert_eq!(theirs.len(), 1);
    assert!(theirs[0]["last_success_at"].is_null());
    assert_eq!(stored(&harness).await, 1);
}

#[tokio::test]
async fn equivalent_spellings_of_one_endpoint_collapse_to_one_normalised_row() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    let mut ids = Vec::new();
    for variant in [
        FCM_ENDPOINT,
        "https:fcm.googleapis.com/fcm/send/capability-path",
        "https:////fcm.googleapis.com/fcm/send/capability-path",
        "HTTPS://FCM.GoogleAPIs.com:443/fcm/send/capability-path",
        "https://\u{ff46}\u{ff43}\u{ff4d}.googleapis.com/fcm/send/capability-path",
    ] {
        let reply = harness
            .send_as(
                Some(&alice),
                Method::POST,
                SUBSCRIPTIONS,
                &subscription(variant),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{variant}");
        ids.push(reply.json()["id"].as_str().expect("an id").to_owned());
    }
    assert!(ids.iter().all(|id| *id == ids[0]));
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT endpoint_url, host FROM push_subscriptions")
            .fetch_all(harness.db.pool())
            .await
            .expect("the rows are readable");
    assert_eq!(
        rows,
        [(FCM_ENDPOINT.to_owned(), "fcm.googleapis.com".to_owned())]
    );
    assert_eq!(
        items(&harness, &alice).await[0]["host"],
        "fcm.googleapis.com"
    );

    for hidden in [
        "https://fcm.googleapis.com/fcm/\tsend/x",
        "https://fcm.googleapis.com/fcm/send/x\r\n",
    ] {
        let reply = harness
            .send_as(
                Some(&alice),
                Method::POST,
                SUBSCRIPTIONS,
                &subscription(hidden),
            )
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{hidden:?}");
        assert_eq!(field_codes(&reply.json())[0].0, "endpoint");
    }
    assert_eq!(stored(&harness).await, 1);
}

#[tokio::test]
async fn a_push_channel_is_recorded_with_its_creator_and_takes_no_configuration() {
    let harness = harness_with(service()).await;
    let alice = harness.actor("alice", "alice@hooks.test", true).await;
    let endpoint = harness.endpoint_for(&alice).await;
    let uri = format!("/api/v1/endpoints/{endpoint}/notifications");
    let created = harness
        .send_as(
            Some(&alice),
            Method::POST,
            &uri,
            &json!({ "kind": "push", "name": "phone", "config": {} }).to_string(),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.json()["kind"], "push");
    assert_eq!(created.json()["config"], json!({}));
    let creator: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM notification_channels")
        .fetch_one(harness.db.pool())
        .await
        .expect("the channel row is readable");
    assert_eq!(creator, Some(alice.id));

    let extra = harness
        .send_as(
            Some(&alice),
            Method::POST,
            &uri,
            &json!({ "kind": "push", "name": "x", "config": { "url": "https://x.test" } })
                .to_string(),
        )
        .await;
    assert_eq!(extra.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(field_codes(&extra.json())[0].0, "config.url");
}

#[tokio::test]
async fn a_push_channel_needs_a_signed_in_caller_and_a_configured_push_service() {
    let harness = harness_with(service()).await;
    let anonymous = harness.anonymous_endpoint(None).await;
    let body = json!({ "kind": "push", "name": "phone", "config": {} }).to_string();
    let refused = harness
        .send_as(
            None,
            Method::POST,
            &format!("/api/v1/endpoints/{anonymous}/notifications"),
            &body,
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&refused), "sign_in_required");

    let off = mixed_harness_configured(with_notifications).await;
    let alice = off.actor("alice", "alice@hooks.test", true).await;
    let endpoint = off.endpoint_for(&alice).await;
    let unavailable: Reply = off
        .send_as(
            Some(&alice),
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/notifications"),
            &body,
        )
        .await;
    assert_eq!(unavailable.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_code(&unavailable), "feature_disabled");
}
