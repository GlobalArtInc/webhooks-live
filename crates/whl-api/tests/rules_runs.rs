mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use bytes::Bytes;
use common::{api_settings, bodiless, create, request, send};
use serde_json::json;
use time::OffsetDateTime;
use whl_api::ApiState;
use whl_capture::{RulesRuntime, RulesSettings};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_testkit::TestDb;

struct Fixture {
    db: TestDb,
    store: Store,
    app: Router,
    endpoint: String,
}

async fn fixture_with(rules_enabled: bool) -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let state = ApiState::new(store.clone(), api_settings());
    let state = if rules_enabled {
        let rules = RulesRuntime::new(RulesSettings::default(), None).expect("the runtime builds");
        state.with_rules(rules)
    } else {
        state
    };
    let app = whl_api::router(state);
    let endpoint = create(&app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    Fixture {
        db,
        store,
        app,
        endpoint,
    }
}

async fn fixture() -> Fixture {
    fixture_with(true).await
}

impl Fixture {
    fn path(&self, tail: &str) -> String {
        format!("/api/v1/endpoints/{}{tail}", self.endpoint)
    }

    async fn add_rule(&self) {
        let rule = json!({"name": "after", "steps": [
            {"kind": "set_var", "into": "a", "value": "1"},
        ]});
        let reply = send(
            &self.app,
            request(Method::POST, &self.path("/rules"), &rule.to_string()),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.json());
    }

    async fn capture(&self) -> String {
        let id = CaptureId::generate().expect("an id is generated");
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: EndpointId::parse(&self.endpoint).expect("a valid endpoint id"),
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
        id.to_string()
    }

    async fn get(&self, tail: &str) -> common::Reply {
        send(&self.app, bodiless(Method::GET, &self.path(tail))).await
    }

    async fn rerun(&self, capture: &str) -> common::Reply {
        send(
            &self.app,
            bodiless(
                Method::POST,
                &self.path(&format!("/captures/{capture}/rule-runs")),
            ),
        )
        .await
    }
}

fn ids(reply: &common::Reply) -> Vec<String> {
    reply.json()["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| {
            item["capture_id"]
                .as_str()
                .expect("a capture id")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn the_capture_detail_carries_its_rule_runs() {
    let fixture = fixture().await;
    fixture.add_rule().await;
    let capture = fixture.capture().await;

    let reply = fixture.get(&format!("/captures/{capture}")).await;

    assert_eq!(reply.status, StatusCode::OK);
    let runs = reply.json()["rule_runs"].clone();
    let runs = runs.as_array().expect("rule_runs is an array");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["trigger"], "capture");
    assert_eq!(runs[0]["state"], "queued");
    assert_eq!(runs[0]["capture_id"], capture);
}

#[tokio::test]
async fn the_endpoint_run_list_pages_newest_first_and_filters_by_state() {
    let fixture = fixture().await;
    fixture.add_rule().await;
    let first = fixture.capture().await;
    fixture.capture().await;
    let newest = fixture.capture().await;

    let page = fixture.get("/rule-runs?limit=2").await;
    let cursor = page.json()["next_cursor"]
        .as_str()
        .expect("a full page has a cursor")
        .to_owned();
    let rest = fixture
        .get(&format!("/rule-runs?limit=2&cursor={cursor}"))
        .await;
    let failed = fixture.get("/rule-runs?state=failed").await;

    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(ids(&page)[0], newest);
    assert_eq!(ids(&page).len(), 2);
    assert_eq!(ids(&rest), [first]);
    assert!(rest.json()["next_cursor"].is_null());
    assert!(ids(&failed).is_empty());
}

#[tokio::test]
async fn the_run_list_refuses_a_bad_limit_state_and_cursor() {
    let fixture = fixture().await;

    let limit = fixture.get("/rule-runs?limit=0").await;
    let state = fixture.get("/rule-runs?state=lost").await;
    let cursor = fixture.get("/rule-runs?cursor=not-a-run").await;

    assert_eq!(limit.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(state.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cursor.status, StatusCode::BAD_REQUEST);
    assert_eq!(cursor.json()["error"]["code"], "invalid_cursor");
}

#[tokio::test]
async fn the_capture_run_route_lists_that_capture_only_and_refuses_an_unknown_one() {
    let fixture = fixture().await;
    fixture.add_rule().await;
    let first = fixture.capture().await;
    fixture.capture().await;
    let unknown = CaptureId::generate()
        .expect("an id is generated")
        .to_string();

    let listed = fixture.get(&format!("/captures/{first}/rule-runs")).await;
    let missing = fixture.get(&format!("/captures/{unknown}/rule-runs")).await;

    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(ids(&listed), [first]);
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json()["error"]["code"], "capture_not_found");
}

#[tokio::test]
async fn a_rerun_is_accepted_queued_and_refused_while_it_is_active() {
    let fixture = fixture().await;
    let capture = fixture.capture().await;

    let without_rules = fixture.rerun(&capture).await;
    fixture.add_rule().await;
    let accepted = fixture.rerun(&capture).await;
    let again = fixture.rerun(&capture).await;

    assert_eq!(without_rules.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(accepted.status, StatusCode::ACCEPTED, "{}", accepted.json());
    let run = accepted.json();
    assert_eq!(run["trigger"], "rerun");
    assert_eq!(run["state"], "queued");
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.json()["error"]["code"], "rerun_active");
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE trigger = 'rerun'")
        .fetch_one(fixture.db.pool())
        .await
        .expect("the reruns are countable");
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn a_rerun_is_unavailable_when_the_rules_engine_is_switched_off() {
    let fixture = fixture_with(false).await;
    let capture = fixture.capture().await;

    let reply = fixture.rerun(&capture).await;

    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.json()["error"]["code"], "feature_disabled");
}
