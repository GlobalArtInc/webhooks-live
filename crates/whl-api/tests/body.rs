mod common;

use axum::Router;
use axum::http::{HeaderMap, Method, StatusCode};
use common::{Harness, UNKNOWN_ID, bodiless, create, harness, send};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_testkit::TestDb;

const SAFE_HEADERS: [(&str, &str); 5] = [
    ("cache-control", "no-store"),
    ("content-security-policy", "sandbox; default-src 'none'"),
    ("cross-origin-resource-policy", "same-origin"),
    ("x-content-type-options", "nosniff"),
    ("content-disposition", "inline"),
];

struct Seeded {
    endpoint: String,
    capture: String,
}

async fn endpoint(app: &Router) -> String {
    create(app, "{}").await["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

async fn seed(db: &TestDb, endpoint: &str, content_type: Option<&str>, body: &[u8]) -> String {
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
         client_ip, headers, query, content_type, body_size, stored_bytes, response_status) \
         VALUES ($1, $2, $3, $4, 'POST', 'https://whl.test/x', '', 'whl.test', \
         '203.0.113.7'::inet, '[]'::jsonb, '[]'::jsonb, $5, $6, $7, 200)",
    )
    .bind(endpoint)
    .bind(seq)
    .bind(capture.unpack())
    .bind(OffsetDateTime::now_utc())
    .bind(content_type)
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

async fn seeded(harness: &Harness, content_type: Option<&str>, body: &[u8]) -> Seeded {
    let endpoint = endpoint(&harness.app).await;
    let capture = seed(&harness.db, &endpoint, content_type, body).await;
    Seeded { endpoint, capture }
}

fn body_uri(seeded: &Seeded, query: &str) -> String {
    format!(
        "/api/v1/endpoints/{}/captures/{}/body{query}",
        seeded.endpoint, seeded.capture
    )
}

fn sorted(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

fn expected(content_type: &str, length: usize, disposition: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = SAFE_HEADERS
        .iter()
        .filter(|(name, _)| *name != "content-disposition")
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    pairs.push(("content-type".to_owned(), content_type.to_owned()));
    pairs.push(("content-length".to_owned(), length.to_string()));
    pairs.push(("content-disposition".to_owned(), disposition.to_owned()));
    pairs.sort();
    pairs
}

fn binary() -> Vec<u8> {
    (0..=255_u8).cycle().take(70_000).collect()
}

#[tokio::test]
async fn default_view_is_octet_stream_with_the_exact_header_set() {
    let harness = harness().await;
    let data = binary();
    let seeded = seeded(&harness, Some("application/json"), &data).await;

    let reply = send(&harness.app, bodiless(Method::GET, &body_uri(&seeded, ""))).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, data);
    assert_eq!(
        sorted(&reply.headers),
        expected("application/octet-stream", data.len(), "inline")
    );
}

#[tokio::test]
async fn text_view_is_plain_text_with_the_exact_header_set() {
    let harness = harness().await;
    let seeded = seeded(&harness, Some("application/json"), "h\u{e9}llo".as_bytes()).await;

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &body_uri(&seeded, "?view=text")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, "h\u{e9}llo".as_bytes());
    assert_eq!(
        sorted(&reply.headers),
        expected("text/plain; charset=utf-8", 6, "inline")
    );
}

#[tokio::test]
async fn download_adds_an_attachment_disposition_with_a_safe_name() {
    let harness = harness().await;
    let seeded = seeded(&harness, None, b"abc").await;
    let disposition = format!("attachment; filename=\"{}.bin\"", seeded.capture);

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &body_uri(&seeded, "?download=1")),
    )
    .await;
    assert_eq!(
        sorted(&reply.headers),
        expected("application/octet-stream", 3, &disposition)
    );

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &body_uri(&seeded, "?download=1&view=text")),
    )
    .await;
    assert_eq!(
        sorted(&reply.headers),
        expected("text/plain; charset=utf-8", 3, &disposition)
    );

    for query in [
        "?download=0",
        "?download=true",
        "?download=",
        "?download=yes",
    ] {
        let reply = send(
            &harness.app,
            bodiless(Method::GET, &body_uri(&seeded, query)),
        )
        .await;
        assert_eq!(reply.headers["content-disposition"], "inline", "{query}");
    }
}

#[tokio::test]
async fn unknown_view_values_fall_back_to_octet_stream() {
    let harness = harness().await;
    let seeded = seeded(&harness, Some("text/html"), b"<b>x</b>").await;

    for query in ["?view=html", "?view=TEXT", "?view=", "?view=text/html"] {
        let reply = send(
            &harness.app,
            bodiless(Method::GET, &body_uri(&seeded, query)),
        )
        .await;
        assert_eq!(
            reply.headers["content-type"], "application/octet-stream",
            "{query}"
        );
    }
}

#[tokio::test]
async fn active_content_is_never_served_with_an_active_type() {
    let harness = harness().await;
    let cases = [
        (
            "text/html",
            "<html><script>alert(document.cookie)</script></html>",
        ),
        ("text/html; charset=utf-8", "<img src=x onerror=alert(1)>"),
        (
            "image/svg+xml",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"><script>1</script></svg>",
        ),
        (
            "application/xhtml+xml",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"/>",
        ),
        ("text/javascript", "alert(1)"),
        ("text/html\"; <script>alert(1)</script>", "<b>"),
    ];
    for (captured, body) in cases {
        let seeded = seeded(&harness, Some(captured), body.as_bytes()).await;
        for query in ["", "?view=text", "?download=1", "?view=text&download=1"] {
            let reply = send(
                &harness.app,
                bodiless(Method::GET, &body_uri(&seeded, query)),
            )
            .await;
            assert_eq!(reply.status, StatusCode::OK);
            assert_eq!(reply.body, body.as_bytes());
            let content_type = reply.headers["content-type"].to_str().expect("ascii");
            assert!(
                content_type == "application/octet-stream"
                    || content_type == "text/plain; charset=utf-8",
                "{captured} {query}: {content_type}"
            );
            assert_eq!(reply.headers["x-content-type-options"], "nosniff");
            assert_eq!(
                reply.headers["content-security-policy"],
                "sandbox; default-src 'none'"
            );
            let all_values: String = reply
                .headers
                .values()
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                .collect();
            assert!(!all_values.contains("html"), "{captured} {query}");
            assert!(!all_values.contains("svg"), "{captured} {query}");
            assert!(!all_values.contains("script"), "{captured} {query}");
        }
    }
}

#[tokio::test]
async fn head_answers_the_get_headers_without_a_body() {
    let harness = harness().await;
    let data = binary();
    let seeded = seeded(&harness, None, &data).await;

    let get = send(&harness.app, bodiless(Method::GET, &body_uri(&seeded, ""))).await;
    let head = send(&harness.app, bodiless(Method::HEAD, &body_uri(&seeded, ""))).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(sorted(&head.headers), sorted(&get.headers));
    assert_eq!(head.headers["content-length"], data.len().to_string());
}

#[tokio::test]
async fn an_empty_body_answers_200_with_a_zero_length() {
    let harness = harness().await;
    let seeded = seeded(&harness, None, b"").await;

    let reply = send(&harness.app, bodiless(Method::GET, &body_uri(&seeded, ""))).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.is_empty());
    assert_eq!(
        sorted(&reply.headers),
        expected("application/octet-stream", 0, "inline")
    );
}

#[tokio::test]
async fn unknown_expired_and_deleted_endpoints_answer_410() {
    let harness = harness().await;
    let live = seeded(&harness, None, b"x").await;
    let expired = seeded(&harness, None, b"x").await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '8 days' WHERE id = $1")
        .bind(&expired.endpoint)
        .execute(harness.db.pool())
        .await
        .expect("the endpoint is aged");
    let deleted = seeded(&harness, None, b"x").await;
    send(
        &harness.app,
        bodiless(
            Method::DELETE,
            &format!("/api/v1/endpoints/{}", deleted.endpoint),
        ),
    )
    .await;
    let unknown = Seeded {
        endpoint: UNKNOWN_ID.to_owned(),
        capture: live.capture.clone(),
    };

    for target in [&unknown, &expired, &deleted] {
        let reply = send(&harness.app, bodiless(Method::GET, &body_uri(target, ""))).await;
        assert_eq!(reply.status, StatusCode::GONE);
        assert_eq!(reply.json()["error"]["code"], "endpoint_gone");
        assert!(
            reply.headers["content-type"]
                .to_str()
                .expect("ascii")
                .starts_with("application/json")
        );
    }
}

#[tokio::test]
async fn missing_foreign_and_malformed_captures_answer_404() {
    let harness = harness().await;
    let first = seeded(&harness, None, b"one").await;
    let second = seeded(&harness, None, b"two").await;
    let missing = CaptureId::generate()
        .expect("an id is generated")
        .to_string();

    let cases = [
        (
            Seeded {
                endpoint: first.endpoint.clone(),
                capture: missing,
            },
            "capture_not_found",
        ),
        (
            Seeded {
                endpoint: first.endpoint.clone(),
                capture: second.capture.clone(),
            },
            "capture_not_found",
        ),
        (
            Seeded {
                endpoint: first.endpoint.clone(),
                capture: "not-a-uuid".to_owned(),
            },
            "not_found",
        ),
        (
            Seeded {
                endpoint: "short".to_owned(),
                capture: first.capture.clone(),
            },
            "not_found",
        ),
    ];
    for (target, code) in cases {
        let reply = send(&harness.app, bodiless(Method::GET, &body_uri(&target, ""))).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_eq!(reply.json()["error"]["code"], code);
    }
}

#[tokio::test]
async fn reading_the_body_extends_the_endpoint_lifetime() {
    let harness = harness().await;
    let seeded = seeded(&harness, None, b"x").await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '2 hours' WHERE id = $1")
        .bind(&seeded.endpoint)
        .execute(harness.db.pool())
        .await
        .expect("the endpoint is aged");

    let reply = send(&harness.app, bodiless(Method::GET, &body_uri(&seeded, ""))).await;
    assert_eq!(reply.status, StatusCode::OK);
    let age: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM now() - last_active_at)::float8 FROM endpoints WHERE id = $1",
    )
    .bind(&seeded.endpoint)
    .fetch_one(harness.db.pool())
    .await
    .expect("the age is read");
    assert!(age < 60.0, "{age}");
}
