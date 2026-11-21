mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use common::{Actor, Reply, SsoHarness, api_settings, field_codes, mixed_harness, request, send};
use serde_json::json;
use whl_api::{ApiState, ScheduleSettings};
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::schedules::{CreateScheduleOutcome, NewSchedule, ScheduleHeader, ScheduleRequest};

const KEYRING: &str = "1:a2tra2tra2tra2tra2tra2tra2tra2tra2tra2tra2s=";

fn app(harness: &SsoHarness, keyring: Option<Arc<Keyring>>) -> Router {
    let state = ApiState::new(harness.store.clone(), api_settings())
        .with_access(harness.access.clone())
        .with_schedules(ScheduleSettings::new(5, Duration::from_secs(300)));
    let state = match keyring {
        Some(keyring) => state.with_secrets(keyring),
        None => state,
    };
    whl_api::router(state)
}

async fn call(app: &Router, actor: &Actor, method: Method, uri: &str, body: &str) -> Reply {
    let mut built = request(method, uri, body);
    built.headers_mut().insert(
        header::COOKIE,
        actor.cookie.parse().expect("a valid cookie header"),
    );
    send(app, built).await
}

fn schedules_uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/schedules")
}

fn create_body(url: &str) -> String {
    json!({
        "name": "probe",
        "cron": "*/5 * * * *",
        "request": { "method": "GET", "url": url }
    })
    .to_string()
}

async fn add_editor(harness: &SsoHarness, owner: &Actor, endpoint: &str) -> Actor {
    let editor = harness.actor("editor", "editor@example.test", true).await;
    let reply = harness
        .send_as(
            Some(owner),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            &json!({ "email": editor.email, "role": "editor" }).to_string(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    editor
}

async fn seed(harness: &SsoHarness, owner: &Actor, endpoint: &str, url: &str) -> String {
    let id = whl_core::endpoint_id::EndpointId::parse(endpoint).expect("a valid id");
    let outcome = harness
        .store
        .create_schedule(
            &id,
            common::RETENTION,
            &NewSchedule {
                created_by: owner.id,
                name: "seeded".to_owned(),
                cron: "*/5 * * * *".to_owned(),
                timezone: "UTC".to_owned(),
                request: ScheduleRequest {
                    method: "GET".to_owned(),
                    url: url.to_owned(),
                    headers: vec![ScheduleHeader {
                        name: "X-One".to_owned(),
                        value: "one".to_owned(),
                    }],
                    body: None,
                },
                enabled: true,
                next_run_at: None,
            },
            5,
        )
        .await
        .expect("the schedule is stored");
    match outcome {
        CreateScheduleOutcome::Created(record) => record.id.hyphenated().to_string(),
        other => panic!("the schedule was not created: {other:?}"),
    }
}

#[tokio::test]
async fn the_sixth_schedule_is_refused_with_the_limit() {
    let harness = mixed_harness().await;
    let app = app(&harness, None);
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    for _ in 0..5 {
        let reply = call(
            &app,
            &owner,
            Method::POST,
            &schedules_uri(&endpoint),
            &create_body("https://8.8.8.8/health"),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    }
    let reply = call(
        &app,
        &owner,
        Method::POST,
        &schedules_uri(&endpoint),
        &create_body("https://8.8.8.8/health"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "limit_reached");
    assert_eq!(error["error"]["limit"]["name"], "schedules");
    assert_eq!(error["error"]["limit"]["max"], 5);
}

#[tokio::test]
async fn a_caller_with_no_role_gets_gone() {
    let harness = mixed_harness().await;
    let app = app(&harness, None);
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let stranger = harness
        .actor("stranger", "stranger@example.test", true)
        .await;
    let endpoint = harness.endpoint_for(&owner).await;

    let reply = call(&app, &stranger, Method::GET, &schedules_uri(&endpoint), "").await;
    assert_eq!(reply.status, StatusCode::GONE);
    assert_eq!(reply.json()["error"]["code"], "endpoint_gone");
}

#[tokio::test]
async fn invalid_fields_are_reported_with_their_codes() {
    let harness = mixed_harness().await;
    let app = app(&harness, None);
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let cases = [
        (
            json!({"name": "p", "cron": "61 * * * *", "request": {"method": "GET", "url": "https://8.8.8.8/"}}),
            ("cron", "cron_invalid"),
        ),
        (
            json!({"name": "p", "cron": "* * * * *", "request": {"method": "GET", "url": "https://8.8.8.8/"}}),
            ("cron", "interval_too_short"),
        ),
        (
            json!({"name": "p", "cron": "*/5 * * * *", "timezone": "Mars/Base", "request": {"method": "GET", "url": "https://8.8.8.8/"}}),
            ("timezone", "timezone_invalid"),
        ),
        (
            json!({"name": "p", "cron": "*/5 * * * *", "request": {"method": "GET", "url": "http://127.0.0.1/"}}),
            ("request.url", "target_refused"),
        ),
        (
            json!({"name": "p", "cron": "*/5 * * * *", "request": {"method": "TRACE", "url": "https://8.8.8.8/"}}),
            ("request.method", "invalid_format"),
        ),
        (
            json!({"name": "p", "cron": "*/5 * * * *", "request": {"method": "GET", "url": "https://8.8.8.8/?k={{ secret.K | upper }}"}}),
            ("request.url", "secret_not_allowed"),
        ),
    ];
    for (body, (field, code)) in cases {
        let reply = call(
            &app,
            &owner,
            Method::POST,
            &schedules_uri(&endpoint),
            &body.to_string(),
        )
        .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        let found = field_codes(&reply.json());
        assert!(
            found.contains(&(field.to_owned(), code.to_owned())),
            "{body}: {found:?}"
        );
    }
}

#[tokio::test]
async fn an_editor_cannot_edit_a_request_that_already_holds_a_secret_reference() {
    let harness = mixed_harness().await;
    let app = app(&harness, None);
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let editor = add_editor(&harness, &owner, &endpoint).await;
    let id = seed(
        &harness,
        &owner,
        &endpoint,
        "https://8.8.8.8/?k={{ secret.K }}",
    )
    .await;
    let uri = format!("{}/{id}", schedules_uri(&endpoint));
    let moved = json!({"request": {"method": "GET", "url": "https://1.1.1.1/"}}).to_string();

    let refused = call(&app, &editor, Method::PATCH, &uri, &moved).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "owner_required");

    let renamed = call(
        &app,
        &editor,
        Method::PATCH,
        &uri,
        &json!({"name": "renamed"}).to_string(),
    )
    .await;
    assert_eq!(renamed.status, StatusCode::OK);
}

#[tokio::test]
async fn an_editor_cannot_add_a_secret_reference_to_a_request_but_the_owner_can() {
    let harness = mixed_harness().await;
    let keyring = Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"));
    let app = app(&harness, Some(Arc::clone(&keyring)));
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let editor = add_editor(&harness, &owner, &endpoint).await;
    let id = whl_core::endpoint_id::EndpointId::parse(&endpoint).expect("a valid id");
    let name = SecretName::parse("K").expect("a valid name");
    let sealed = keyring
        .seal(
            &id,
            &name,
            &Secret::new("a-long-secret-value".to_owned()).expect("a valid secret"),
        )
        .expect("the secret seals");
    harness
        .store
        .put_secret(&id, common::RETENTION, &name, &sealed, 20)
        .await
        .expect("the secret is stored");
    let created = call(
        &app,
        &owner,
        Method::POST,
        &schedules_uri(&endpoint),
        &create_body("https://8.8.8.8/"),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let schedule = created.json()["id"].as_str().expect("an id").to_owned();
    let uri = format!("{}/{schedule}", schedules_uri(&endpoint));
    let leaking = json!({"request": {"method": "GET", "url": "https://8.8.8.8/?k={{ secret.K }}"}})
        .to_string();

    let refused = call(&app, &editor, Method::PATCH, &uri, &leaking).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "owner_required");

    let allowed = call(&app, &owner, Method::PATCH, &uri, &leaking).await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert!(!allowed.json().to_string().contains("a-long-secret-value"));
}
