mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{RETENTION, Reply, api_settings, bodiless, field_codes, request, send};
use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_api::ApiState;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::Keyring;
use whl_store::Store;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_testkit::TestDb;
use whl_verify::Verification;

const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

struct Rig {
    db: TestDb,
    store: Store,
    app: Router,
}

async fn rig(with_secrets: bool) -> Rig {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let mut state = ApiState::new(store.clone(), api_settings());
    if with_secrets {
        state = state.with_secrets(Arc::new(Keyring::parse(KEYS).expect("keys parse")));
    }
    Rig {
        db,
        store,
        app: whl_api::router(state),
    }
}

async fn endpoint(app: &Router) -> String {
    common::create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

async fn put_secret(app: &Router, endpoint: &str, name: &str) {
    let reply = send(
        app,
        request(
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/secrets/{name}"),
            r#"{"value":"shared-secret"}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
}

async fn patch(app: &Router, endpoint: &str, body: Value) -> Reply {
    send(
        app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}"),
            &body.to_string(),
        ),
    )
    .await
}

fn github(secret_name: &str) -> Value {
    json!({ "verifier": { "scheme": "github", "secret_name": secret_name } })
}

async fn delete_secret(app: &Router, endpoint: &str, name: &str) -> Reply {
    send(
        app,
        bodiless(
            Method::DELETE,
            &format!("/api/v1/endpoints/{endpoint}/secrets/{name}"),
        ),
    )
    .await
}

#[tokio::test]
async fn a_preset_verifier_is_stored_resolved_and_read_back() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    put_secret(&rig.app, &endpoint, "HOOK").await;

    let reply = patch(&rig.app, &endpoint, github("HOOK")).await;

    assert_eq!(reply.status, StatusCode::OK);
    let expected = json!({
        "scheme": "github",
        "secret_name": "HOOK",
        "header": "x-hub-signature-256",
        "algorithm": "sha256",
        "encoding": "hex",
        "prefix": "sha256=",
        "timestamp_header": null,
    });
    assert_eq!(reply.json()["verifier"], expected);
    let shown = send(
        &rig.app,
        bodiless(Method::GET, &format!("/api/v1/endpoints/{endpoint}")),
    )
    .await;
    assert_eq!(shown.json()["verifier"], expected);
}

#[tokio::test]
async fn a_verifier_naming_an_unknown_secret_is_refused() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;

    let reply = patch(&rig.app, &endpoint, github("MISSING")).await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![(
            "verifier.secret_name".to_owned(),
            "secret_unknown".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_generic_verifier_without_its_fields_is_refused_per_field() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    put_secret(&rig.app, &endpoint, "HOOK").await;

    let reply = patch(
        &rig.app,
        &endpoint,
        json!({ "verifier": { "scheme": "hmac", "secret_name": "HOOK", "algorithm": "md5" } }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![
            ("verifier.algorithm".to_owned(), "invalid_format".to_owned()),
            ("verifier.header".to_owned(), "required".to_owned()),
            ("verifier.encoding".to_owned(), "required".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_verifier_cannot_be_set_when_creating_the_endpoint() {
    let rig = rig(true).await;

    let reply = send(
        &rig.app,
        request(
            Method::POST,
            "/api/v1/endpoints",
            &github("HOOK").to_string(),
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![(
            "verifier.secret_name".to_owned(),
            "secret_unknown".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_verifier_needs_the_secrets_keyring() {
    let rig = rig(false).await;
    let endpoint = endpoint(&rig.app).await;

    let reply = patch(&rig.app, &endpoint, github("HOOK")).await;

    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.json()["error"]["code"], "feature_disabled");
}

#[tokio::test]
async fn null_removes_the_verifier() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    put_secret(&rig.app, &endpoint, "HOOK").await;
    patch(&rig.app, &endpoint, github("HOOK")).await;

    let reply = patch(&rig.app, &endpoint, json!({ "verifier": null })).await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.json()["verifier"].is_null());
}

#[tokio::test]
async fn a_secret_used_by_the_verifier_cannot_be_deleted() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    put_secret(&rig.app, &endpoint, "HOOK").await;
    patch(&rig.app, &endpoint, github("HOOK")).await;

    let refused = delete_secret(&rig.app, &endpoint, "HOOK").await;

    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.json()["error"]["code"], "secret_in_use");
    assert_eq!(
        refused.json()["error"]["references"],
        json!([{ "kind": "verifier", "id": endpoint }])
    );
    patch(&rig.app, &endpoint, json!({ "verifier": null })).await;
    let allowed = delete_secret(&rig.app, &endpoint, "HOOK").await;
    assert_eq!(allowed.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn capture_summaries_carry_the_verification() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    let capture = CaptureId::generate().expect("an id is generated");
    let outcome = rig
        .store
        .store_verified_capture(
            NewCapture {
                id: capture,
                endpoint_id: EndpointId::parse(&endpoint).expect("a valid endpoint id"),
                received_at: OffsetDateTime::now_utc(),
                method: "POST".to_owned(),
                url: "http://h/x".to_owned(),
                path: "/x".to_owned(),
                query_string: String::new(),
                host: "h".to_owned(),
                client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                headers: Vec::new(),
                query: Vec::new(),
                form: None,
                content_type: None,
                body: Default::default(),
                status_override: None,
            },
            Some(Verification::Invalid),
            CapturePolicy {
                max_captures: 500,
                retention: RETENTION,
            },
        )
        .await
        .expect("the capture is stored");
    assert!(matches!(outcome, CaptureOutcome::Stored { .. }));

    let listed = send(
        &rig.app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/captures"),
        ),
    )
    .await;

    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["items"][0]["verification"], "invalid");
}

#[tokio::test]
async fn an_unreadable_stored_verifier_never_blocks_reading_or_clearing_it() {
    let rig = rig(true).await;
    let endpoint = endpoint(&rig.app).await;
    sqlx::query(
        "UPDATE endpoints SET verifier = \'{\"scheme\":\"future\",\"secret_name\":\"HOOK\",\"extra\":1}\'::jsonb \
         WHERE id = $1",
    )
    .bind(&endpoint)
    .execute(rig.db.pool())
    .await
    .expect("verifier rewrites");

    let shown = send(
        &rig.app,
        bodiless(Method::GET, &format!("/api/v1/endpoints/{endpoint}")),
    )
    .await;
    assert_eq!(shown.status, StatusCode::OK);
    assert_eq!(shown.json()["verifier"]["scheme"], "future");
    let renamed = patch(&rig.app, &endpoint, json!({ "title": "still editable" })).await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.json()["verifier"]["scheme"], "future");
    let cleared = patch(&rig.app, &endpoint, json!({ "verifier": null })).await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert!(cleared.json()["verifier"].is_null());
}
