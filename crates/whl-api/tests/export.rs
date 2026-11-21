mod common;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Response, StatusCode, header};
use common::{Harness, UNKNOWN_ID, bodiless, create, field_codes, harness, request, send};
use serde_json::Value;
use time::OffsetDateTime;
use tower::ServiceExt;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::{ExportSelection, Store};
use whl_testkit::TestDb;

const BASE: i64 = 1_800_000_000;

struct Seed<'a> {
    seq: i64,
    path: &'a str,
    body: &'a [u8],
    kind: &'a str,
    stored_bytes: i32,
}

async fn seed(db: &TestDb, endpoint: &str, spec: Seed<'_>) -> String {
    let capture = CaptureId::generate().expect("an id is generated");
    let received_at = OffsetDateTime::from_unix_timestamp(BASE + spec.seq).expect("a timestamp");
    let url = format!("https://whl.test/{endpoint}{}", spec.path);
    let size = i32::try_from(spec.body.len()).expect("the body fits");
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, form, content_type, body_size, stored_bytes, status_override, \
         response_status, kind) VALUES ($1, $2, $3, $4, 'POST', $5, $6, 'whl.test', \
         '203.0.113.7'::inet, '[{\"name\":\"x-a\",\"value\":\"1\"}]'::jsonb, '[]'::jsonb, NULL, \
         'text/plain', $7, $8, NULL, 202, $9)",
    )
    .bind(endpoint)
    .bind(spec.seq)
    .bind(capture.unpack())
    .bind(received_at)
    .bind(url)
    .bind(spec.path)
    .bind(size)
    .bind(spec.stored_bytes)
    .bind(spec.kind)
    .execute(db.pool())
    .await
    .expect("the capture is inserted");
    sqlx::query("INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, $2, '')")
        .bind(capture.unpack())
        .bind(spec.body)
        .execute(db.pool())
        .await
        .expect("the body is inserted");
    capture.to_string()
}

fn plain<'a>(seq: i64, path: &'a str, body: &'a [u8]) -> Seed<'a> {
    Seed {
        seq,
        path,
        body,
        kind: "http",
        stored_bytes: 100,
    }
}

async fn endpoint(app: &Router) -> String {
    create(app, "{}").await["id"]
        .as_str()
        .expect("the id is a string")
        .to_owned()
}

fn text(body: &[u8]) -> String {
    String::from_utf8(body.to_vec()).expect("the export is UTF-8")
}

#[tokio::test]
async fn each_format_answers_its_content_type_and_a_safe_file_name() {
    let Harness { db: _db, app } = harness().await;
    let id = endpoint(&app).await;
    for (format, content_type, extension) in [
        ("json", "application/json", "json"),
        ("ndjson", "application/x-ndjson", "ndjson"),
        ("csv", "text/csv; charset=utf-8", "csv"),
        ("zip", "application/zip", "zip"),
    ] {
        let uri = format!("/api/v1/endpoints/{id}/export?format={format}");
        let reply = send(&app, bodiless(Method::GET, &uri)).await;
        assert_eq!(reply.status, StatusCode::OK, "{format}");
        assert_eq!(
            reply.headers[header::CONTENT_TYPE],
            content_type,
            "{format}"
        );
        let disposition = reply.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("the disposition is text")
            .to_owned();
        assert!(
            disposition.starts_with(&format!("attachment; filename=\"webhooks-live-{id}-")),
            "{disposition}"
        );
        assert!(
            disposition.ends_with(&format!(".{extension}\"")),
            "{disposition}"
        );
        assert_eq!(reply.headers["x-content-type-options"], "nosniff");
        assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
    }
}

#[tokio::test]
async fn json_document_holds_the_header_and_each_capture_with_its_body() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/first", b"hello")).await;
    seed(&db, &id, plain(2, "/second", b"world")).await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=json"),
        ),
    )
    .await;
    let document: Value = serde_json::from_slice(&reply.body).expect("the export is JSON");
    assert_eq!(document["format"], "webhooks-live-export");
    assert_eq!(document["version"], 1);
    assert_eq!(document["endpoint"]["id"], id.as_str());
    assert_eq!(document["endpoint"]["response"]["status"], 200);
    let captures = document["captures"]
        .as_array()
        .expect("captures is an array");
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0]["seq"], 1);
    assert_eq!(captures[0]["path"], "/first");
    assert_eq!(captures[0]["body"]["encoding"], "utf8");
    assert_eq!(captures[0]["body"]["data"], "hello");
    assert_eq!(captures[1]["body"]["data"], "world");
}

#[tokio::test]
async fn ndjson_writes_the_header_line_then_one_capture_per_line() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/a", b"one")).await;
    seed(&db, &id, plain(2, "/b", b"two")).await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=ndjson"),
        ),
    )
    .await;
    let lines: Vec<Value> = text(&reply.body)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["format"], "webhooks-live-export");
    assert!(lines[0].get("captures").is_none());
    assert_eq!(lines[1]["body"]["data"], "one");
    assert_eq!(lines[2]["seq"], 2);
}

#[tokio::test]
async fn csv_neutralises_formula_cells_and_quotes_cells_with_separators() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/x", b"=HYPERLINK(\"x\"),y")).await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=csv"),
        ),
    )
    .await;
    let csv = text(&reply.body);
    assert!(csv.starts_with("id,seq,kind,received_at,method,path,"));
    assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\"),y\""), "{csv}");
}

#[tokio::test]
async fn zip_names_body_entries_by_sequence_and_id_and_lists_the_index() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    let capture = seed(&db, &id, plain(1, "/a", b"payload")).await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=zip"),
        ),
    )
    .await;
    assert!(reply.body.starts_with(b"PK\x03\x04"));
    let name = format!("bodies/1-{capture}.bin");
    assert!(text_contains(&reply.body, name.as_bytes()));
    assert!(text_contains(&reply.body, b"captures.ndjson"));
}

fn text_contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[tokio::test]
async fn a_selection_exports_only_the_named_captures_in_sequence_order() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    let first = seed(&db, &id, plain(1, "/a", b"a")).await;
    seed(&db, &id, plain(2, "/b", b"b")).await;
    let third = seed(&db, &id, plain(3, "/c", b"c")).await;
    let body = format!("{{\"format\":\"ndjson\",\"capture_ids\":[\"{third}\",\"{first}\"]}}");
    let reply = send(
        &app,
        request(
            Method::POST,
            &format!("/api/v1/endpoints/{id}/export"),
            &body,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let seqs: Vec<i64> = text(&reply.body)
        .lines()
        .skip(1)
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("each line is JSON");
            value["seq"].as_i64().expect("seq is a number")
        })
        .collect();
    assert_eq!(seqs, vec![1, 3]);
}

#[tokio::test]
async fn invalid_selections_answer_422_with_the_field_that_failed() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    let capture = seed(&db, &id, plain(1, "/a", b"a")).await;
    let uri = format!("/api/v1/endpoints/{id}/export");
    let unknown_format = send(
        &app,
        request(
            Method::POST,
            &uri,
            &format!("{{\"format\":\"xml\",\"capture_ids\":[\"{capture}\"]}}"),
        ),
    )
    .await;
    assert_eq!(unknown_format.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&unknown_format.json()),
        vec![("format".to_owned(), "invalid_format".to_owned())]
    );
    let empty = send(
        &app,
        request(
            Method::POST,
            &uri,
            "{\"format\":\"json\",\"capture_ids\":[]}",
        ),
    )
    .await;
    assert_eq!(empty.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&empty.json()),
        vec![("capture_ids".to_owned(), "required".to_owned())]
    );
    let ids: Vec<String> = (0..501)
        .map(|_| "00000000-0000-7000-8000-000000000000".to_owned())
        .collect();
    let too_many = serde_json::json!({"format": "json", "capture_ids": ids}).to_string();
    let too_many = send(&app, request(Method::POST, &uri, &too_many)).await;
    assert_eq!(too_many.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&too_many.json()),
        vec![("capture_ids".to_owned(), "out_of_range".to_owned())]
    );
}

#[tokio::test]
async fn a_selection_above_the_export_size_answers_422_before_streaming() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(
        &db,
        &id,
        Seed {
            seq: 1,
            path: "/huge",
            body: b"x",
            kind: "http",
            stored_bytes: 300 * 1024 * 1024,
        },
    )
    .await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=json"),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.json()["error"]["code"], "export_too_large");
}

#[tokio::test]
async fn the_kind_filter_selects_the_export() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/http", b"h")).await;
    seed(
        &db,
        &id,
        Seed {
            seq: 2,
            path: "/mail",
            body: b"m",
            kind: "email",
            stored_bytes: 100,
        },
    )
    .await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=ndjson&kind=email"),
        ),
    )
    .await;
    let lines: Vec<Value> = text(&reply.body)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["kind"], "email");
}

#[tokio::test]
async fn an_unknown_endpoint_answers_410_for_an_export() {
    let Harness { db: _db, app } = harness().await;
    let reply = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{UNKNOWN_ID}/export?format=json"),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::GONE);
}

async fn large_endpoint(db: &TestDb, app: &Router) -> String {
    let id = endpoint(app).await;
    let payload = vec![b'a'; 128 * 1024];
    for seq in 1..=8 {
        seed(db, &id, plain(seq, "/large", &payload)).await;
    }
    id
}

async fn begin_export(app: &Router, id: &str) -> Response<Body> {
    app.clone()
        .oneshot(bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=json"),
        ))
        .await
        .expect("the router does not fail")
}

#[tokio::test]
async fn a_second_export_while_the_slot_is_busy_answers_429_rate_limited() {
    let Harness { db, app } = harness().await;
    let id = large_endpoint(&db, &app).await;
    let first = begin_export(&app, &id).await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = send(
        &app,
        bodiless(
            Method::GET,
            &format!("/api/v1/endpoints/{id}/export?format=json"),
        ),
    )
    .await;
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.json()["error"]["code"], "rate_limited");
    drop(first);
}

#[tokio::test]
async fn the_slot_is_released_after_the_client_drops_the_body_mid_stream() {
    let Harness { db, app } = harness().await;
    let id = large_endpoint(&db, &app).await;
    let first = begin_export(&app, &id).await;
    assert_eq!(first.status(), StatusCode::OK);
    drop(first);
    let mut released = false;
    for _ in 0..100 {
        let next = begin_export(&app, &id).await;
        if next.status() == StatusCode::OK {
            released = true;
            break;
        }
        assert_eq!(next.status(), StatusCode::TOO_MANY_REQUESTS);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(released, "the slot was never released");
}

#[tokio::test]
async fn a_store_failure_mid_export_ends_the_body_with_an_error() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/a", b"one")).await;
    sqlx::query("DROP TABLE capture_bodies CASCADE")
        .execute(db.pool())
        .await
        .expect("the bodies table is dropped");
    let response = begin_export(&app, &id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let outcome = axum::body::to_bytes(response.into_body(), usize::MAX).await;
    assert!(outcome.is_err());
}

#[tokio::test]
async fn head_on_the_export_answers_405_with_the_allowed_methods() {
    let Harness { db: _db, app } = harness().await;
    let id = endpoint(&app).await;
    let reply = send(
        &app,
        bodiless(Method::HEAD, &format!("/api/v1/endpoints/{id}/export")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.headers[header::ALLOW], "GET, POST");
}

#[tokio::test]
async fn export_pages_stop_at_the_snapshot_sequence() {
    let Harness { db, app } = harness().await;
    let id = endpoint(&app).await;
    seed(&db, &id, plain(1, "/a", b"a")).await;
    seed(&db, &id, plain(2, "/b", b"b")).await;
    let store = Store::from_pool(db.pool().clone());
    let endpoint_id = EndpointId::parse(&id).expect("the id parses");
    let selection = ExportSelection::Filter(Default::default());
    let snapshot = store
        .export_snapshot(&endpoint_id, &selection)
        .await
        .expect("the snapshot is taken");
    assert_eq!(snapshot.last_seq, 2);
    assert_eq!(snapshot.total_bytes, 200);
    seed(&db, &id, plain(3, "/c", b"c")).await;
    let page = store
        .export_page(&endpoint_id, &selection, 0, snapshot.last_seq, 50)
        .await
        .expect("the page is read");
    let seqs: Vec<i64> = page.iter().map(|detail| detail.summary.seq).collect();
    assert_eq!(seqs, vec![1, 2]);
}
