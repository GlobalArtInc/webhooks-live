mod common;

use std::io;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{Method, StatusCode};
use common::matrix::cast;
use common::{RETENTION, Reply, SsoHarness, UNKNOWN_ID, api_settings, bodiless, request, send};
use serde_json::json;
use tracing_subscriber::fmt::MakeWriter;
use whl_api::ApiState;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, MAX_SECRETS_PER_ENDPOINT, SecretName};
use whl_store::{GrantedRole, Store};
use whl_testkit::TestDb;

const KEYS: &str =
    "2:AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=,1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const VALUE: &str = "sk_live_51Hq-super-secret-value";

struct Rig {
    db: TestDb,
    store: Store,
    app: Router,
    keyring: Arc<Keyring>,
}

fn keyring() -> Arc<Keyring> {
    Arc::new(Keyring::parse(KEYS).expect("the key set is valid"))
}

async fn rig() -> Rig {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let keyring = keyring();
    let state = ApiState::new(store.clone(), api_settings()).with_secrets(Arc::clone(&keyring));
    Rig {
        db,
        store,
        app: whl_api::router(state),
        keyring,
    }
}

async fn endpoint(app: &Router) -> String {
    common::create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

fn secrets_uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/secrets")
}

fn secret_uri(endpoint: &str, name: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/secrets/{name}")
}

async fn put(app: &Router, endpoint: &str, name: &str, value: &str) -> Reply {
    let body = json!({ "value": value }).to_string();
    send(
        app,
        request(Method::PUT, &secret_uri(endpoint, name), &body),
    )
    .await
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn assert_no_value(reply: &Reply) {
    let text = String::from_utf8_lossy(&reply.body);
    assert!(!text.contains(VALUE), "{text}");
}

#[tokio::test]
async fn a_secret_is_stored_encrypted_and_never_returned() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;

    let stored = put(&rig.app, &endpoint, "API_KEY", VALUE).await;

    assert_eq!(stored.status, StatusCode::OK);
    let body = stored.json();
    assert_eq!(body["name"], "API_KEY");
    assert!(body["created_at"].is_string() && body["updated_at"].is_string());
    assert_eq!(body.as_object().expect("an object").len(), 3);
    assert_no_value(&stored);

    let listed = send(&rig.app, bodiless(Method::GET, &secrets_uri(&endpoint))).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["items"].as_array().expect("items").len(), 1);
    assert_eq!(listed.json()["items"][0]["name"], "API_KEY");
    assert_eq!(
        listed.json()["items"][0]
            .as_object()
            .expect("an object")
            .len(),
        3
    );
    assert_no_value(&listed);

    let (key_id, ciphertext): (i16, Vec<u8>) = sqlx::query_as(
        "SELECT key_id, ciphertext FROM endpoint_secrets WHERE endpoint_id = $1 AND name = 'API_KEY'",
    )
    .bind(&endpoint)
    .fetch_one(rig.db.pool())
    .await
    .expect("the row exists");
    assert_eq!(key_id, 2, "writes use the first key of the set");
    assert!(
        !ciphertext
            .windows(VALUE.len())
            .any(|window| window == VALUE.as_bytes())
    );
    let sealed = rig
        .store
        .get_sealed_secret(
            &EndpointId::parse(&endpoint).expect("an id"),
            &SecretName::parse("API_KEY").expect("a name"),
        )
        .await
        .expect("reads")
        .expect("the row exists");
    let opened = rig
        .keyring
        .open(
            &EndpointId::parse(&endpoint).expect("an id"),
            &SecretName::parse("API_KEY").expect("a name"),
            &sealed,
        )
        .expect("opens");
    assert_eq!(opened.unpack(), VALUE);
}

#[tokio::test]
async fn putting_again_replaces_the_value_and_keeps_the_creation_time() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    let first = put(&rig.app, &endpoint, "API_KEY", VALUE).await.json();

    let second = put(&rig.app, &endpoint, "API_KEY", "another-value-9").await;

    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(second.json()["created_at"], first["created_at"]);
    let listed = send(&rig.app, bodiless(Method::GET, &secrets_uri(&endpoint))).await;
    assert_eq!(listed.json()["items"].as_array().expect("items").len(), 1);
}

#[tokio::test]
async fn deleting_removes_the_secret_and_a_second_delete_is_not_found() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    put(&rig.app, &endpoint, "API_KEY", VALUE).await;

    let deleted = send(
        &rig.app,
        bodiless(Method::DELETE, &secret_uri(&endpoint, "API_KEY")),
    )
    .await;
    let again = send(
        &rig.app,
        bodiless(Method::DELETE, &secret_uri(&endpoint, "API_KEY")),
    )
    .await;
    let bad_name = send(
        &rig.app,
        bodiless(Method::DELETE, &secret_uri(&endpoint, "api-key")),
    )
    .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(again.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&again), "secret_not_found");
    assert_eq!(bad_name.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&bad_name), "secret_not_found");
    let listed = send(&rig.app, bodiless(Method::GET, &secrets_uri(&endpoint))).await;
    assert!(listed.json()["items"].as_array().expect("items").is_empty());
}

#[tokio::test]
async fn the_name_and_the_value_are_validated_and_errors_never_echo_the_value() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    let largest = "x".repeat(4096);
    let too_long = format!("{VALUE}{}", "y".repeat(4096));

    let bad_name = put(&rig.app, &endpoint, "api_key", VALUE).await;
    let empty = put(&rig.app, &endpoint, "API_KEY", "").await;
    let long = put(&rig.app, &endpoint, "API_KEY", &too_long).await;
    let largest = put(&rig.app, &endpoint, "API_KEY", &largest).await;
    let not_text = send(
        &rig.app,
        request(
            Method::PUT,
            &secret_uri(&endpoint, "API_KEY"),
            &json!({ "value": 12345 }).to_string(),
        ),
    )
    .await;
    let missing = send(
        &rig.app,
        request(
            Method::PUT,
            &secret_uri(&endpoint, "API_KEY"),
            &json!({ "valeu": VALUE }).to_string(),
        ),
    )
    .await;
    let broken = send(
        &rig.app,
        request(
            Method::PUT,
            &secret_uri(&endpoint, "API_KEY"),
            &format!("{{\"value\": \"{VALUE}"),
        ),
    )
    .await;

    assert_eq!(bad_name.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        common::field_codes(&bad_name.json()),
        [("name".to_owned(), "invalid_format".to_owned())]
    );
    assert_eq!(
        common::field_codes(&empty.json()),
        [("value".to_owned(), "required".to_owned())]
    );
    assert_eq!(
        common::field_codes(&long.json()),
        [("value".to_owned(), "too_long".to_owned())]
    );
    assert_eq!(largest.status, StatusCode::OK);
    assert_eq!(
        common::field_codes(&not_text.json()),
        [("value".to_owned(), "invalid_format".to_owned())]
    );
    assert_eq!(
        common::field_codes(&missing.json()),
        [
            ("valeu".to_owned(), "not_allowed".to_owned()),
            ("value".to_owned(), "required".to_owned())
        ]
    );
    assert_eq!(broken.status, StatusCode::BAD_REQUEST);
    for reply in [&bad_name, &empty, &long, &not_text, &missing, &broken] {
        assert_no_value(reply);
    }
}

#[tokio::test]
async fn an_endpoint_holds_at_most_twenty_secrets_and_replacing_one_at_the_limit_still_works() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    for index in 0..MAX_SECRETS_PER_ENDPOINT {
        let stored = put(&rig.app, &endpoint, &format!("KEY_{index}"), VALUE).await;
        assert_eq!(stored.status, StatusCode::OK, "{index}");
    }

    let over = put(&rig.app, &endpoint, "ONE_TOO_MANY", VALUE).await;
    let replaced = put(&rig.app, &endpoint, "KEY_0", "new-value-1").await;
    let other = common::create(&rig.app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let elsewhere = put(&rig.app, &other, "ONE_TOO_MANY", VALUE).await;

    assert_eq!(over.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&over), "limit_reached");
    assert_eq!(over.json()["error"]["limit"]["name"], "secrets");
    assert_eq!(over.json()["error"]["limit"]["max"], 20);
    assert_eq!(replaced.status, StatusCode::OK);
    assert_eq!(elsewhere.status, StatusCode::OK);
}

#[tokio::test]
async fn an_unknown_or_deleted_endpoint_is_gone_and_a_malformed_id_is_not_found() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    send(
        &rig.app,
        bodiless(Method::DELETE, &format!("/api/v1/endpoints/{endpoint}")),
    )
    .await;

    for id in [UNKNOWN_ID, endpoint.as_str()] {
        let stored = put(&rig.app, id, "API_KEY", VALUE).await;
        let listed = send(&rig.app, bodiless(Method::GET, &secrets_uri(id))).await;
        let deleted = send(
            &rig.app,
            bodiless(Method::DELETE, &secret_uri(id, "API_KEY")),
        )
        .await;
        for reply in [stored, listed, deleted] {
            assert_eq!(reply.status, StatusCode::GONE, "{id}");
            assert_eq!(code(&reply), "endpoint_gone");
        }
    }
    let malformed = put(&rig.app, "not-an-id", "API_KEY", VALUE).await;
    assert_eq!(malformed.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn without_a_key_set_the_routes_answer_feature_disabled_and_the_instance_says_so() {
    let harness = common::harness().await;
    let endpoint = endpoint(&harness.app).await;

    let stored = put(&harness.app, &endpoint, "API_KEY", VALUE).await;
    let listed = send(&harness.app, bodiless(Method::GET, &secrets_uri(&endpoint))).await;
    let deleted = send(
        &harness.app,
        bodiless(Method::DELETE, &secret_uri(&endpoint, "API_KEY")),
    )
    .await;
    let instance = send(&harness.app, bodiless(Method::GET, "/api/v1/instance")).await;
    let enabled = rig().await;
    let enabled_instance = send(&enabled.app, bodiless(Method::GET, "/api/v1/instance")).await;

    for reply in [&stored, &listed, &deleted] {
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code(reply), "feature_disabled");
        assert!(reply.headers.get("retry-after").is_none());
        assert_no_value(reply);
    }
    assert_eq!(instance.json()["features"]["secrets"], false);
    assert_eq!(enabled_instance.json()["features"]["secrets"], true);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoint_secrets")
        .fetch_one(harness.db.pool())
        .await
        .expect("counts");
    assert_eq!(rows, 0);
}

async fn sso_rig() -> (SsoHarness, String, [common::Actor; 4]) {
    let mut harness = common::sso_harness().await;
    harness.app = whl_api::router(
        ApiState::new(harness.store.clone(), api_settings())
            .with_access(harness.access.clone())
            .with_secrets(keyring()),
    );
    let cast = cast(&harness).await;
    let endpoint = harness.endpoint_for(&cast.owner).await;
    let id = EndpointId::parse(&endpoint).expect("an id");
    for (actor, role) in [
        (&cast.editor, GrantedRole::Editor),
        (&cast.viewer, GrantedRole::Viewer),
    ] {
        harness
            .store
            .set_member(&id, RETENTION, &actor.email, role)
            .await
            .expect("the member is stored");
    }
    (
        harness,
        endpoint,
        [cast.owner, cast.editor, cast.viewer, cast.stranger],
    )
}

#[tokio::test]
async fn the_secret_routes_need_an_editor_and_hide_the_endpoint_from_outsiders() {
    let (harness, endpoint, [owner, editor, viewer, stranger]) = sso_rig().await;
    let body = json!({ "value": VALUE }).to_string();
    let routes = [
        (Method::GET, secrets_uri(&endpoint), String::new()),
        (Method::PUT, secret_uri(&endpoint, "API_KEY"), body),
        (
            Method::DELETE,
            secret_uri(&endpoint, "API_KEY"),
            String::new(),
        ),
    ];

    for (method, uri, body) in &routes {
        let label = format!("{method} {uri}");
        let anonymous = harness.send_as(None, method.clone(), uri, body).await;
        let outsider = harness
            .send_as(Some(&stranger), method.clone(), uri, body)
            .await;
        let reader = harness
            .send_as(Some(&viewer), method.clone(), uri, body)
            .await;
        assert_eq!(anonymous.status, StatusCode::GONE, "{label}");
        assert_eq!(outsider.status, StatusCode::GONE, "{label}");
        assert_eq!(reader.status, StatusCode::FORBIDDEN, "{label}");
        assert_eq!(code(&reader), "forbidden", "{label}");
    }
    let rows = |harness: &SsoHarness| {
        let pool = harness.db.pool().clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM endpoint_secrets")
                .fetch_one(&pool)
                .await
                .expect("counts")
        }
    };
    assert_eq!(rows(&harness).await, 0, "refused callers stored nothing");
    for actor in [&editor, &owner] {
        let stored = harness
            .send_as(
                Some(actor),
                Method::PUT,
                &secret_uri(&endpoint, "API_KEY"),
                &json!({ "value": VALUE }).to_string(),
            )
            .await;
        let listed = harness
            .send_as(Some(actor), Method::GET, &secrets_uri(&endpoint), "")
            .await;
        let deleted = harness
            .send_as(
                Some(actor),
                Method::DELETE,
                &secret_uri(&endpoint, "API_KEY"),
                "",
            )
            .await;
        assert_eq!(stored.status, StatusCode::OK);
        assert_eq!(listed.status, StatusCode::OK);
        assert_eq!(deleted.status, StatusCode::NO_CONTENT);
        assert_no_value(&listed);
    }
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the buffer is readable")).into_owned()
    }
}

#[tokio::test]
async fn neither_tracing_output_nor_error_bodies_contain_the_value() {
    let rig = rig().await;
    let endpoint = endpoint(&rig.app).await;
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let stored = put(&rig.app, &endpoint, "API_KEY", VALUE).await;
    let invalid = put(
        &rig.app,
        &endpoint,
        "API_KEY",
        &format!("{VALUE}{}", "z".repeat(5000)),
    )
    .await;
    sqlx::query("DROP TABLE endpoint_secrets")
        .execute(rig.db.pool())
        .await
        .expect("the table is dropped");
    let broken = put(&rig.app, &endpoint, "API_KEY", VALUE).await;
    let broken_list = send(&rig.app, bodiless(Method::GET, &secrets_uri(&endpoint))).await;

    assert_eq!(stored.status, StatusCode::OK);
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(broken.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(broken_list.status, StatusCode::INTERNAL_SERVER_ERROR);
    let logs = captured.text();
    assert!(logs.contains("secret stored"), "the capture works: {logs}");
    assert!(logs.contains("secret storage failed"), "{logs}");
    assert!(!logs.contains(VALUE), "{logs}");
    assert!(!logs.contains("sk_live"), "{logs}");
    for reply in [&stored, &invalid, &broken, &broken_list] {
        assert_no_value(reply);
    }
}
