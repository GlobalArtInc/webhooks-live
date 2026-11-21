mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use common::matrix::error_code;
use common::{
    Actor, RETENTION, Reply, SsoHarness, api_settings, field_codes, harness, mixed_harness,
    mixed_harness_with_api, mixed_harness_with_import_cooldown, send, sso_harness,
};
use futures_util::stream;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::captures::{CapturePolicy, NameValue, NewCapture};

const BASE: i64 = 1_700_000_000;
const JSON: &str = "application/json";
const NDJSON: &str = "application/x-ndjson";
const NEW: &str = "/api/v1/endpoints/import";

async fn seed(harness: &SsoHarness, endpoint: &str, count: usize) {
    seed_padded(harness, endpoint, count, 0).await;
}

async fn seed_padded(harness: &SsoHarness, endpoint: &str, count: usize, padding: usize) {
    let policy = CapturePolicy {
        max_captures: 500,
        retention: RETENTION,
    };
    for index in 0..count {
        let mut body: Vec<u8> = if index == 1 {
            vec![0xff, 0xfe, 0x00, 0x41]
        } else {
            let mut text = b"hello".to_vec();
            text.resize(5 + padding, b'a');
            text
        };
        body.shrink_to_fit();
        let offset = i64::try_from(index).expect("the index fits");
        let capture = NewCapture {
            id: CaptureId::generate().expect("an id is generated"),
            endpoint_id: EndpointId::parse(endpoint).expect("a valid endpoint id"),
            received_at: OffsetDateTime::from_unix_timestamp(BASE + offset)
                .expect("a valid timestamp"),
            method: "POST".to_owned(),
            url: format!("https://whl.test/{endpoint}/item-{index}?a=1"),
            path: format!("/item-{index}"),
            query_string: "a=1".to_owned(),
            host: "whl.test".to_owned(),
            client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
            headers: vec![NameValue {
                name: "x-test".to_owned(),
                value: format!("value-{index}"),
            }],
            query: vec![NameValue {
                name: "a".to_owned(),
                value: "1".to_owned(),
            }],
            form: None,
            content_type: Some("text/plain".to_owned()),
            body: Bytes::from(body),
            status_override: None,
        };
        harness
            .store
            .store_capture(capture, policy)
            .await
            .expect("the capture is stored");
    }
}

async fn seed_email(harness: &SsoHarness, endpoint: &str) {
    let id = CaptureId::generate().expect("an id is generated");
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, form, content_type, body_size, stored_bytes, status_override, \
         response_status, kind, meta) VALUES ($1, 100, $2, $3, 'EMAIL', 'mailto:test', '/', \
         'whl.test', '203.0.113.9'::inet, '[]'::jsonb, '[]'::jsonb, NULL, 'text/plain', 4, 100, \
         NULL, 200, 'email', '{\"subject\":\"Test subject\"}'::jsonb)",
    )
    .bind(endpoint)
    .bind(id.unpack())
    .bind(OffsetDateTime::from_unix_timestamp(BASE + 100).expect("a valid timestamp"))
    .execute(harness.db.pool())
    .await
    .expect("the capture is inserted");
    sqlx::query(
        "INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, 'mail', '')",
    )
    .bind(id.unpack())
    .execute(harness.db.pool())
    .await
    .expect("the body is inserted");
}

async fn export_text(harness: &SsoHarness, owner: &Actor, endpoint: &str, format: &str) -> String {
    let reply = harness
        .send_as(
            Some(owner),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/export?format={format}"),
            "",
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    String::from_utf8(reply.body).expect("the export is UTF-8")
}

fn captures_of(text: &str) -> Vec<Value> {
    let document: Value = serde_json::from_str(text).expect("the export is JSON");
    document["captures"]
        .as_array()
        .expect("the captures are listed")
        .clone()
}

fn without_identity(mut capture: Value) -> Value {
    let object = capture.as_object_mut().expect("a capture is an object");
    for key in ["id", "seq", "meta"] {
        object.remove(key);
    }
    capture
}

fn request(actor: Option<&Actor>, uri: &str, content_type: &str, body: Body) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(actor) = actor {
        builder = builder.header(header::COOKIE, &actor.cookie);
    }
    builder.body(body).expect("the request is valid")
}

async fn import(
    harness: &SsoHarness,
    actor: Option<&Actor>,
    uri: &str,
    content_type: &str,
    text: &str,
) -> Reply {
    send(
        &harness.app,
        request(actor, uri, content_type, Body::from(text.to_owned())),
    )
    .await
}

fn existing(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}/import")
}

async fn counts(harness: &SsoHarness, endpoint: &str) -> (i32, i64) {
    sqlx::query_as("SELECT capture_count, last_seq FROM endpoints WHERE id = $1")
        .bind(endpoint)
        .fetch_one(harness.db.pool())
        .await
        .expect("the endpoint row is read")
}

async fn list(harness: &SsoHarness, owner: &Actor, endpoint: &str) -> Value {
    let reply = harness
        .send_as(
            Some(owner),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/captures?order=asc"),
            "",
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.json()
}

#[tokio::test]
async fn an_export_imports_into_a_new_endpoint_with_the_same_content() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 3).await;
    seed_email(&harness, &source).await;
    let exported = export_text(&harness, &owner, &source, "json").await;

    let reply = import(&harness, Some(&owner), NEW, JSON, &exported).await;

    assert_eq!(reply.status, StatusCode::CREATED);
    let report = reply.json();
    assert_eq!(report["imported"], 4);
    assert_eq!(report["skipped"], 0);
    assert_eq!(report["reason"], Value::Null);
    let created = report["endpoint_id"]
        .as_str()
        .expect("the new id")
        .to_owned();
    assert_ne!(created, source);
    assert_eq!(
        reply.headers[header::LOCATION],
        format!("/api/v1/endpoints/{created}")
    );

    let again = export_text(&harness, &owner, &created, "json").await;
    let before = captures_of(&exported);
    let after = captures_of(&again);
    assert_eq!(
        before
            .iter()
            .cloned()
            .map(without_identity)
            .collect::<Vec<_>>(),
        after
            .iter()
            .cloned()
            .map(without_identity)
            .collect::<Vec<_>>()
    );
    for (original, copy) in before.iter().zip(&after) {
        assert_ne!(original["id"], copy["id"]);
        assert_eq!(copy["meta"]["original_id"], original["id"]);
    }
    assert_eq!(after[3]["meta"]["subject"], "Test subject");
    let listed = list(&harness, &owner, &created).await;
    let items = listed["items"].as_array().expect("the items are listed");
    assert!(items.iter().all(|item| item["imported"] == true));
    assert_eq!(items[3]["kind"], "email");
}

#[tokio::test]
async fn an_ndjson_export_imports_into_an_existing_endpoint_and_runs_nothing() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 3).await;
    seed_email(&harness, &source).await;
    let target = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "ndjson").await;

    let reply = import(
        &harness,
        Some(&owner),
        &existing(&target),
        NDJSON,
        &exported,
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["imported"], 4);
    assert_eq!(reply.json().get("endpoint_id"), None);
    assert_eq!(counts(&harness, &target).await, (4, 4));
    let listed = list(&harness, &owner, &target).await;
    assert_eq!(
        listed["items"][0]["received_at"],
        "2023-11-14T22:13:20.000Z"
    );
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_jobs")
        .fetch_one(harness.db.pool())
        .await
        .expect("the jobs are counted");
    assert_eq!(jobs, 0);
}

#[tokio::test]
async fn an_import_stops_at_the_capture_cap_and_reports_what_it_skipped() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 5).await;
    let target = harness.endpoint_for(&owner).await;
    sqlx::query("UPDATE endpoints SET max_captures = 3 WHERE id = $1")
        .bind(&target)
        .execute(harness.db.pool())
        .await
        .expect("the cap is set");
    let exported = export_text(&harness, &owner, &source, "json").await;

    let reply = import(&harness, Some(&owner), &existing(&target), JSON, &exported).await;

    assert_eq!(reply.status, StatusCode::OK);
    let report = reply.json();
    assert_eq!(report["imported"], 3);
    assert_eq!(report["skipped"], 2);
    assert_eq!(report["reason"], "capacity");
    assert_eq!(counts(&harness, &target).await, (3, 3));
}

#[tokio::test]
async fn an_import_stops_at_the_storage_cap() {
    let mut settings = api_settings();
    settings.storage_cap_bytes = 1;
    let harness = mixed_harness_with_api(settings).await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 3).await;
    let target = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "json").await;

    let reply = import(&harness, Some(&owner), &existing(&target), JSON, &exported).await;

    assert_eq!(reply.status, StatusCode::OK);
    let report = reply.json();
    assert_eq!(report["imported"], 0);
    assert_eq!(report["skipped"], 3);
    assert_eq!(report["reason"], "storage");
}

#[tokio::test]
async fn a_malformed_capture_is_refused_with_its_position_and_leaves_nothing_behind() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 3).await;
    let target = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "json").await;
    let first = exported
        .find("\"method\":\"POST\"")
        .expect("a first method");
    let second = exported[first + 1..]
        .find("\"method\":\"POST\"")
        .expect("a second method")
        + first
        + 1;
    let broken = format!(
        "{}\"method\":\"PO ST\"{}",
        &exported[..second],
        &exported[second + "\"method\":\"POST\"".len()..]
    );

    let into_new = import(&harness, Some(&owner), NEW, JSON, &broken).await;
    let into_existing = import(&harness, Some(&owner), &existing(&target), JSON, &broken).await;

    for reply in [&into_new, &into_existing] {
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            field_codes(&reply.json()),
            vec![("captures.1.method".to_owned(), "invalid_format".to_owned())]
        );
    }
    assert_eq!(counts(&harness, &target).await, (0, 0));
    let listing = harness
        .send_as(Some(&owner), Method::GET, "/api/v1/endpoints", "")
        .await;
    assert_eq!(
        listing.json()["items"].as_array().map(Vec::len),
        Some(2),
        "the endpoint made for the failed import is gone"
    );
}

#[tokio::test]
async fn only_an_editor_or_owner_may_import_and_outsiders_see_the_endpoint_gone() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let viewer = harness.actor("vie", "viewer@example.test", true).await;
    let editor = harness.actor("edi", "editor@example.test", true).await;
    let outsider = harness.actor("out", "outsider@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;
    for (guest, role) in [(&viewer, "viewer"), (&editor, "editor")] {
        let shared = harness
            .send_as(
                Some(&owner),
                Method::PUT,
                &format!("/api/v1/endpoints/{target}/members"),
                &format!(r#"{{"email":"{}","role":"{role}"}}"#, guest.email),
            )
            .await;
        assert_eq!(shared.status, StatusCode::OK);
    }
    let empty = export_text(&harness, &owner, &target, "json").await;

    let as_viewer = import(&harness, Some(&viewer), &existing(&target), JSON, &empty).await;
    let as_outsider = import(&harness, Some(&outsider), &existing(&target), JSON, &empty).await;
    let as_editor = import(&harness, Some(&editor), &existing(&target), JSON, &empty).await;

    assert_eq!(as_viewer.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&as_viewer), "forbidden");
    assert_eq!(as_outsider.status, StatusCode::GONE);
    assert_eq!(error_code(&as_outsider), "endpoint_gone");
    assert_eq!(as_editor.status, StatusCode::OK);
}

#[tokio::test]
async fn an_anonymous_caller_must_sign_in_to_import() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;
    let empty = export_text(&harness, &owner, &target, "json").await;

    let into_existing = import(&harness, None, &existing(&target), JSON, &empty).await;
    let into_new = import(&harness, None, NEW, JSON, &empty).await;

    for reply in [into_existing, into_new] {
        assert_eq!(reply.status, StatusCode::FORBIDDEN);
        assert_eq!(error_code(&reply), "sign_in_required");
    }
}

#[tokio::test]
async fn importing_is_not_available_without_users() {
    let open = harness().await;
    let reply = send(&open.app, request(None, NEW, JSON, Body::from("{}"))).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&reply), "not_available_in_mode");
}

#[tokio::test]
async fn an_upload_declared_over_the_size_limit_is_refused_before_it_is_read() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;
    let mut oversized = request(Some(&owner), &existing(&target), JSON, Body::empty());
    oversized.headers_mut().insert(
        header::CONTENT_LENGTH,
        (100_u64 * 1024 * 1024 + 1)
            .to_string()
            .parse()
            .expect("a header value"),
    );

    let reply = send(&harness.app, oversized).await;

    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error_code(&reply), "payload_too_large");
}

#[tokio::test]
async fn an_upload_that_is_neither_json_nor_ndjson_is_refused() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;

    let reply = import(
        &harness,
        Some(&owner),
        &existing(&target),
        "application/zip",
        "{}",
    )
    .await;

    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn a_second_import_from_the_same_caller_is_refused_while_the_first_runs() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;
    let uri = existing(&target);
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    let pending = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let slow = request(Some(&owner), &uri, JSON, Body::from_stream(pending));
    let app = harness.app.clone();
    let first = tokio::spawn(async move { send(&app, slow).await });
    sender
        .send(Ok(b"{".to_vec()))
        .await
        .expect("the first chunk is sent");

    let mut refused = None;
    for _ in 0..100 {
        let reply = import(&harness, Some(&owner), &uri, JSON, "{}").await;
        if reply.status == StatusCode::TOO_MANY_REQUESTS {
            refused = Some(reply);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let refused = refused.expect("the second import is refused while the first holds its slot");
    assert_eq!(error_code(&refused), "rate_limited");
    drop(sender);
    let finished = first.await.expect("the first import ends");
    assert_eq!(finished.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn keep_times_false_stamps_the_import_time_and_the_flag_is_validated() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 1).await;
    let target = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "json").await;

    let refused = import(
        &harness,
        Some(&owner),
        &format!("{}?keep_times=maybe", existing(&target)),
        JSON,
        &exported,
    )
    .await;
    let accepted = import(
        &harness,
        Some(&owner),
        &format!("{}?keep_times=false", existing(&target)),
        JSON,
        &exported,
    )
    .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&refused.json()),
        vec![("keep_times".to_owned(), "invalid_format".to_owned())]
    );
    assert_eq!(accepted.status, StatusCode::OK);
    let listed = list(&harness, &owner, &target).await;
    let stamped = listed["items"][0]["received_at"]
        .as_str()
        .expect("a timestamp");
    let stamped = OffsetDateTime::parse(stamped, &Rfc3339).expect("an RFC 3339 timestamp");
    assert!((OffsetDateTime::now_utc() - stamped).abs() < time::Duration::minutes(5));
}

async fn owned_endpoints(harness: &SsoHarness, owner: &Actor) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM endpoints WHERE owner_id = $1 AND deleted_at IS NULL")
        .bind(owner.id)
        .fetch_one(harness.db.pool())
        .await
        .expect("the endpoints are counted")
}

async fn wait_for_endpoints(harness: &SsoHarness, owner: &Actor, wanted: i64) {
    for _ in 0..200 {
        if owned_endpoints(harness, owner).await == wanted {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the owner never reached {wanted} endpoints");
}

#[tokio::test]
async fn a_second_import_within_the_cooldown_is_refused_with_a_retry_after() {
    let harness = mixed_harness_with_import_cooldown(Duration::from_secs(60)).await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let target = harness.endpoint_for(&owner).await;
    let empty = export_text(&harness, &owner, &target, "json").await;

    let first = import(&harness, Some(&owner), &existing(&target), JSON, &empty).await;
    let second = import(&harness, Some(&owner), &existing(&target), JSON, &empty).await;

    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&second), "rate_limited");
    let retry: u64 = second.headers[header::RETRY_AFTER]
        .to_str()
        .expect("a header text")
        .parse()
        .expect("a number of seconds");
    assert!((1..=60).contains(&retry));
}

#[tokio::test]
async fn a_client_that_disconnects_mid_import_leaves_no_new_endpoint_behind() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "json").await;
    let marker = "\"captures\":[";
    let cut = exported.find(marker).expect("the capture list opens") + marker.len();
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(1);
    let pending = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let upload = request(Some(&owner), NEW, JSON, Body::from_stream(pending));
    let app = harness.app.clone();
    let client = tokio::spawn(async move { send(&app, upload).await });
    sender
        .send(Ok(exported.as_bytes()[..cut].to_vec()))
        .await
        .expect("the header is sent");
    wait_for_endpoints(&harness, &owner, 2).await;

    client.abort();
    drop(sender);

    wait_for_endpoints(&harness, &owner, 1).await;
}

#[tokio::test]
async fn an_upload_larger_than_the_json_body_limit_passes_through_the_router() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed_padded(&harness, &source, 1, 300 * 1024).await;
    let exported = export_text(&harness, &owner, &source, "json").await;
    assert!(exported.len() > 256 * 1024);

    let reply = import(&harness, Some(&owner), NEW, JSON, &exported).await;

    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.json()["imported"], 1);
}

#[tokio::test]
async fn the_endpoint_block_is_checked_for_a_new_endpoint_and_ignored_for_an_existing_one() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    let target = harness.endpoint_for(&owner).await;
    let exported = export_text(&harness, &owner, &source, "json").await;
    let broken = exported.replacen("\"overflow\":\"refuse\"", "\"overflow\":\"bogus\"", 1);
    assert_ne!(broken, exported);

    let into_new = import(&harness, Some(&owner), NEW, JSON, &broken).await;
    let into_existing = import(&harness, Some(&owner), &existing(&target), JSON, &broken).await;

    assert_eq!(into_new.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        field_codes(&into_new.json())
            .iter()
            .any(|(field, _)| field == "endpoint.overflow")
    );
    assert_eq!(into_existing.status, StatusCode::OK);
}

#[tokio::test]
async fn an_import_stops_between_batches_when_the_free_space_runs_out() {
    let mut settings = api_settings();
    settings.storage_cap_bytes = 10_000_000;
    let harness = mixed_harness_with_api(settings).await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let source = harness.endpoint_for(&owner).await;
    seed(&harness, &source, 60).await;
    let target = harness.endpoint_for(&owner).await;
    let filler = harness.endpoint_for(&owner).await;
    let used: i64 = sqlx::query_scalar("SELECT bytes_used FROM endpoints WHERE id = $1")
        .bind(&source)
        .fetch_one(harness.db.pool())
        .await
        .expect("the usage is read");
    let each = used / 60 + 54;
    let free = 50 * each + each / 2;
    sqlx::query("UPDATE endpoints SET bytes_used = $2 WHERE id = $1")
        .bind(&filler)
        .bind(10_000_000 - used - free)
        .execute(harness.db.pool())
        .await
        .expect("the filler is sized");
    let exported = export_text(&harness, &owner, &source, "json").await;

    let reply = import(&harness, Some(&owner), &existing(&target), JSON, &exported).await;

    assert_eq!(reply.status, StatusCode::OK);
    let report = reply.json();
    assert_eq!(report["imported"], 50);
    assert_eq!(report["skipped"], 10);
    assert_eq!(report["reason"], "storage");
}
