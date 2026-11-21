mod common;

use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use common::{Harness, UNKNOWN_ID, bodiless, create, field_codes, harness, send};
use serde_json::{Value, json};
use sqlx::postgres::PgListener;
use time::OffsetDateTime;
use tower::ServiceExt;
use whl_core::capture_id::CaptureId;
use whl_testkit::TestDb;

const BASE: i64 = 1_800_000_000;
const OVERHEAD: i64 = 100;

#[derive(Clone)]
struct Spec {
    method: &'static str,
    path: String,
    query: Vec<(&'static str, &'static str)>,
    headers: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
    status: Option<i16>,
    at: i64,
    form: Option<Value>,
}

impl Spec {
    fn new(method: &'static str, path: &str) -> Self {
        Self {
            method,
            path: path.to_owned(),
            query: Vec::new(),
            headers: Vec::new(),
            body: Vec::new(),
            status: None,
            at: BASE,
            form: None,
        }
    }
}

async fn insert(db: &TestDb, endpoint: &str, spec: Spec) -> String {
    let capture = CaptureId::generate().expect("an id is generated");
    let size = i64::try_from(spec.body.len()).expect("the body fits");
    let stored = size + OVERHEAD;
    let seq: i64 = sqlx::query_scalar(
        "UPDATE endpoints SET last_seq = last_seq + 1, capture_count = capture_count + 1, \
         bytes_used = bytes_used + $2 WHERE id = $1 RETURNING last_seq",
    )
    .bind(endpoint)
    .bind(stored)
    .fetch_one(db.pool())
    .await
    .expect("the counters are updated");
    let query_string = spec
        .query
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let url = if query_string.is_empty() {
        format!("https://whl.test/{endpoint}{}", spec.path)
    } else {
        format!("https://whl.test/{endpoint}{}?{query_string}", spec.path)
    };
    let headers: Vec<Value> = spec
        .headers
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    let query: Vec<Value> = spec
        .query
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    let received_at = OffsetDateTime::from_unix_timestamp(spec.at).expect("a valid timestamp");
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, form, content_type, body_size, stored_bytes, status_override, \
         response_status) VALUES ($1, $2, $3, $4, $5, $6, $7, 'whl.test', '203.0.113.7'::inet, \
         $8::text::jsonb, $9::text::jsonb, $10::text::jsonb, 'text/plain', $11, $12, $13, 202)",
    )
    .bind(endpoint)
    .bind(seq)
    .bind(capture.unpack())
    .bind(received_at)
    .bind(spec.method)
    .bind(url)
    .bind(spec.path.as_str())
    .bind(serde_json::to_string(&headers).expect("headers encode"))
    .bind(serde_json::to_string(&query).expect("query encodes"))
    .bind(spec.form.as_ref().map(Value::to_string))
    .bind(size)
    .bind(stored)
    .bind(spec.status)
    .execute(db.pool())
    .await
    .expect("the capture is inserted");
    let header_text: String = spec
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\n"))
        .collect();
    let search_text = format!(
        "{} {} {}{}",
        spec.path,
        query_string,
        header_text,
        String::from_utf8_lossy(&spec.body)
    )
    .to_lowercase();
    sqlx::query("INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, $2, $3)")
        .bind(capture.unpack())
        .bind(&spec.body)
        .bind(search_text)
        .execute(db.pool())
        .await
        .expect("the body is inserted");
    capture.to_string()
}

async fn endpoint_id(app: &Router) -> String {
    create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

async fn get(app: &Router, uri: &str) -> common::Reply {
    send(app, bodiless(Method::GET, uri)).await
}

fn seqs(page: &Value) -> Vec<i64> {
    page["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["seq"].as_i64().expect("seq is an integer"))
        .collect()
}

async fn seeded(db: &TestDb, endpoint: &str, count: i64) {
    for _ in 0..count {
        insert(db, endpoint, Spec::new("POST", "/hook")).await;
    }
}

#[tokio::test]
async fn list_pages_in_both_orders_without_gaps_or_duplicates() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    seeded(&db, &id, 7).await;

    for (order, expected) in [
        ("desc", vec![7, 6, 5, 4, 3, 2, 1]),
        ("asc", vec![1, 2, 3, 4, 5, 6, 7]),
    ] {
        let mut collected = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let mut uri = format!("/api/v1/endpoints/{id}/captures?order={order}&limit=3");
            if let Some(cursor) = &cursor {
                uri.push_str(&format!("&cursor={cursor}"));
            }
            let reply = get(&app, &uri).await;
            assert_eq!(reply.status, StatusCode::OK);
            pages += 1;
            let page = reply.json();
            collected.extend(seqs(&page));
            match page["next_cursor"].as_str() {
                Some(next) => cursor = Some(next.to_owned()),
                None => break,
            }
        }
        assert_eq!(
            pages, 3,
            "seven captures at three per page make three pages"
        );
        assert_eq!(collected, expected, "order {order}");
    }
}

#[tokio::test]
async fn cursor_stays_stable_when_captures_arrive_between_pages() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    seeded(&db, &id, 5).await;
    let first = get(&app, &format!("/api/v1/endpoints/{id}/captures?limit=2")).await;
    let page = first.json();
    assert_eq!(seqs(&page), vec![5, 4]);
    seeded(&db, &id, 2).await;
    let cursor = page["next_cursor"].as_str().expect("a cursor is returned");
    let second = get(
        &app,
        &format!("/api/v1/endpoints/{id}/captures?limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(seqs(&second.json()), vec![3, 2]);
}

#[tokio::test]
async fn each_filter_selects_its_captures_alone() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    insert(&db, &id, Spec::new("POST", "/a")).await;
    insert(
        &db,
        &id,
        Spec {
            status: Some(404),
            at: BASE + 10,
            ..Spec::new("PUT", "/b")
        },
    )
    .await;
    insert(
        &db,
        &id,
        Spec {
            status: Some(503),
            at: BASE + 20,
            ..Spec::new("POST", "/c")
        },
    )
    .await;
    insert(
        &db,
        &id,
        Spec {
            status: Some(201),
            at: BASE + 30,
            ..Spec::new("GET", "/d")
        },
    )
    .await;
    let base = format!("/api/v1/endpoints/{id}/captures?order=asc");
    let cases = [
        ("method=POST", vec![1, 3]),
        ("method=POST,PUT", vec![1, 2, 3]),
        ("status=5xx", vec![3]),
        ("status=none", vec![1]),
        ("status=4xx,none", vec![1, 2]),
        ("after_seq=2", vec![3, 4]),
        (
            "since=2027-01-15T08:00:10Z&until=2027-01-15T08:00:30Z",
            vec![2, 3],
        ),
        ("q=/c", vec![3]),
    ];
    for (filter, expected) in cases {
        let page = get(&app, &format!("{base}&{filter}")).await.json();
        assert_eq!(seqs(&page), expected, "filter {filter}");
    }
    let page = get(&app, &format!("{base}&method=POST&status=5xx&after_seq=1")).await;
    assert_eq!(seqs(&page.json()), vec![3], "combined filters");
}

#[tokio::test]
async fn search_matches_headers_and_path_but_not_unrelated_captures() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    insert(
        &db,
        &id,
        Spec {
            headers: vec![("X-Trace", "trace-zeta")],
            ..Spec::new("POST", "/orders/alpha")
        },
    )
    .await;
    insert(
        &db,
        &id,
        Spec {
            body: b"sigma".to_vec(),
            ..Spec::new("POST", "/other")
        },
    )
    .await;
    let base = format!("/api/v1/endpoints/{id}/captures");
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=ZETA")).await.json()),
        vec![1]
    );
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=alpha")).await.json()),
        vec![1]
    );
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=sigma")).await.json()),
        vec![2]
    );
}

#[tokio::test]
async fn search_treats_sql_metacharacters_and_percent_literally() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    insert(&db, &id, Spec::new("POST", "/plain")).await;
    insert(
        &db,
        &id,
        Spec {
            body: b"100% a_b' OR 1=1 --".to_vec(),
            ..Spec::new("POST", "/special")
        },
    )
    .await;
    let base = format!("/api/v1/endpoints/{id}/captures");
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=100%25")).await.json()),
        vec![2]
    );
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=a_b%27")).await.json()),
        vec![2]
    );
    assert_eq!(
        seqs(
            &get(&app, &format!("{base}?q=%27%20OR%201%3D1"))
                .await
                .json()
        ),
        vec![2]
    );
    assert_eq!(
        seqs(&get(&app, &format!("{base}?q=%25")).await.json()),
        vec![2]
    );
    assert!(seqs(&get(&app, &format!("{base}?q=a%25b")).await.json()).is_empty());
}

#[tokio::test]
async fn invalid_query_parameters_answer_422_and_invalid_cursor_400() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    seeded(&db, &id, 3).await;
    let base = format!("/api/v1/endpoints/{id}/captures");
    let reply = get(&app, &format!("{base}?limit=0")).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("limit".to_owned(), "out_of_range".to_owned())]
    );
    let reply = get(&app, &format!("{base}?limit=abc&order=sideways")).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(field_codes(&reply.json()).len(), 2);
    assert_eq!(
        get(&app, &format!("{base}?limit=201")).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let reply = get(&app, &format!("{base}?q=")).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let reply = get(&app, &format!("{base}?cursor=not-a-cursor")).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.json()["error"]["code"], "invalid_cursor");
    let first = get(&app, &format!("{base}?limit=1")).await.json();
    let cursor = first["next_cursor"].as_str().expect("a cursor is returned");
    let reply = get(&app, &format!("{base}?order=asc&limit=1&cursor={cursor}")).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.json()["error"]["code"], "invalid_cursor");
    let reply = get(&app, &format!("{base}?method=POST&limit=1&cursor={cursor}")).await;
    assert_eq!(
        reply.status,
        StatusCode::BAD_REQUEST,
        "a changed filter set"
    );
}

#[tokio::test]
async fn total_counts_the_filtered_set_ignoring_cursor_and_after_seq() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    for index in 0..5 {
        let method = if index % 2 == 0 { "POST" } else { "GET" };
        insert(&db, &id, Spec::new(method, "/t")).await;
    }
    let base = format!("/api/v1/endpoints/{id}/captures?method=POST&limit=1");
    let first = get(&app, &base).await.json();
    assert_eq!(first["total"], 3);
    let cursor = first["next_cursor"].as_str().expect("a cursor is returned");
    let second = get(&app, &format!("{base}&cursor={cursor}")).await.json();
    assert_eq!(second["total"], 3);
    let after = get(&app, &format!("{base}&after_seq=3")).await.json();
    assert_eq!(after["total"], 3);
    assert_eq!(seqs(&after), vec![5]);
}

#[tokio::test]
async fn detail_returns_every_documented_field() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let form = json!({
        "fields": [{"name": "k", "value": "v"}],
        "files": [{"name": "f", "filename": "a.txt", "content_type": null, "size": 3}]
    });
    let capture = insert(
        &db,
        &id,
        Spec {
            headers: vec![("X-Dup", "one"), ("X-Dup", "two")],
            query: vec![("a", "1"), ("a", "2")],
            body: b"hello".to_vec(),
            status: Some(404),
            form: Some(form.clone()),
            ..Spec::new("POST", "/detail")
        },
    )
    .await;
    let reply = get(&app, &format!("/api/v1/endpoints/{id}/captures/{capture}")).await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["id"], capture);
    assert_eq!(body["seq"], 1);
    assert_eq!(body["endpoint_id"], id);
    assert!(
        body["received_at"]
            .as_str()
            .expect("timestamp")
            .ends_with('Z')
    );
    assert_eq!(body["method"], "POST");
    assert_eq!(body["path"], "/detail");
    assert_eq!(body["query_string"], "a=1&a=2");
    assert_eq!(body["client_ip"], "203.0.113.7");
    assert_eq!(body["content_type"], "text/plain");
    assert_eq!(body["body_size"], 5);
    assert_eq!(body["status_override"], 404);
    assert_eq!(body["response_status"], 202);
    assert!(body["forward_state"].is_null());
    assert_eq!(body["url"], format!("https://whl.test/{id}/detail?a=1&a=2"));
    assert_eq!(body["host"], "whl.test");
    assert_eq!(
        body["headers"],
        json!([{"name": "X-Dup", "value": "one"}, {"name": "X-Dup", "value": "two"}])
    );
    assert_eq!(
        body["query"],
        json!([{"name": "a", "value": "1"}, {"name": "a", "value": "2"}])
    );
    assert_eq!(body["form"], form);
    assert_eq!(
        body["body"],
        json!({"encoding": "utf8", "data": "hello", "truncated": false})
    );
    assert_eq!(body["forwards"], json!([]));
}

#[tokio::test]
async fn detail_body_is_a_prefix_of_one_mebibyte_and_marks_truncation() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let size = 1024 * 1024 + 10;
    let capture = insert(
        &db,
        &id,
        Spec {
            body: vec![b'x'; size],
            ..Spec::new("POST", "/big")
        },
    )
    .await;
    let request = bodiless(
        Method::GET,
        &format!("/api/v1/endpoints/{id}/captures/{capture}"),
    );
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("the body is readable");
    let body: Value = serde_json::from_slice(&bytes).expect("the body is JSON");
    assert_eq!(body["body"]["truncated"], true);
    assert_eq!(
        body["body"]["data"].as_str().map(str::len),
        Some(1024 * 1024)
    );
    assert_eq!(body["body_size"], size);
}

#[tokio::test]
async fn latest_returns_the_newest_capture_and_404_when_empty() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let reply = get(&app, &format!("/api/v1/endpoints/{id}/captures/latest")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "capture_not_found");
    seeded(&db, &id, 2).await;
    let reply = get(&app, &format!("/api/v1/endpoints/{id}/captures/latest")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["seq"], 2);
    assert_eq!(
        reply
            .headers
            .get(header::CACHE_CONTROL)
            .expect("cache control"),
        "no-store"
    );
    assert_eq!(
        reply
            .headers
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .expect("nosniff"),
        "nosniff"
    );
}

#[tokio::test]
async fn deleting_one_capture_updates_counters_and_notifies() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let first = insert(&db, &id, Spec::new("POST", "/one")).await;
    insert(&db, &id, Spec::new("POST", "/two")).await;
    let mut listener = PgListener::connect_with(db.pool())
        .await
        .expect("a listener connects");
    listener
        .listen("whl_events")
        .await
        .expect("the channel is listened to");
    let uri = format!("/api/v1/endpoints/{id}/captures/{first}");
    let reply = send(&app, bodiless(Method::DELETE, &uri)).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("the notification arrives")
        .expect("the notification is readable");
    let payload: Value = serde_json::from_str(notification.payload()).expect("payload is JSON");
    assert_eq!(
        payload,
        json!({"t": "capture_deleted", "e": id, "d": {"capture_id": first, "seq": 1}})
    );
    let endpoint = get(&app, &format!("/api/v1/endpoints/{id}")).await.json();
    assert_eq!(endpoint["capture_count"], 1);
    assert_eq!(endpoint["bytes_used"], OVERHEAD);
    assert_eq!(get(&app, &uri).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn clear_all_empties_the_list_keeps_seq_and_notifies() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    seeded(&db, &id, 2).await;
    let mut listener = PgListener::connect_with(db.pool())
        .await
        .expect("a listener connects");
    listener
        .listen("whl_events")
        .await
        .expect("the channel is listened to");
    let reply = send(
        &app,
        bodiless(Method::DELETE, &format!("/api/v1/endpoints/{id}/captures")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("the notification arrives")
        .expect("the notification is readable");
    let payload: Value = serde_json::from_str(notification.payload()).expect("payload is JSON");
    assert_eq!(payload, json!({"t": "captures_cleared", "e": id, "d": {}}));
    let page = get(&app, &format!("/api/v1/endpoints/{id}/captures"))
        .await
        .json();
    assert_eq!(page["total"], 0);
    assert!(seqs(&page).is_empty());
    let endpoint = get(&app, &format!("/api/v1/endpoints/{id}")).await.json();
    assert_eq!(endpoint["capture_count"], 0);
    assert_eq!(endpoint["bytes_used"], 0);
    assert_eq!(endpoint["last_seq"], 2);
    insert(&db, &id, Spec::new("POST", "/after")).await;
    let page = get(&app, &format!("/api/v1/endpoints/{id}/captures"))
        .await
        .json();
    assert_eq!(seqs(&page), vec![3]);
}

#[tokio::test]
async fn unknown_expired_and_deleted_endpoints_answer_410() {
    let Harness { db, app } = harness().await;
    let expired = endpoint_id(&app).await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '8 days' WHERE id = $1")
        .bind(&expired)
        .execute(db.pool())
        .await
        .expect("the endpoint is aged");
    let deleted = endpoint_id(&app).await;
    send(
        &app,
        bodiless(Method::DELETE, &format!("/api/v1/endpoints/{deleted}")),
    )
    .await;
    for id in [UNKNOWN_ID, expired.as_str(), deleted.as_str()] {
        let capture = CaptureId::generate()
            .expect("an id is generated")
            .to_string();
        for uri in [
            format!("/api/v1/endpoints/{id}/captures"),
            format!("/api/v1/endpoints/{id}/captures/latest"),
            format!("/api/v1/endpoints/{id}/captures/{capture}"),
        ] {
            let reply = get(&app, &uri).await;
            assert_eq!(reply.status, StatusCode::GONE, "GET {uri}");
            assert_eq!(reply.json()["error"]["code"], "endpoint_gone");
        }
        let reply = send(
            &app,
            bodiless(Method::DELETE, &format!("/api/v1/endpoints/{id}/captures")),
        )
        .await;
        assert_eq!(reply.status, StatusCode::GONE);
        let reply = send(
            &app,
            bodiless(
                Method::DELETE,
                &format!("/api/v1/endpoints/{id}/captures/{capture}"),
            ),
        )
        .await;
        assert_eq!(reply.status, StatusCode::GONE);
    }
}

#[tokio::test]
async fn unknown_captures_and_malformed_ids_answer_404() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let capture = insert(&db, &id, Spec::new("POST", "/known")).await;
    let unknown = CaptureId::generate()
        .expect("an id is generated")
        .to_string();
    let reply = get(&app, &format!("/api/v1/endpoints/{id}/captures/{unknown}")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "capture_not_found");
    let reply = send(
        &app,
        bodiless(
            Method::DELETE,
            &format!("/api/v1/endpoints/{id}/captures/{unknown}"),
        ),
    )
    .await;
    assert_eq!(reply.json()["error"]["code"], "capture_not_found");
    let reply = get(
        &app,
        &format!("/api/v1/endpoints/{id}/captures/not-a-capture"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "not_found");
    let reply = get(&app, "/api/v1/endpoints/not-an-endpoint/captures").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "not_found");
    assert_eq!(
        get(&app, &format!("/api/v1/endpoints/{id}/captures/{capture}"))
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_capture_of_another_endpoint_is_not_found() {
    let Harness { db, app } = harness().await;
    let owner = endpoint_id(&app).await;
    let other = endpoint_id(&app).await;
    let capture = insert(&db, &owner, Spec::new("POST", "/mine")).await;
    let reply = get(
        &app,
        &format!("/api/v1/endpoints/{other}/captures/{capture}"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = send(
        &app,
        bodiless(
            Method::DELETE,
            &format!("/api/v1/endpoints/{other}/captures/{capture}"),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let still = get(
        &app,
        &format!("/api/v1/endpoints/{owner}/captures/{capture}"),
    )
    .await;
    assert_eq!(still.status, StatusCode::OK);
}

#[tokio::test]
async fn detail_keeps_the_full_path_while_the_list_truncates_it() {
    let Harness { db, app } = harness().await;
    let id = endpoint_id(&app).await;
    let path = format!("/{}", "p".repeat(1500));
    let capture = insert(
        &db,
        &id,
        Spec {
            path: path.clone(),
            ..Spec::new("GET", "/")
        },
    )
    .await;
    let detail = get(&app, &format!("/api/v1/endpoints/{id}/captures/{capture}"))
        .await
        .json();
    assert_eq!(detail["path"], path.as_str());
    let page = get(&app, &format!("/api/v1/endpoints/{id}/captures"))
        .await
        .json();
    assert_eq!(page["items"][0]["path"].as_str().map(str::len), Some(1024));
}
