mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{
    Actor, RETENTION, Reply, SsoHarness, api_settings, bodiless, field_codes, harness,
    mixed_harness, request, send,
};
use serde_json::{Value, json};
use whl_api::ApiState;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::Keyring;
use whl_store::{CaptureAuthRecord, GrantedRole};

const USER: &str = "alice";
const PASSWORD: &str = "test-password-one";
const OTHER_PASSWORD: &str = "test-password-two";
const HEADER_VALUE: &str = "test-header-value-one";

struct Rig {
    harness: SsoHarness,
    owner: Actor,
    keyring: Arc<Keyring>,
}

async fn rig(with_keyring: bool) -> Rig {
    let spec = format!("1:{}", STANDARD.encode([1_u8; 32]));
    let keyring = Arc::new(Keyring::parse(&spec).expect("the key set is valid"));
    let mut harness = mixed_harness().await;
    if with_keyring {
        let state = ApiState::new(harness.store.clone(), api_settings())
            .with_access(harness.access.clone())
            .with_secrets(Arc::clone(&keyring));
        harness.app = whl_api::router(state);
    }
    let owner = harness.actor("owner", "owner@example.test", true).await;
    Rig {
        harness,
        owner,
        keyring,
    }
}

impl Rig {
    async fn endpoint(&self) -> String {
        self.harness.endpoint_for(&self.owner).await
    }

    async fn patch(&self, endpoint: &str, body: &Value) -> Reply {
        self.harness
            .send_as(
                Some(&self.owner),
                Method::PATCH,
                &uri(endpoint),
                &body.to_string(),
            )
            .await
    }

    async fn read(&self, endpoint: &str) -> Reply {
        self.harness
            .send_as(Some(&self.owner), Method::GET, &uri(endpoint), "")
            .await
    }
}

fn field(path: &str, code: &str) -> (String, String) {
    (path.to_owned(), code.to_owned())
}

fn uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}")
}

async fn open_endpoint(app: &Router) -> String {
    common::create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

#[tokio::test]
async fn a_new_endpoint_has_no_allow_list_and_no_capture_auth() {
    let harness = harness().await;

    let created = common::create(&harness.app, "").await;

    assert_eq!(created["sender_allow_cidrs"], json!([]));
    assert!(created["capture_auth"].is_null());
}

#[tokio::test]
async fn the_allow_list_is_set_normalised_returned_and_cleared() {
    let rig = rig(false).await;
    let id = rig.endpoint().await;

    let set = rig
        .patch(
            &id,
            &json!({"sender_allow_cidrs": ["203.0.113.7", "2001:db8::/32", "203.0.113.7"]}),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK);
    assert_eq!(
        set.json()["sender_allow_cidrs"],
        json!(["203.0.113.7/32", "2001:db8::/32"])
    );

    let read = rig.read(&id).await.json();
    assert_eq!(
        read["sender_allow_cidrs"],
        json!(["203.0.113.7/32", "2001:db8::/32"])
    );

    let untouched = rig.patch(&id, &json!({"title": "renamed"})).await.json();
    assert_eq!(
        untouched["sender_allow_cidrs"].as_array().map(Vec::len),
        Some(2)
    );

    let cleared = rig.patch(&id, &json!({"sender_allow_cidrs": []})).await;
    assert_eq!(cleared.json()["sender_allow_cidrs"], json!([]));
}

#[tokio::test]
async fn invalid_allow_list_entries_are_refused_one_violation_each() {
    let rig = rig(false).await;
    let id = rig.endpoint().await;

    let bad = rig
        .patch(
            &id,
            &json!({"sender_allow_cidrs": ["203.0.113.7/24", "not-an-ip", 5]}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&bad.json()),
        [
            field("sender_allow_cidrs.0", "invalid_format"),
            field("sender_allow_cidrs.1", "invalid_format"),
            field("sender_allow_cidrs.2", "invalid_format"),
        ]
    );

    let many: Vec<String> = (0..21).map(|index| format!("203.0.113.{index}")).collect();
    let too_many = rig.patch(&id, &json!({"sender_allow_cidrs": many})).await;
    assert_eq!(
        field_codes(&too_many.json()),
        [field("sender_allow_cidrs", "too_many")]
    );

    let not_a_list = rig.patch(&id, &json!({"sender_allow_cidrs": null})).await;
    assert_eq!(
        field_codes(&not_a_list.json()),
        [field("sender_allow_cidrs", "invalid_format")]
    );
}

#[tokio::test]
async fn a_basic_credential_is_stored_as_a_code_and_never_returned() {
    let rig = rig(true).await;
    let id = rig.endpoint().await;

    let set = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "basic", "username": USER, "password": PASSWORD}}),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK);
    assert_eq!(
        set.json()["capture_auth"],
        json!({"kind": "basic", "username": USER})
    );
    assert!(!text(&set).contains(PASSWORD));
    let read = rig.read(&id).await;
    assert!(!text(&read).contains(PASSWORD));
    assert_eq!(read.json()["capture_auth"]["kind"], "basic");

    let stored: String =
        sqlx::query_scalar("SELECT capture_auth::text FROM endpoints WHERE id = $1")
            .bind(&id)
            .fetch_one(rig.harness.db.pool())
            .await
            .expect("the document reads");
    assert!(!stored.contains(PASSWORD));

    let endpoint_id = EndpointId::parse(&id).expect("a valid id");
    let record = rig
        .harness
        .store
        .get_endpoint(&endpoint_id, RETENTION)
        .await
        .expect("the read runs")
        .expect("the endpoint is live");
    let Some(CaptureAuthRecord::Basic {
        key_id,
        username,
        password_mac,
    }) = record.access.capture_auth
    else {
        panic!("a basic record is stored");
    };
    assert_eq!(username.unpack(), USER);
    let key = rig
        .keyring
        .capture_mac_key(key_id, &endpoint_id)
        .expect("the key derives");
    assert!(key.verify(PASSWORD.as_bytes(), password_mac.unpack()));
    assert!(!key.verify(OTHER_PASSWORD.as_bytes(), password_mac.unpack()));
}

#[tokio::test]
async fn a_header_secret_replaces_the_credential_and_null_removes_it() {
    let rig = rig(true).await;
    let id = rig.endpoint().await;
    rig.patch(
        &id,
        &json!({"capture_auth": {"kind": "basic", "username": USER, "password": PASSWORD}}),
    )
    .await;

    let replaced = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "header", "name": "X-Hook-Token", "value": HEADER_VALUE}}),
        )
        .await;
    assert_eq!(
        replaced.json()["capture_auth"],
        json!({"kind": "header", "name": "x-hook-token"})
    );
    assert!(!text(&replaced).contains(HEADER_VALUE));

    let bearer = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "header", "name": "Authorization", "value": "Bearer test-token-one"}}),
        )
        .await;
    assert_eq!(bearer.status, StatusCode::OK);
    assert_eq!(
        bearer.json()["capture_auth"],
        json!({"kind": "header", "name": "authorization"})
    );

    let kept = rig.patch(&id, &json!({"title": "renamed"})).await.json();
    assert_eq!(kept["capture_auth"]["kind"], "header");

    let removed = rig.patch(&id, &json!({"capture_auth": null})).await;
    assert_eq!(removed.status, StatusCode::OK);
    assert!(removed.json()["capture_auth"].is_null());
}

#[tokio::test]
async fn a_credential_set_on_create_is_bound_to_the_new_endpoint_id() {
    let rig = rig(true).await;

    let created = rig
        .harness
        .send_as(
            Some(&rig.owner),
            Method::POST,
            "/api/v1/endpoints",
            &json!({"capture_auth": {"kind": "header", "name": "x-hook-token", "value": HEADER_VALUE}})
                .to_string(),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED);
    let id = EndpointId::parse(created.json()["id"].as_str().expect("an id")).expect("a valid id");
    let record = rig
        .harness
        .store
        .get_endpoint(&id, RETENTION)
        .await
        .expect("the read runs")
        .expect("the endpoint is live");
    let Some(CaptureAuthRecord::Header {
        key_id, value_mac, ..
    }) = record.access.capture_auth
    else {
        panic!("a header record is stored");
    };
    let key = rig
        .keyring
        .capture_mac_key(key_id, &id)
        .expect("the key derives");
    assert!(key.verify(HEADER_VALUE.as_bytes(), value_mac.unpack()));
}

#[tokio::test]
async fn invalid_credentials_are_refused_without_echoing_them() {
    let rig = rig(true).await;
    let id = rig.endpoint().await;

    let bad_basic = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "basic", "username": "a:b", "password": "bad password"}}),
        )
        .await;
    assert_eq!(
        field_codes(&bad_basic.json()),
        [
            field("capture_auth.username", "invalid_format"),
            field("capture_auth.password", "invalid_format"),
        ]
    );
    assert!(!text(&bad_basic).contains("bad password"));

    let bad_header = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "header", "name": "Host", "value": ""}}),
        )
        .await;
    assert_eq!(
        field_codes(&bad_header.json()),
        [
            field("capture_auth.name", "not_allowed"),
            field("capture_auth.value", "required"),
        ]
    );

    let too_short = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "basic", "username": USER, "password": "abc12"}}),
        )
        .await;
    assert_eq!(
        field_codes(&too_short.json()),
        [field("capture_auth.password", "invalid_format")]
    );
    assert!(!text(&too_short).contains("abc12"));

    let edge_space = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "header", "name": "x-hook-token", "value": " padded-value"}}),
        )
        .await;
    assert_eq!(
        field_codes(&edge_space.json()),
        [field("capture_auth.value", "invalid_format")]
    );

    let unknown_kind = rig
        .patch(&id, &json!({"capture_auth": {"kind": "digest"}}))
        .await;
    assert_eq!(
        field_codes(&unknown_kind.json()),
        [field("capture_auth.kind", "invalid_format")]
    );

    let stray_key = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "basic", "username": USER, "password": PASSWORD, "extra": 1}}),
        )
        .await;
    assert_eq!(
        field_codes(&stray_key.json()),
        [field("capture_auth.extra", "not_allowed")]
    );

    let read = rig.read(&id).await.json();
    assert!(read["capture_auth"].is_null());
}

#[tokio::test]
async fn setting_a_credential_needs_the_key_set_but_removing_one_does_not() {
    let rig = rig(false).await;
    let id = rig.endpoint().await;

    let refused = rig
        .patch(
            &id,
            &json!({"capture_auth": {"kind": "basic", "username": USER, "password": PASSWORD}}),
        )
        .await;
    assert_eq!(refused.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refused.json()["error"]["code"], "feature_disabled");

    let removed = rig.patch(&id, &json!({"capture_auth": null})).await;
    assert_eq!(removed.status, StatusCode::OK);
}

#[tokio::test]
async fn an_endpoint_without_an_owner_takes_neither_an_allow_list_nor_a_credential() {
    let harness = harness().await;
    let id = open_endpoint(&harness.app).await;

    let refused = send(
        &harness.app,
        request(
            Method::PATCH,
            &uri(&id),
            r#"{"capture_auth":null,"sender_allow_cidrs":[]}"#,
        ),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&refused.json()),
        [
            field("sender_allow_cidrs", "not_allowed"),
            field("capture_auth", "not_allowed"),
        ]
    );

    let created = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/endpoints",
            r#"{"capture_auth":{"kind":"basic","username":"alice","password":"test-password-one"}}"#,
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&created.json()),
        [field("capture_auth", "not_allowed")]
    );

    let renamed = send(
        &harness.app,
        request(Method::PATCH, &uri(&id), r#"{"title":"renamed"}"#),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::OK);
    let read = send(&harness.app, bodiless(Method::GET, &uri(&id)))
        .await
        .json();
    assert_eq!(read["sender_allow_cidrs"], json!([]));
}

#[tokio::test]
async fn only_the_owner_changes_who_may_send() {
    let rig = rig(false).await;
    let editor = rig
        .harness
        .actor("editor", "editor@example.test", true)
        .await;
    let id = rig.endpoint().await;
    rig.harness
        .store
        .set_member(
            &id.parse().expect("the endpoint id parses"),
            RETENTION,
            &editor.email,
            GrantedRole::Editor,
        )
        .await
        .expect("the member is added");
    let body = json!({"sender_allow_cidrs": ["203.0.113.7"]}).to_string();

    let refused = rig
        .harness
        .send_as(Some(&editor), Method::PATCH, &uri(&id), &body)
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "forbidden");

    let allowed = rig
        .harness
        .send_as(Some(&rig.owner), Method::PATCH, &uri(&id), &body)
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert_eq!(
        allowed.json()["sender_allow_cidrs"],
        json!(["203.0.113.7/32"])
    );
}
