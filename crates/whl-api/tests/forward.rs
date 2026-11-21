mod common;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use common::{Reply, UNKNOWN_ID, bodiless, create, field_codes, request, send};
use seed::capture_in;
use serde_json::{Value, json};
use sqlx::postgres::PgListener;
use whl_api::{ApiSettings, ApiState, ForwardSettings};
use whl_forward::{GuardConfig, StaticLookup};
use whl_store::Store;
use whl_store::forward::{AttemptRecord, ClaimPolicy, Settlement};
use whl_testkit::TestDb;

mod seed {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use time::OffsetDateTime;
    use whl_core::capture_id::CaptureId;
    use whl_core::endpoint_id::EndpointId;
    use whl_store::Store;
    use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};

    pub async fn capture_in(store: &Store, endpoint: &str) -> String {
        let id = CaptureId::generate().expect("an id is generated");
        let outcome = store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: EndpointId::parse(endpoint).expect("a valid endpoint id"),
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
}

struct Rig {
    db: TestDb,
    store: Store,
    app: Router,
}

fn settings(forwarding: bool) -> ApiSettings {
    ApiSettings {
        public_url: "https://whl.test".to_owned(),
        retention: Duration::from_secs(7 * 24 * 3600),
        max_captures: 500,
        max_body_bytes: 10 * 1024 * 1024,
        forwarding,
        private_forward_targets_allowed: false,
        ring_mode_allowed: true,
    }
}

async fn rig_with(forwarding: bool) -> Rig {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let public: IpAddr = "93.184.216.34".parse().expect("a valid address");
    let private: IpAddr = "10.0.0.5".parse().expect("a valid address");
    let lookup = StaticLookup::new()
        .with_answer("public.test", &[public])
        .with_answer("private.test", &[private]);
    let state = ApiState::new(store.clone(), settings(forwarding)).with_forward(
        ForwardSettings::new(GuardConfig::default(), Arc::new(lookup)),
    );
    let app = whl_api::router(state);
    Rig { db, store, app }
}

async fn rig() -> Rig {
    rig_with(true).await
}

fn rules_uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/forward-rules")
}

async fn post_rule(rig: &Rig, endpoint: &str, body: Value) -> Reply {
    send(
        &rig.app,
        request(Method::POST, &rules_uri(endpoint), &body.to_string()),
    )
    .await
}

async fn endpoint_id(rig: &Rig) -> String {
    create(&rig.app, "").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

#[tokio::test]
async fn rules_are_created_listed_updated_and_deleted_in_the_documented_shape() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;

    let created = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/hook"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let rule = created.json();
    let id = rule["id"].as_str().expect("the rule has an id").to_owned();
    assert_eq!(rule["enabled"], true);
    assert_eq!(rule["target_url"], "https://public.test/hook");
    assert_eq!(rule["method"], Value::Null);
    assert_eq!(rule["content_type"], Value::Null);
    assert_eq!(rule["header_allowlist"], json!([]));
    assert_eq!(rule["append_path"], false);
    assert!(
        rule["created_at"]
            .as_str()
            .is_some_and(|text| text.ends_with('Z'))
    );

    let second = post_rule(
        &rig,
        &endpoint,
        json!({
            "target_url": "https://public.test/other",
            "method": "put",
            "content_type": "application/json",
            "header_allowlist": ["X-Signature", "x-signature", "X-Other"],
            "append_path": true,
            "enabled": false,
        }),
    )
    .await;
    assert_eq!(second.status, StatusCode::CREATED);
    let second = second.json();
    assert_eq!(second["method"], "PUT");
    assert_eq!(
        second["header_allowlist"],
        json!(["X-Signature", "X-Other"])
    );
    assert_eq!(second["enabled"], false);

    let listed = send(&rig.app, bodiless(Method::GET, &rules_uri(&endpoint))).await;
    assert_eq!(listed.status, StatusCode::OK);
    let items = listed.json()["items"].clone();
    assert_eq!(items[0]["id"], rule["id"]);
    assert_eq!(items[1]["id"], second["id"]);

    let patched = send(
        &rig.app,
        request(
            Method::PATCH,
            &format!("{}/{id}", rules_uri(&endpoint)),
            &json!({"method": "PATCH", "append_path": true}).to_string(),
        ),
    )
    .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(patched.json()["method"], "PATCH");
    assert_eq!(patched.json()["target_url"], "https://public.test/hook");
    let reset = send(
        &rig.app,
        request(
            Method::PATCH,
            &format!("{}/{id}", rules_uri(&endpoint)),
            &json!({"method": null}).to_string(),
        ),
    )
    .await;
    assert_eq!(reset.json()["method"], Value::Null);
    assert_eq!(reset.json()["append_path"], true);

    let deleted = send(
        &rig.app,
        bodiless(Method::DELETE, &format!("{}/{id}", rules_uri(&endpoint))),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let remaining = send(&rig.app, bodiless(Method::GET, &rules_uri(&endpoint))).await;
    assert_eq!(remaining.json()["items"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn the_endpoint_carries_its_rules() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let uri = format!("/api/v1/endpoints/{endpoint}");

    let empty = send(&rig.app, bodiless(Method::GET, &uri)).await;
    assert_eq!(empty.json()["forward_rules"], json!([]));

    let created = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/hook"}),
    )
    .await;
    let shown = send(&rig.app, bodiless(Method::GET, &uri)).await;
    assert_eq!(shown.json()["forward_rules"], json!([created.json()]));
    let patched = send(&rig.app, request(Method::PATCH, &uri, "{}")).await;
    assert_eq!(patched.json()["forward_rules"], json!([created.json()]));
}

#[tokio::test]
async fn a_sixth_rule_is_refused_and_a_freed_slot_is_reusable() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let mut first = String::new();
    for index in 0..5 {
        let reply = post_rule(
            &rig,
            &endpoint,
            json!({"target_url": format!("https://public.test/{index}")}),
        )
        .await;
        assert_eq!(reply.status, StatusCode::CREATED);
        if index == 0 {
            first = reply.json()["id"].as_str().expect("an id").to_owned();
        }
    }

    let refused = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/6"}),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["error"]["code"], "forward_rule_limit");

    send(
        &rig.app,
        bodiless(Method::DELETE, &format!("{}/{first}", rules_uri(&endpoint))),
    )
    .await;
    let again = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/7"}),
    )
    .await;
    assert_eq!(again.status, StatusCode::CREATED);
}

#[tokio::test]
async fn target_urls_are_checked_with_field_codes() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let cases = [
        ("http://10.0.0.5/", "target_refused"),
        ("http://169.254.169.254/latest", "target_refused"),
        ("http://localhost/", "target_refused"),
        ("https://private.test/", "target_refused"),
        ("https://missing.test/", "target_unresolvable"),
        ("ftp://public.test/", "invalid_format"),
        ("not a url", "invalid_format"),
    ];
    for (target, code) in cases {
        let reply = post_rule(&rig, &endpoint, json!({"target_url": target})).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{target}");
        let body = reply.json();
        assert_eq!(body["error"]["code"], "validation_failed", "{target}");
        assert_eq!(
            field_codes(&body),
            vec![("target_url".to_owned(), code.to_owned())],
            "{target}"
        );
    }
    let long = format!("https://public.test/{}", "a".repeat(2100));
    let reply = post_rule(&rig, &endpoint, json!({"target_url": long})).await;
    assert_eq!(
        field_codes(&reply.json()),
        vec![("target_url".to_owned(), "invalid_format".to_owned())]
    );
}

#[tokio::test]
async fn every_invalid_field_is_reported_with_its_path() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let reply = post_rule(
        &rig,
        &endpoint,
        json!({
            "method": "NOT A TOKEN",
            "content_type": "nonsense",
            "header_allowlist": ["Good", "bad header", 7, ""],
            "enabled": "yes",
            "surprise": 1,
        }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut fields = field_codes(&reply.json());
    fields.sort();
    let mut expected: Vec<(String, String)> = [
        ("content_type", "invalid_format"),
        ("enabled", "invalid_format"),
        ("header_allowlist.1", "invalid_format"),
        ("header_allowlist.2", "invalid_format"),
        ("header_allowlist.3", "required"),
        ("method", "invalid_format"),
        ("surprise", "not_allowed"),
        ("target_url", "required"),
    ]
    .iter()
    .map(|(field, code)| ((*field).to_owned(), (*code).to_owned()))
    .collect();
    expected.sort();
    assert_eq!(fields, expected);

    let long_method = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/", "method": "A".repeat(17)}),
    )
    .await;
    assert_eq!(
        field_codes(&long_method.json()),
        vec![("method".to_owned(), "too_long".to_owned())]
    );
    let many = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/", "header_allowlist": vec!["X"; 51]}),
    )
    .await;
    assert_eq!(
        field_codes(&many.json()),
        vec![("header_allowlist".to_owned(), "too_many".to_owned())]
    );
}

#[tokio::test]
async fn patch_rechecks_a_changed_target_and_reports_missing_things() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let rule = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/", "content_type": "text/plain"}),
    )
    .await
    .json();
    let uri = format!(
        "{}/{}",
        rules_uri(&endpoint),
        rule["id"].as_str().expect("an id")
    );

    let refused = send(
        &rig.app,
        request(
            Method::PATCH,
            &uri,
            &json!({"target_url": "http://10.1.1.1/"}).to_string(),
        ),
    )
    .await;
    assert_eq!(
        field_codes(&refused.json()),
        vec![("target_url".to_owned(), "target_refused".to_owned())]
    );
    let cleared = send(
        &rig.app,
        request(
            Method::PATCH,
            &uri,
            &json!({"content_type": null}).to_string(),
        ),
    )
    .await;
    assert_eq!(cleared.json()["content_type"], Value::Null);
    assert_eq!(cleared.json()["target_url"], "https://public.test/");

    let empty = send(&rig.app, request(Method::PATCH, &uri, "")).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    assert_eq!(empty.json()["error"]["code"], "invalid_json");

    let missing = send(
        &rig.app,
        request(
            Method::PATCH,
            &format!(
                "{}/00000000-0000-4000-8000-000000000000",
                rules_uri(&endpoint)
            ),
            "{}",
        ),
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.json()["error"]["code"], "forward_rule_not_found");
    let malformed = send(
        &rig.app,
        request(
            Method::PATCH,
            &format!("{}/nope", rules_uri(&endpoint)),
            "{}",
        ),
    )
    .await;
    assert_eq!(malformed.json()["error"]["code"], "not_found");
}

#[tokio::test]
async fn rule_routes_answer_gone_for_an_unknown_endpoint_and_refuse_writes_when_forwarding_is_off()
{
    let rig = rig().await;
    let gone = post_rule(
        &rig,
        UNKNOWN_ID,
        json!({"target_url": "https://public.test/"}),
    )
    .await;
    assert_eq!(gone.status, StatusCode::GONE);
    assert_eq!(gone.json()["error"]["code"], "endpoint_gone");
    let listed = send(&rig.app, bodiless(Method::GET, &rules_uri(UNKNOWN_ID))).await;
    assert_eq!(listed.status, StatusCode::GONE);

    let off = rig_with(false).await;
    let endpoint = endpoint_id(&off).await;
    let refused = post_rule(
        &off,
        &endpoint,
        json!({"target_url": "https://public.test/"}),
    )
    .await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND);
    let listed = send(&off.app, bodiless(Method::GET, &rules_uri(&endpoint))).await;
    assert_eq!(listed.status, StatusCode::OK);
}

#[tokio::test]
async fn rule_changes_announce_a_thin_endpoint_updated_event() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let mut listener = PgListener::connect(rig.db.url())
        .await
        .expect("the listener connects");
    listener
        .listen("whl_events")
        .await
        .expect("the listener subscribes");

    let rule = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/"}),
    )
    .await
    .json();
    let uri = format!(
        "{}/{}",
        rules_uri(&endpoint),
        rule["id"].as_str().expect("an id")
    );
    send(
        &rig.app,
        request(Method::PATCH, &uri, &json!({"enabled": false}).to_string()),
    )
    .await;
    send(&rig.app, bodiless(Method::DELETE, &uri)).await;

    for _ in 0..3 {
        let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
            .await
            .expect("a notification arrives")
            .expect("the listener stays connected");
        let payload: Value =
            serde_json::from_str(notification.payload()).expect("the payload is JSON");
        assert_eq!(payload, json!({"t": "endpoint_updated", "e": endpoint}));
    }
}

fn forward_uri(endpoint: &str, capture: &str, verb: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/captures/{capture}/{verb}")
}

#[tokio::test]
async fn resend_creates_a_pending_manual_job_for_an_ad_hoc_target_or_a_rule() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let capture = capture_in(&rig.store, &endpoint).await;
    let rule = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/rule", "method": "PUT", "enabled": false}),
    )
    .await
    .json();

    let adhoc = send(
        &rig.app,
        request(
            Method::POST,
            &forward_uri(&endpoint, &capture, "forward"),
            &json!({"target_url": "https://public.test/adhoc"}).to_string(),
        ),
    )
    .await;
    assert_eq!(adhoc.status, StatusCode::ACCEPTED);
    let job = adhoc.json();
    assert_eq!(job["kind"], "manual");
    assert_eq!(job["rule_id"], Value::Null);
    assert_eq!(job["target_url"], "https://public.test/adhoc");
    assert_eq!(job["method"], "POST");
    assert_eq!(job["state"], "pending");
    assert_eq!(job["attempts"], json!([]));
    assert_eq!(job["finished_at"], Value::Null);

    let by_rule = send(
        &rig.app,
        request(
            Method::POST,
            &forward_uri(&endpoint, &capture, "resend"),
            &json!({"rule_id": rule["id"]}).to_string(),
        ),
    )
    .await;
    assert_eq!(by_rule.status, StatusCode::ACCEPTED);
    let job = by_rule.json();
    assert_eq!(job["rule_id"], rule["id"]);
    assert_eq!(job["target_url"], "https://public.test/rule");
    assert_eq!(job["method"], "PUT");

    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_jobs WHERE kind = 'manual'")
        .fetch_one(rig.db.pool())
        .await
        .expect("the jobs are countable");
    assert_eq!(queued, 2);
}

#[tokio::test]
async fn resend_validates_its_input_and_reports_missing_things() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    let capture = capture_in(&rig.store, &endpoint).await;
    let rule = post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/"}),
    )
    .await
    .json();
    let rule_id = rule["id"].as_str().expect("an id").to_owned();
    let uri = forward_uri(&endpoint, &capture, "forward");

    let refused = send(
        &rig.app,
        request(
            Method::POST,
            &uri,
            &json!({"target_url": "http://10.0.0.1/"}).to_string(),
        ),
    )
    .await;
    assert_eq!(
        field_codes(&refused.json()),
        vec![("target_url".to_owned(), "target_refused".to_owned())]
    );
    let neither = send(&rig.app, request(Method::POST, &uri, "")).await;
    assert_eq!(
        field_codes(&neither.json()),
        vec![("target_url".to_owned(), "required".to_owned())]
    );
    let both = send(
        &rig.app,
        request(
            Method::POST,
            &uri,
            &json!({"rule_id": rule_id, "target_url": "https://public.test/", "method": "PUT"})
                .to_string(),
        ),
    )
    .await;
    let mut conflicts = field_codes(&both.json());
    conflicts.sort();
    assert_eq!(
        conflicts,
        vec![
            ("method".to_owned(), "not_allowed".to_owned()),
            ("target_url".to_owned(), "not_allowed".to_owned()),
        ]
    );

    send(
        &rig.app,
        bodiless(
            Method::DELETE,
            &format!("{}/{rule_id}", rules_uri(&endpoint)),
        ),
    )
    .await;
    let deleted_rule = send(
        &rig.app,
        request(Method::POST, &uri, &json!({"rule_id": rule_id}).to_string()),
    )
    .await;
    assert_eq!(deleted_rule.status, StatusCode::NOT_FOUND);
    assert_eq!(
        deleted_rule.json()["error"]["code"],
        "forward_rule_not_found"
    );

    let unknown_capture = send(
        &rig.app,
        request(
            Method::POST,
            &forward_uri(&endpoint, "0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b", "forward"),
            &json!({"target_url": "https://public.test/"}).to_string(),
        ),
    )
    .await;
    assert_eq!(unknown_capture.status, StatusCode::NOT_FOUND);
    assert_eq!(unknown_capture.json()["error"]["code"], "capture_not_found");
    let gone = send(
        &rig.app,
        request(
            Method::POST,
            &forward_uri(UNKNOWN_ID, &capture, "forward"),
            &json!({"target_url": "https://public.test/"}).to_string(),
        ),
    )
    .await;
    assert_eq!(gone.status, StatusCode::GONE);
}

#[tokio::test]
async fn capture_detail_and_summary_carry_the_jobs_and_their_attempts() {
    let rig = rig().await;
    let endpoint = endpoint_id(&rig).await;
    post_rule(
        &rig,
        &endpoint,
        json!({"target_url": "https://public.test/rule"}),
    )
    .await;
    let capture = capture_in(&rig.store, &endpoint).await;
    let manual = send(
        &rig.app,
        request(
            Method::POST,
            &forward_uri(&endpoint, &capture, "forward"),
            &json!({"target_url": "https://public.test/manual"}).to_string(),
        ),
    )
    .await
    .json();

    let detail_uri = format!("/api/v1/endpoints/{endpoint}/captures/{capture}");
    let pending = send(&rig.app, bodiless(Method::GET, &detail_uri))
        .await
        .json();
    assert_eq!(pending["forward_state"], "pending");
    assert_eq!(pending["forwards"].as_array().map(Vec::len), Some(2));
    assert_eq!(pending["forwards"][0]["id"], manual["id"]);

    let claimed = rig
        .store
        .claim_forward_jobs(ClaimPolicy {
            limit: 10,
            max_attempts: 3,
            lease: Duration::from_secs(60),
        })
        .await
        .expect("the claim succeeds");
    for job in claimed {
        let (status, body, truncated) = if job.target_url.ends_with("/rule") {
            (Some(200), Some(b"ok".to_vec()), false)
        } else {
            (Some(502), Some(vec![b'z'; 4096]), true)
        };
        let settlement = if status == Some(200) {
            Settlement::Succeeded
        } else {
            Settlement::Failed
        };
        rig.store
            .record_forward_attempt(
                &AttemptRecord {
                    job_id: job.id,
                    attempt: job.attempt,
                    duration_ms: 42,
                    status,
                    error: None,
                    final_url: Some(job.target_url.clone()),
                    response_content_type: Some("text/plain".to_owned()),
                    response_body: body,
                    response_truncated: truncated,
                },
                settlement,
            )
            .await
            .expect("the attempt is recorded");
    }

    let done = send(&rig.app, bodiless(Method::GET, &detail_uri))
        .await
        .json();
    assert_eq!(done["forward_state"], "failed");
    let failed = &done["forwards"][0];
    assert_eq!(failed["state"], "failed");
    assert!(failed["finished_at"].is_string());
    let attempt = &failed["attempts"][0];
    assert_eq!(attempt["attempt"], 1);
    assert_eq!(attempt["status"], 502);
    assert_eq!(attempt["duration_ms"], 42);
    assert_eq!(attempt["error"], Value::Null);
    assert_eq!(attempt["response_content_type"], "text/plain");
    assert_eq!(attempt["response_body"]["encoding"], "utf8");
    assert_eq!(attempt["response_body"]["truncated"], true);
    assert_eq!(
        attempt["response_body"]["data"].as_str().map(str::len),
        Some(4096)
    );
    assert_eq!(done["forwards"][1]["state"], "succeeded");
    assert_eq!(
        done["forwards"][1]["attempts"][0]["response_body"]["data"],
        "ok"
    );

    let listed = send(
        &rig.app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/captures"),
        ),
    )
    .await
    .json();
    assert_eq!(listed["items"][0]["forward_state"], "failed");
}
