mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use bytes::Bytes;
use common::matrix::{cast, store_capture};
use common::tokens::mint;
use common::{
    RETENTION, api_settings, create, field_codes, mixed_harness, request, send, sso_harness,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_api::ApiState;
use whl_auth::{GRANT_HEADER, ViewGrant};
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

fn pair(name: &str, value: &str) -> NameValue {
    NameValue {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

async fn stored_capture(store: &Store, endpoint: &str) -> String {
    let id = CaptureId::generate().expect("an id is generated");
    let outcome = store
        .store_capture(
            NewCapture {
                id,
                endpoint_id: EndpointId::parse(endpoint).expect("a valid endpoint id"),
                received_at: OffsetDateTime::now_utc(),
                method: "POST".to_owned(),
                url: "http://h/hook/orders?mode=test".to_owned(),
                path: "/hook/orders".to_owned(),
                query_string: "mode=test".to_owned(),
                host: "h".to_owned(),
                client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                headers: vec![pair("X-Token", "abc")],
                query: vec![pair("mode", "test")],
                form: None,
                content_type: Some("application/json".to_owned()),
                body: Bytes::from_static(br#"{"id":"evt_1","data":{"amount":2000}}"#),
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

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let app = whl_api::router(ApiState::new(store.clone(), api_settings()));
    let endpoint = create(&app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    let capture = stored_capture(&store, &endpoint).await;
    Fixture {
        db,
        app,
        endpoint,
        capture,
    }
}

impl Fixture {
    async fn preview(&self, body: Value) -> common::Reply {
        send(
            &self.app,
            request(
                Method::POST,
                &format!("/api/v1/endpoints/{}/template/preview", self.endpoint),
                &body.to_string(),
            ),
        )
        .await
    }

    async fn variables(&self, query: &str) -> common::Reply {
        send(
            &self.app,
            common::bodiless(
                Method::GET,
                &format!(
                    "/api/v1/endpoints/{}/template/variables{query}",
                    self.endpoint
                ),
            ),
        )
        .await
    }

    fn draft(&self, body: &str) -> Value {
        json!({
            "capture_id": self.capture,
            "response": {
                "status": 200,
                "content_type": "application/json",
                "headers": [],
                "body": {"encoding": "utf8", "data": body},
            },
        })
    }

    async fn set_var(&self, name: &str, value: Value) {
        let reply = send(
            &self.app,
            request(
                Method::PUT,
                &format!("/api/v1/endpoints/{}/vars/{name}", self.endpoint),
                &json!({"value": value}).to_string(),
            ),
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    }

    async fn counts(&self) -> (i64, i64, String, String) {
        let captures: i64 =
            sqlx::query_scalar("SELECT count(*) FROM captures WHERE endpoint_id = $1")
                .bind(&self.endpoint)
                .fetch_one(self.db.pool())
                .await
                .expect("the count runs");
        let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs WHERE endpoint_id = $1")
            .bind(&self.endpoint)
            .fetch_one(self.db.pool())
            .await
            .expect("the count runs");
        let vars: String = sqlx::query_scalar(
            "SELECT coalesce(string_agg(name || '=' || value::text || '@' || updated_at::text, ',' ORDER BY name), '') \
             FROM endpoint_vars WHERE endpoint_id = $1",
        )
        .bind(&self.endpoint)
        .fetch_one(self.db.pool())
        .await
        .expect("the variables read");
        let endpoint: String = sqlx::query_scalar(
            "SELECT (capture_count, bytes_used, last_active_at, response_body, response_templated)::text \
             FROM endpoints WHERE id = $1",
        )
        .bind(&self.endpoint)
        .fetch_one(self.db.pool())
        .await
        .expect("the endpoint reads");
        (captures, runs, vars, endpoint)
    }
}

#[tokio::test]
async fn a_draft_renders_status_headers_and_body_against_the_named_capture() {
    let fixture = fixture().await;
    let mut draft = fixture.draft(r#"{"id":"{{ json("$.id") }}","path":"{{ request.path }}"}"#);
    draft["response"]["status_template"] = json!("{{ json(\"$.data.amount\") | truncate(1) }}01");
    draft["response"]["headers"] = json!([{"name": "X-Mode", "value": "{{ request.query.mode }}"}]);
    let reply = fixture.preview(draft).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    let preview = reply.json();
    assert_eq!(preview["capture_id"], fixture.capture);
    assert_eq!(preview["diagnostics"], json!([]));
    let rendered = &preview["rendered"];
    assert_eq!(rendered["status"], 201);
    assert_eq!(rendered["content_type"], "application/json");
    assert_eq!(
        rendered["headers"],
        json!([{"name": "X-Mode", "value": "test"}])
    );
    let text = r#"{"id":"evt_1","path":"/hook/orders"}"#;
    assert_eq!(
        rendered["body"],
        json!({"encoding": "utf8", "data": text, "truncated": false})
    );
    assert_eq!(rendered["body_size"], text.len());
    assert!(rendered["duration_ms"].is_u64());
}

#[tokio::test]
async fn a_template_error_names_its_position_and_nothing_renders() {
    let fixture = fixture().await;
    let reply = fixture
        .preview(fixture.draft("{\n  \"at\": \"{{ now | isodate }}\"\n}"))
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    let preview = reply.json();
    assert_eq!(preview["rendered"], Value::Null);
    let diagnostic = &preview["diagnostics"][0];
    assert_eq!(diagnostic["field"], "response.body");
    assert_eq!(diagnostic["code"], "template_invalid");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(
        (diagnostic["line"].as_u64(), diagnostic["column"].as_u64()),
        (Some(2), Some(19))
    );
    assert!(diagnostic["length"].as_u64() >= Some(1));
    assert!(
        diagnostic["message"]
            .as_str()
            .is_some_and(|text| text.contains("isodate"))
    );
}

#[tokio::test]
async fn a_value_the_capture_lacks_is_a_warning_and_the_draft_still_renders() {
    let fixture = fixture().await;
    let reply = fixture
        .preview(fixture.draft("a={{ request.query.missing }}."))
        .await;
    let preview = reply.json();
    assert_eq!(preview["rendered"]["body"]["data"], "a=.");
    assert_eq!(preview["diagnostics"][0]["severity"], "warning");
    assert_eq!(preview["diagnostics"][0]["code"], "value_missing");
}

#[tokio::test]
async fn secrets_are_refused_in_a_response_template() {
    let fixture = fixture().await;
    let mut draft = fixture.draft("{{ secret.TOKEN }}");
    draft["response"]["headers"] = json!([{"name": "X-Key", "value": "{{ secret.TOKEN }}"}]);
    let preview = fixture.preview(draft).await.json();
    assert_eq!(preview["rendered"], Value::Null);
    let found: Vec<(&str, &str)> = preview["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .map(|item| {
            (
                item["field"].as_str().unwrap_or_default(),
                item["code"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("response.headers.0.value", "secret_not_allowed"),
            ("response.body", "secret_not_allowed")
        ]
    );
}

#[tokio::test]
async fn without_a_capture_the_draft_is_checked_and_not_rendered() {
    let fixture = fixture().await;
    let mut valid = fixture.draft("{{ request.method }}");
    valid["capture_id"] = Value::Null;
    let preview = fixture.preview(valid).await.json();
    assert_eq!(preview["capture_id"], Value::Null);
    assert_eq!(preview["rendered"], Value::Null);
    assert_eq!(preview["diagnostics"], json!([]));
    let mut invalid = fixture.draft("{{ nope }}");
    invalid["capture_id"] = Value::Null;
    let preview = fixture.preview(invalid).await.json();
    assert_eq!(preview["diagnostics"][0]["code"], "template_invalid");
}

#[tokio::test]
async fn an_unknown_capture_is_404_and_a_malformed_id_is_422() {
    let fixture = fixture().await;
    let mut draft = fixture.draft("x");
    draft["capture_id"] = json!(CaptureId::generate().expect("an id").to_string());
    let reply = fixture.preview(draft).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "capture_not_found");
    let other = create(&fixture.app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let store = Store::from_pool(fixture.db.pool().clone());
    let foreign = stored_capture(&store, &other).await;
    let mut draft = fixture.draft("x");
    draft["capture_id"] = json!(foreign);
    assert_eq!(fixture.preview(draft).await.status, StatusCode::NOT_FOUND);
    let mut draft = fixture.draft("x");
    draft["capture_id"] = json!("not-a-capture");
    let reply = fixture.preview(draft).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        [("capture_id".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn the_request_shape_is_validated() {
    let fixture = fixture().await;
    let reply = fixture
        .preview(json!({"response": {"delay_ms": 5, "body": {"encoding": "utf8", "data": ""}}, "extra": 1}))
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let codes = field_codes(&reply.json());
    assert!(codes.contains(&("extra".to_owned(), "not_allowed".to_owned())));
    assert!(codes.contains(&("response.delay_ms".to_owned(), "not_allowed".to_owned())));
    let missing = fixture.preview(json!({"capture_id": null})).await;
    assert_eq!(
        field_codes(&missing.json()),
        [("response".to_owned(), "required".to_owned())]
    );
}

#[tokio::test]
async fn a_draft_over_the_size_limit_or_with_a_bad_header_is_reported_as_diagnostics() {
    let fixture = fixture().await;
    let reply = fixture
        .preview(fixture.draft(&"x".repeat(64 * 1024 + 1)))
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    let preview = reply.json();
    assert_eq!(preview["rendered"], Value::Null);
    assert_eq!(preview["diagnostics"][0]["field"], "response.body");
    assert_eq!(preview["diagnostics"][0]["code"], "too_long");
    let mut draft = fixture.draft("x");
    draft["response"]["headers"] = json!([{"name": "bad name", "value": "v"}]);
    let preview = fixture.preview(draft).await.json();
    assert_eq!(preview["rendered"], Value::Null);
    assert_eq!(
        preview["diagnostics"][0]["field"],
        "response.headers.0.name"
    );
    let huge = fixture
        .preview(fixture.draft(&"x".repeat(300 * 1024)))
        .await;
    assert_eq!(huge.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn rendered_output_over_the_engine_limit_is_an_error() {
    let fixture = fixture().await;
    let store = Store::from_pool(fixture.db.pool().clone());
    let big = {
        let id = CaptureId::generate().expect("an id");
        let outcome = store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: EndpointId::parse(&fixture.endpoint).expect("an id"),
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
                    body: Bytes::from("x".repeat(60 * 1024)),
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
    };
    let mut draft = fixture.draft(&"{{ request.body }}".repeat(30));
    draft["capture_id"] = json!(big);
    let preview = fixture.preview(draft).await.json();
    assert_eq!(preview["rendered"], Value::Null);
    assert_eq!(preview["diagnostics"][0]["code"], "output_limit_exceeded");
    assert_eq!(preview["diagnostics"][0]["severity"], "error");
}

#[tokio::test]
async fn a_preview_changes_nothing_stored() {
    let fixture = fixture().await;
    fixture.set_var("hits", json!(2)).await;
    let before = fixture.counts().await;
    let reply = fixture
        .preview(fixture.draft("{{ counter.hits }}|{{ store.hits }}|{{ uuid() }}"))
        .await;
    assert_eq!(
        reply.json()["rendered"]["body"]["data"]
            .as_str()
            .map(|text| &text[..4]),
        Some("2|2|")
    );
    fixture.preview(fixture.draft("{{ nope }}")).await;
    fixture
        .variables(&format!("?capture_id={}", fixture.capture))
        .await;
    assert_eq!(fixture.counts().await, before);
}

#[tokio::test]
async fn the_catalogue_lists_names_with_samples_from_the_capture() {
    let fixture = fixture().await;
    fixture.set_var("hits", json!(2)).await;
    let reply = fixture
        .variables(&format!("?capture_id={}", fixture.capture))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    let catalogue = reply.json();
    assert_eq!(
        catalogue["delimiters"],
        json!({"open": "{{", "close": "}}"})
    );
    let items = catalogue["items"].as_array().expect("items");
    let find = |path: &str| {
        items
            .iter()
            .find(|item| item["path"] == path)
            .unwrap_or_else(|| panic!("no entry {path}"))
    };
    assert_eq!(
        find("request.method"),
        &json!({
            "path": "request.method",
            "insert_text": "{{ request.method }}",
            "group": "request",
            "type": "string",
            "sample": "\"POST\"",
            "description": "The HTTP method",
        })
    );
    assert_eq!(find("request.query.mode")["sample"], "\"test\"");
    assert_eq!(find("request.header.x-token")["sample"], "\"abc\"");
    assert_eq!(find("json(\"$.data.amount\")")["type"], "integer");
    assert_eq!(find("json(\"$.data.amount\")")["sample"], "2000");
    assert_eq!(find("counter.hits")["group"], "variables");
    assert_eq!(find("uuid()")["group"], "helpers");
    assert_eq!(find("uuid()")["sample"], Value::Null);
    for item in items {
        let keys: Vec<&str> = item
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "description",
                "group",
                "insert_text",
                "path",
                "sample",
                "type"
            ],
            "{item}"
        );
        assert!(
            !item["path"]
                .as_str()
                .unwrap_or_default()
                .starts_with("secret.")
        );
    }
    let again = fixture
        .variables(&format!("?capture_id={}", fixture.capture))
        .await;
    assert_eq!(again.json(), catalogue);
}

#[tokio::test]
async fn without_a_capture_the_catalogue_has_no_request_samples() {
    let fixture = fixture().await;
    fixture.set_var("hits", json!(2)).await;
    let catalogue = fixture.variables("").await.json();
    let items = catalogue["items"].as_array().expect("items");
    let method = items
        .iter()
        .find(|item| item["path"] == "request.method")
        .expect("the fixed name is listed");
    assert_eq!(method["sample"], Value::Null);
    let counter = items
        .iter()
        .find(|item| item["path"] == "counter.hits")
        .expect("the counter is listed");
    assert_eq!(counter["sample"], "2");
    assert!(items.iter().all(|item| {
        !item["path"]
            .as_str()
            .unwrap_or_default()
            .starts_with("request.query.")
    }));
}

#[tokio::test]
async fn the_catalogue_refuses_an_unknown_or_malformed_capture() {
    let fixture = fixture().await;
    let unknown = CaptureId::generate().expect("an id").to_string();
    let reply = fixture.variables(&format!("?capture_id={unknown}")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "capture_not_found");
    let reply = fixture.variables("?capture_id=nope").await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn both_routes_answer_410_for_an_unknown_endpoint_and_404_for_a_malformed_id() {
    let fixture = fixture().await;
    let ghost = "7k2m9q4xw8c1d5f3h6j0n2p4";
    for (method, tail, body) in [
        (Method::POST, "preview", "{\"response\":{}}"),
        (Method::GET, "variables", ""),
    ] {
        let uri = format!("/api/v1/endpoints/{ghost}/template/{tail}");
        let reply = send(&fixture.app, request(method.clone(), &uri, body)).await;
        assert_eq!(reply.status, StatusCode::GONE, "{uri}");
        let uri = format!("/api/v1/endpoints/short/template/{tail}");
        let reply = send(&fixture.app, request(method, &uri, body)).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn a_busy_replica_answers_429_with_a_retry_after() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let app = whl_api::router(ApiState::new(store, api_settings()).with_preview_slots(0));
    let endpoint = create(&app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let preview = send(
        &app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/template/preview"),
            r#"{"response":{"body":{"encoding":"utf8","data":"x"}}}"#,
        ),
    )
    .await;
    assert_eq!(preview.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        preview
            .headers
            .get(header::RETRY_AFTER)
            .map(|value| value.as_bytes()),
        Some(&b"1"[..])
    );
    let variables = send(
        &app,
        common::bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/template/variables"),
        ),
    )
    .await;
    assert_eq!(variables.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_viewer_may_use_both_routes_a_stranger_may_not_and_tokens_are_refused() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let endpoint = harness.endpoint_for(&cast.owner).await;
    harness
        .store
        .set_member(
            &EndpointId::parse(&endpoint).expect("a valid id"),
            RETENTION,
            &cast.editor.email,
            whl_store::GrantedRole::Editor,
        )
        .await
        .expect("the editor is added");
    harness
        .store
        .set_member(
            &EndpointId::parse(&endpoint).expect("a valid id"),
            RETENTION,
            &cast.viewer.email,
            whl_store::GrantedRole::Viewer,
        )
        .await
        .expect("the viewer is added");
    let capture = store_capture(&harness.store, &endpoint).await;
    let preview = format!("/api/v1/endpoints/{endpoint}/template/preview");
    let variables = format!("/api/v1/endpoints/{endpoint}/template/variables?capture_id={capture}");
    let body = r#"{"response":{"body":{"encoding":"utf8","data":"x"}}}"#;
    for (actor, expected) in [
        (&cast.owner, StatusCode::OK),
        (&cast.editor, StatusCode::OK),
        (&cast.viewer, StatusCode::OK),
        (&cast.stranger, StatusCode::GONE),
    ] {
        let reply = harness
            .send_as(Some(actor), Method::POST, &preview, body)
            .await;
        assert_eq!(reply.status, expected, "preview as {}", actor.email);
        let reply = harness
            .send_as(Some(actor), Method::GET, &variables, "")
            .await;
        assert_eq!(reply.status, expected, "variables as {}", actor.email);
    }
    let token = mint(&harness, &cast.owner, &common::tokens::ALL_SCOPES, None).await;
    for (method, uri, payload) in [
        (Method::POST, preview.as_str(), body),
        (Method::GET, variables.as_str(), ""),
    ] {
        let reply = harness
            .send_bearer(&token.token, method, uri, payload)
            .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(reply.json()["error"]["code"], "session_required", "{uri}");
    }
    let anonymous = harness.send_as(None, Method::POST, &preview, body).await;
    assert_eq!(anonymous.status, StatusCode::GONE);
}

#[tokio::test]
async fn a_view_protected_endpoint_refuses_both_routes_without_a_grant_and_serves_them_with_one() {
    let harness = mixed_harness().await;
    let endpoint = harness.anonymous_endpoint(None).await;
    sqlx::query("UPDATE endpoints SET view_password_hash = $2 WHERE id = $1")
        .bind(&endpoint)
        .bind("stub-hash-for-direct-inserts")
        .execute(harness.db.pool())
        .await
        .expect("the endpoint is protected");
    let grant = ViewGrant::generate().expect("entropy is available");
    sqlx::query(
        "INSERT INTO endpoint_grants (token_hash, endpoint_id, expires_at) \
         VALUES ($1, $2, now() + interval '1 hour')",
    )
    .bind(grant.digest().unpack())
    .bind(&endpoint)
    .execute(harness.db.pool())
    .await
    .expect("the grant is stored");
    let preview = format!("/api/v1/endpoints/{endpoint}/template/preview");
    let variables = format!("/api/v1/endpoints/{endpoint}/template/variables");
    let body = r#"{"response":{"body":{"encoding":"utf8","data":"x"}}}"#;
    for (method, uri, payload) in [
        (Method::POST, preview.as_str(), body),
        (Method::GET, variables.as_str(), ""),
    ] {
        let locked = harness
            .send_with(None, Some("203.0.113.7"), &[], method.clone(), uri, payload)
            .await;
        assert_eq!(locked.status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(locked.json()["error"]["code"], "grant_required", "{uri}");
        let opened = harness
            .send_with(
                None,
                Some("203.0.113.7"),
                &[(GRANT_HEADER, grant.unpack())],
                method,
                uri,
                payload,
            )
            .await;
        assert_eq!(opened.status, StatusCode::OK, "{uri} {:?}", opened.json());
    }
}
