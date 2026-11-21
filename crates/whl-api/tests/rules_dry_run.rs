mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use bytes::Bytes;
use common::{api_settings, create, field_codes, request, send};
use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_api::ApiState;
use whl_capture::{RulesRuntime, RulesSettings};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_testkit::TestDb;

struct Fixture {
    db: TestDb,
    app: Router,
    endpoint: String,
    capture: String,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let rules = RulesRuntime::new(RulesSettings::default(), None).expect("the runtime builds");
    let app = whl_api::router(ApiState::new(store.clone(), api_settings()).with_rules(rules));
    let endpoint = create(&app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    let id = CaptureId::generate().expect("an id is generated");
    let outcome = store
        .store_capture(
            NewCapture {
                id,
                endpoint_id: EndpointId::parse(&endpoint).expect("a valid endpoint id"),
                received_at: OffsetDateTime::now_utc(),
                method: "POST".to_owned(),
                url: "http://h/x".to_owned(),
                path: "/x".to_owned(),
                query_string: String::new(),
                host: "h".to_owned(),
                client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                headers: vec![NameValue {
                    name: "X-Token".to_owned(),
                    value: "abc".to_owned(),
                }],
                query: Vec::new(),
                form: None,
                content_type: Some("text/plain".to_owned()),
                body: Bytes::from_static(b"hello"),
                status_override: None,
            },
            CapturePolicy {
                max_captures: 500,
                retention: Duration::from_secs(3600),
            },
        )
        .await
        .expect("the capture is stored");
    assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
    Fixture {
        db,
        app,
        endpoint,
        capture: id.to_string(),
    }
}

impl Fixture {
    async fn add_rule(&self, rule: Value) {
        let reply = send(
            &self.app,
            request(
                Method::POST,
                &format!("/api/v1/endpoints/{}/rules", self.endpoint),
                &rule.to_string(),
            ),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.json());
    }

    async fn dry_run(&self, body: Value) -> common::Reply {
        send(
            &self.app,
            request(
                Method::POST,
                &format!("/api/v1/endpoints/{}/rules/dry-run", self.endpoint),
                &body.to_string(),
            ),
        )
        .await
    }
}

#[tokio::test]
async fn a_dry_run_of_the_saved_rules_reports_the_steps_and_writes_nothing() {
    let fixture = fixture().await;
    fixture
        .add_rule(json!({
            "name": "answer",
            "condition": {"value": "request.header.x-token", "op": "eq", "arg": "abc"},
            "steps": [
                {"kind": "increment", "counter": "hits", "by": 2},
                {"kind": "notify", "channel_id": "0190c3a2-7d1e-7000-8000-000000000001"},
                {"kind": "respond", "status": "202", "body": "{{ counter.hits }}"},
            ],
        }))
        .await;

    let reply = fixture
        .dry_run(json!({"capture_id": fixture.capture}))
        .await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    let run = reply.json();
    assert_eq!(run["trigger"], "dry_run");
    assert_eq!(run["state"], "succeeded");
    assert_eq!(run["capture_id"], fixture.capture);
    assert_eq!(run["rules_version"], 1);
    assert_eq!(run["response_status"], 202);
    let entries = run["entries"].as_array().expect("entries are an array");
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0]["matched"], true);
    assert_eq!(entries[1]["outcome"], "no_op");
    assert_eq!(entries[2]["outcome"], "deferred");
    assert_eq!(entries[3]["outcome"], "ok");
    let written: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoint_vars")
        .fetch_one(fixture.db.pool())
        .await
        .expect("the variables are countable");
    assert_eq!(written, 0);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE trigger = 'dry_run'")
            .fetch_one(fixture.db.pool())
            .await
            .expect("the runs are countable");
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn unsaved_draft_rules_are_validated_and_run_instead_of_the_saved_ones() {
    let fixture = fixture().await;

    let reply = fixture
        .dry_run(json!({
            "capture_id": fixture.capture,
            "rules": [{
                "name": "draft",
                "steps": [{"kind": "respond", "status": "201", "body": "{{ request.method }}"}],
            }],
        }))
        .await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    assert_eq!(reply.json()["response_status"], 201);
}

#[tokio::test]
async fn an_invalid_draft_reports_its_field_path_and_code() {
    let fixture = fixture().await;

    let reply = fixture
        .dry_run(json!({
            "capture_id": fixture.capture,
            "rules": [{"name": "draft", "steps": [{"kind": "teleport"}]}],
        }))
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![(
            "rules.0.steps.0.kind".to_owned(),
            "invalid_format".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_dry_run_against_an_unknown_capture_is_not_found() {
    let fixture = fixture().await;
    let stranger = CaptureId::generate()
        .expect("an id is generated")
        .to_string();

    let reply = fixture.dry_run(json!({"capture_id": stranger})).await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_dry_run_request_must_name_a_capture_and_only_known_fields() {
    let fixture = fixture().await;

    let missing = fixture.dry_run(json!({})).await;
    let unknown = fixture
        .dry_run(json!({"capture_id": fixture.capture, "extra": 1}))
        .await;

    assert_eq!(
        field_codes(&missing.json()),
        vec![("capture_id".to_owned(), "required".to_owned())]
    );
    assert_eq!(
        field_codes(&unknown.json()),
        vec![("extra".to_owned(), "not_allowed".to_owned())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_dry_runs_of_one_endpoint_are_limited_to_one_at_a_time() {
    let fixture = fixture().await;
    let heavy = "{{ request.body | base64 | hex | sha256 | hex | base64 | hex }}".repeat(400);
    fixture
        .add_rule(json!({
            "name": "heavy",
            "steps": [{"kind": "respond", "status": "200", "body": heavy}],
        }))
        .await;

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let app = fixture.app.clone();
        let uri = format!("/api/v1/endpoints/{}/rules/dry-run", fixture.endpoint);
        let body = json!({"capture_id": fixture.capture}).to_string();
        tasks.spawn(async move { send(&app, request(Method::POST, &uri, &body)).await.status });
    }
    let mut statuses = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        statuses.push(joined.expect("the request task finishes"));
    }

    assert!(statuses.contains(&StatusCode::OK), "{statuses:?}");
    assert!(
        statuses.contains(&StatusCode::TOO_MANY_REQUESTS),
        "{statuses:?}"
    );
}
