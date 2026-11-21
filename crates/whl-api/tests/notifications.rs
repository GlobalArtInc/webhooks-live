mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{Reply, api_settings, bodiless, create, field_codes, request, send};
use serde_json::{Value, json};
use whl_api::{ApiState, NotifyApiSettings};
use whl_secrets::Keyring;
use whl_store::Store;
use whl_testkit::TestDb;

const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const SLACK: &str = "https://hooks.slack.com/services/T0/B0/very-secret-part";

struct Rig {
    db: TestDb,
    app: Router,
}

async fn rig(keyring: bool, enabled: bool) -> Rig {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let mut state =
        ApiState::new(store, api_settings()).with_notifications(NotifyApiSettings { enabled });
    if keyring {
        state = state.with_secrets(Arc::new(Keyring::parse(KEYS).expect("a valid key set")));
    }
    Rig {
        db,
        app: whl_api::router(state),
    }
}

fn uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/notifications")
}

async fn post(app: &Router, endpoint: &str, body: Value) -> Reply {
    send(
        app,
        request(Method::POST, &uri(endpoint), &body.to_string()),
    )
    .await
}

fn slack(name: &str) -> Value {
    json!({ "kind": "slack", "name": name, "config": { "url": SLACK } })
}

#[tokio::test]
async fn a_created_channel_never_returns_or_stores_its_url_in_plain_text() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = post(&rig.app, &endpoint, slack("alerts")).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(
        reply.json()["config"]["url"],
        "https://hooks.slack.com/services/..."
    );
    let list = send(&rig.app, bodiless(Method::GET, &uri(&endpoint))).await;
    assert!(!String::from_utf8_lossy(&list.body).contains("very-secret-part"));
    let stored: String = sqlx::query_scalar(
        "SELECT config::text || encode(secret_ciphertext, 'escape') FROM notification_channels",
    )
    .fetch_one(rig.db.pool())
    .await
    .expect("the row is readable");
    assert!(!stored.contains("very-secret-part"));
}

#[tokio::test]
async fn bad_targets_are_refused_with_field_errors() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let off_pattern = post(
        &rig.app,
        &endpoint,
        json!({ "kind": "slack", "name": "a", "config": { "url": "https://evil.test/services/x" } }),
    )
    .await;
    assert_eq!(off_pattern.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&off_pattern.json()),
        [("config.url".to_owned(), "invalid_format".to_owned())]
    );
    let private = post(
        &rig.app,
        &endpoint,
        json!({ "kind": "webhook", "name": "b", "config": { "url": "http://127.0.0.1/hook" } }),
    )
    .await;
    assert_eq!(private.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(field_codes(&private.json())[0].1, "target_refused");
    let missing = post(
        &rig.app,
        &endpoint,
        json!({ "kind": "telegram", "name": "c", "config": { "token_secret": "NOPE", "chat_id": "123" } }),
    )
    .await;
    assert_eq!(field_codes(&missing.json())[0].0, "config.token_secret");
}

#[tokio::test]
async fn the_channel_count_is_limited_per_endpoint() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for index in 0..10 {
        let reply = post(&rig.app, &endpoint, slack(&format!("c{index}"))).await;
        assert_eq!(reply.status, StatusCode::CREATED);
    }
    let reply = post(&rig.app, &endpoint, slack("extra")).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.json()["error"]["code"], "limit_reached");
}

#[tokio::test]
async fn the_test_route_queues_a_delivery_and_is_throttled() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let channel = post(&rig.app, &endpoint, slack("alerts")).await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let test_uri = format!("{}/{channel}/test", uri(&endpoint));
    for _ in 0..5 {
        let reply = send(&rig.app, bodiless(Method::POST, &test_uri)).await;
        assert_eq!(reply.status, StatusCode::ACCEPTED);
        assert_eq!(reply.json()["state"], "pending");
    }
    let reply = send(&rig.app, bodiless(Method::POST, &test_uri)).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_patch_can_rename_but_not_change_the_kind() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let channel = post(&rig.app, &endpoint, slack("alerts")).await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let channel_uri = format!("{}/{channel}", uri(&endpoint));
    let renamed = send(
        &rig.app,
        request(
            Method::PATCH,
            &channel_uri,
            &json!({ "name": "pager" }).to_string(),
        ),
    )
    .await;
    assert_eq!(renamed.json()["name"], "pager");
    let kind = send(
        &rig.app,
        request(
            Method::PATCH,
            &channel_uri,
            &json!({ "kind": "discord" }).to_string(),
        ),
    )
    .await;
    assert_eq!(
        field_codes(&kind.json())[0],
        ("kind".to_owned(), "not_allowed".to_owned())
    );
}

#[tokio::test]
async fn the_routes_need_a_keyring_and_the_feature_switch() {
    let without_keyring = rig(false, true).await;
    let endpoint = create(&without_keyring.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = post(&without_keyring.app, &endpoint, slack("a")).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.json()["error"]["code"], "feature_disabled");

    let disabled = rig(true, false).await;
    let endpoint = create(&disabled.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = send(&disabled.app, bodiless(Method::GET, &uri(&endpoint))).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_masked_url_cannot_overwrite_the_stored_one_and_the_instance_is_no_target() {
    let rig = rig(true, true).await;
    let endpoint = create(&rig.app, "{}").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let channel = post(
        &rig.app,
        &endpoint,
        json!({ "kind": "webhook", "name": "w", "config": { "url": "https://93.184.216.34/hook" } }),
    )
    .await;
    assert_eq!(channel.status, StatusCode::CREATED);
    let channel = channel.json();
    let masked = channel["config"]["url"].as_str().unwrap().to_owned();
    let channel_uri = format!("{}/{}", uri(&endpoint), channel["id"].as_str().unwrap());
    let reply = send(
        &rig.app,
        request(
            Method::PATCH,
            &channel_uri,
            &json!({ "config": { "url": masked } }).to_string(),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json())[0],
        ("config.url".to_owned(), "invalid_format".to_owned())
    );

    let own = post(
        &rig.app,
        &endpoint,
        json!({ "kind": "webhook", "name": "x", "config": { "url": format!("https://whl.test/{endpoint}") } }),
    )
    .await;
    assert_eq!(
        field_codes(&own.json())[0],
        ("config.url".to_owned(), "target_refused".to_owned())
    );
}
