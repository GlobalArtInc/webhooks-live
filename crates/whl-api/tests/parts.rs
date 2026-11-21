mod common;

use axum::http::{Method, StatusCode};
use common::{Harness, Reply, bodiless, create, harness, send};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_testkit::TestDb;

const MESSAGE: &str = "From: a@example.org\r\nSubject: s\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nhello part\r\n--b1\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b1--\r\n";

struct Seeded {
    endpoint: String,
    capture: String,
}

async fn seed(db: &TestDb, endpoint: &str, body: &[u8]) -> String {
    let capture = CaptureId::generate().expect("an id is generated");
    let size = i64::try_from(body.len()).expect("the body fits");
    let seq: i64 = sqlx::query_scalar(
        "UPDATE endpoints SET last_seq = last_seq + 1, capture_count = capture_count + 1, \
         bytes_used = bytes_used + $2 WHERE id = $1 RETURNING last_seq",
    )
    .bind(endpoint)
    .bind(size + 100)
    .fetch_one(db.pool())
    .await
    .expect("the counters are updated");
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, content_type, body_size, stored_bytes, response_status, kind) \
         VALUES ($1, $2, $3, $4, 'SMTP', 'mailto:x@mail.test', '', 'mail.test', \
         '203.0.113.7'::inet, '[]'::jsonb, '[]'::jsonb, 'multipart/mixed', $5, $6, 250, 'email')",
    )
    .bind(endpoint)
    .bind(seq)
    .bind(capture.unpack())
    .bind(OffsetDateTime::now_utc())
    .bind(i32::try_from(size).expect("the size fits"))
    .bind(i32::try_from(size + 100).expect("the size fits"))
    .execute(db.pool())
    .await
    .expect("the capture is inserted");
    sqlx::query("INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, $2, '')")
        .bind(capture.unpack())
        .bind(body)
        .execute(db.pool())
        .await
        .expect("the body is inserted");
    capture.to_string()
}

async fn seeded() -> (Harness, Seeded) {
    let harness = harness().await;
    let endpoint = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    let capture = seed(&harness.db, &endpoint, MESSAGE.as_bytes()).await;
    (harness, Seeded { endpoint, capture })
}

fn part_uri(seeded: &Seeded, index: &str, query: &str) -> String {
    format!(
        "/api/v1/endpoints/{}/captures/{}/parts/{index}{query}",
        seeded.endpoint, seeded.capture
    )
}

fn header<'a>(reply: &'a Reply, name: &str) -> &'a str {
    reply
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_text_part_is_served_as_octet_stream_by_default_with_the_safe_headers() {
    let (harness, seeded) = seeded().await;

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &part_uri(&seeded, "0", "")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&reply.body).starts_with("hello part"));
    assert_eq!(header(&reply, "content-type"), "application/octet-stream");
    assert_eq!(header(&reply, "x-content-type-options"), "nosniff");
    assert_eq!(
        header(&reply, "content-security-policy"),
        "sandbox; default-src 'none'"
    );
    assert_eq!(header(&reply, "content-disposition"), "inline");
}

#[tokio::test]
async fn the_text_view_serves_a_text_part_as_plain_text_and_never_html() {
    let (harness, seeded) = seeded().await;

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &part_uri(&seeded, "0", "?view=text")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(header(&reply, "content-type"), "text/plain; charset=utf-8");
}

#[tokio::test]
async fn a_binary_part_is_decoded_and_downloaded_under_its_sanitised_name() {
    let (harness, seeded) = seeded().await;

    let reply = send(
        &harness.app,
        bodiless(
            Method::GET,
            &part_uri(&seeded, "1", "?view=text&download=1"),
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, b"%PDF-1.4\n");
    assert_eq!(header(&reply, "content-type"), "application/octet-stream");
    assert_eq!(
        header(&reply, "content-disposition"),
        "attachment; filename=\"report.pdf\""
    );
}

#[tokio::test]
async fn an_unknown_or_malformed_index_is_a_404_with_the_part_code() {
    let (harness, seeded) = seeded().await;

    for index in ["2", "99", "-1", "x"] {
        let reply = send(
            &harness.app,
            bodiless(Method::GET, &part_uri(&seeded, index, "")),
        )
        .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{index}");
        assert_eq!(reply.json()["error"]["code"], "part_not_found", "{index}");
    }
}
