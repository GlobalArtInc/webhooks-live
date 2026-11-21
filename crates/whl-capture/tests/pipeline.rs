mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::routing::get;
use common::{FixedSource, Harness, PUBLIC_URL, request, request_from};
use time::macros::datetime;
use tower::ServiceExt;
use whl_core::capture_id::CaptureId;

#[tokio::test]
async fn get_without_a_body_is_captured() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;

    let reply = harness.get(&format!("/{id}/orders/7?x=1")).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "");
    assert_eq!(reply.header("x-endpoint-id"), Some(id.unpack()));
    let request_id = reply.header("x-request-id").expect("request id header");

    let rows = harness.rows(&id).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!((row.seq, row.method.as_str()), (1, "GET"));
    assert_eq!(row.id, request_id);
    assert_eq!(row.path, "/orders/7");
    assert_eq!(row.body_size, 0);
    assert!(row.body.is_empty());
    assert_eq!(row.response_status, 200);
    assert_eq!(row.client_ip, "198.51.100.10");
    assert!(row.form.is_none());
}

#[tokio::test]
async fn every_method_is_captured_on_the_bare_id() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let methods = [
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "PURGE",
    ];
    for method in methods {
        let reply = harness
            .send(
                request(method, &format!("/{id}"))
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await;
        assert_eq!(reply.status, 200, "{method}");
    }
    let stored: Vec<_> = harness
        .rows(&id)
        .await
        .into_iter()
        .map(|row| row.method)
        .collect();
    assert_eq!(stored, methods);
}

#[tokio::test]
async fn trailing_slash_is_captured_with_the_slash_as_path() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    assert_eq!(harness.get(&format!("/{id}/")).await.status, 200);
    assert_eq!(harness.rows(&id).await[0].path, "/");
}

#[tokio::test]
async fn post_with_json_keeps_the_exact_bytes_and_content_type() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-type", "application/json")
                .header("content-encoding", "gzip")
                .body(Body::from("{\"order\": 42}"))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    assert_eq!(row.body, b"{\"order\": 42}");
    assert_eq!(row.body_size, 13);
    assert_eq!(row.content_type.as_deref(), Some("application/json"));
    assert!(row.form.is_none());
    assert!(row.search_text.contains("{\"order\": 42}"));
}

#[tokio::test]
async fn empty_post_is_captured_with_zero_size() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness.post(&format!("/{id}"), "").await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    assert_eq!((row.body_size, row.method.as_str()), (0, "POST"));
}

#[tokio::test]
async fn urlencoded_form_is_parsed_into_fields() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Ada+Lovelace&tag=a&tag=b%26c"))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    let form = row.form.as_ref().expect("form is parsed");
    assert_eq!(
        form["fields"],
        serde_json::json!([
            {"name": "name", "value": "Ada Lovelace"},
            {"name": "tag", "value": "a"},
            {"name": "tag", "value": "b&c"},
        ])
    );
    assert_eq!(form["files"], serde_json::json!([]));
    assert_eq!(row.body, b"name=Ada+Lovelace&tag=a&tag=b%26c");
}

#[tokio::test]
async fn multipart_keeps_text_fields_and_file_descriptors_only() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let body = "--XX\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n\
                --XX\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"a.bin\"\r\n\
                Content-Type: application/octet-stream\r\n\r\n0123456789\r\n--XX--\r\n";
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-type", "multipart/form-data; boundary=XX")
                .body(Body::from(body))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    let form = row.form.as_ref().expect("form is parsed");
    assert_eq!(
        form["fields"],
        serde_json::json!([{"name": "title", "value": "hello"}])
    );
    assert_eq!(
        form["files"],
        serde_json::json!([{
            "name": "upload",
            "filename": "a.bin",
            "content_type": "application/octet-stream",
            "size": 10
        }])
    );
    assert_eq!(row.body, body.as_bytes());
}

#[tokio::test]
async fn headers_keep_duplicates_and_first_appearance_order() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("x-first", "1")
                .header("x-dup", "a")
                .header("x-second", "2")
                .header("x-dup", "b")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    assert_eq!(row.header_values("x-dup"), ["a", "b"]);
    let names: Vec<_> = row.headers.iter().map(|(name, _)| name.as_str()).collect();
    let position = |wanted: &str| names.iter().position(|name| *name == wanted).expect(wanted);
    assert!(position("x-first") < position("x-dup"));
    assert!(position("x-dup") < position("x-second"));
}

#[tokio::test]
async fn non_utf8_header_values_are_stored_lossily() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("GET", &format!("/{id}"))
                .header(
                    "x-raw",
                    axum::http::HeaderValue::from_bytes(b"caf\xe9").expect("valid"),
                )
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        harness.rows(&id).await[0].header_values("x-raw"),
        ["caf\u{FFFD}"]
    );
}

#[tokio::test]
async fn query_is_stored_in_order_with_duplicates_and_nul_is_scrubbed() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let reply = harness.get(&format!("/{id}?b=2&a=1&b=3&z=%00end")).await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    let flat: Vec<_> = row
        .query
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        flat,
        [("b", "2"), ("a", "1"), ("b", "3"), ("z", "\u{FFFD}end")]
    );
}

#[tokio::test]
async fn status_override_comes_from_the_first_path_segment() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;

    let reply = harness.get(&format!("/{id}/404/orders")).await;
    assert_eq!(reply.status, 404);
    let ignored = harness.get(&format!("/{id}/orders/404")).await;
    assert_eq!(ignored.status, 200);
    let out_of_range = harness.get(&format!("/{id}/600")).await;
    assert_eq!(out_of_range.status, 200);

    let rows = harness.rows(&id).await;
    assert_eq!(
        (rows[0].status_override, rows[0].response_status),
        (Some(404), 404)
    );
    assert_eq!(
        (rows[1].status_override, rows[1].response_status),
        (None, 200)
    );
    assert_eq!(rows[2].status_override, None);
}

#[tokio::test]
async fn malformed_first_segment_answers_404_and_stores_nothing() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let valid = id.unpack().to_owned();
    let cases = [
        valid.to_uppercase(),
        valid[..23].to_owned(),
        format!("{valid}a"),
        format!("{}i", &valid[..23]),
        "api".to_owned(),
    ];
    for case in cases {
        let reply = harness.get(&format!("/{case}")).await;
        assert_eq!(reply.status, 404, "{case}");
        assert_eq!(reply.body, "Not found.");
        assert_eq!(reply.header("x-webhooks-live-error"), Some("malformed_id"));
        assert_eq!(reply.header("content-security-policy"), Some("sandbox"));
        let nested = harness.get(&format!("/{case}/deeper/path")).await;
        assert_eq!(nested.status, 404, "{case} nested");
    }
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn unknown_expired_and_deleted_endpoints_answer_410() {
    let harness = Harness::new().await;
    let unknown = whl_core::endpoint_id::EndpointId::generate().expect("entropy");
    let expired = harness.endpoint().await;
    let deleted = harness.endpoint().await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '8 days' WHERE id = $1")
        .bind(expired.unpack())
        .execute(harness.db.pool())
        .await
        .expect("update");
    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(deleted.unpack())
        .execute(harness.db.pool())
        .await
        .expect("update");

    for id in [&unknown, &expired, &deleted] {
        let reply = harness.post(&format!("/{id}"), "payload").await;
        assert_eq!(reply.status, 410, "{id}");
        assert_eq!(reply.header("x-webhooks-live-error"), Some("endpoint_gone"));
        assert_eq!(reply.header("x-endpoint-id"), Some(id.unpack()));
        assert_eq!(
            reply.body,
            format!(
                "This endpoint does not exist or has expired. Create a new one at {PUBLIC_URL}."
            )
        );
    }
    assert!(harness.rows(&expired).await.is_empty());
    assert!(harness.rows(&deleted).await.is_empty());
}

#[tokio::test]
async fn a_capture_extends_the_sliding_expiry() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '6 days' WHERE id = $1")
        .bind(id.unpack())
        .execute(harness.db.pool())
        .await
        .expect("update");
    assert_eq!(harness.get(&format!("/{id}")).await.status, 200);
    let age: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM now() - last_active_at)::float8 FROM endpoints WHERE id = $1",
    )
    .bind(id.unpack())
    .fetch_one(harness.db.pool())
    .await
    .expect("age reads");
    assert!(age < 60.0, "last_active_at was refreshed, age {age}");
}

#[tokio::test]
async fn untrusted_peer_cannot_spoof_forwarded_headers() {
    let harness = Harness::with_proxies(&["10.0.0.0/8"]).await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("GET", &format!("/{id}"))
                .header("host", "hooks.local")
                .header("x-forwarded-for", "1.1.1.1")
                .header("x-forwarded-proto", "https")
                .header("x-forwarded-host", "evil.example")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    assert_eq!(row.client_ip, "198.51.100.10");
    assert_eq!(row.host, "hooks.local");
    assert_eq!(row.url, format!("http://hooks.local/{id}"));
}

#[tokio::test]
async fn trusted_peer_chain_resolves_client_ip_scheme_host_and_url() {
    let harness = Harness::with_proxies(&["10.0.0.0/8"]).await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request_from("POST", &format!("/{id}/hook?a=1"), "10.0.0.7:9000")
                .header("host", "internal.svc")
                .header("x-forwarded-for", "6.6.6.6, 203.0.113.50, 10.1.1.1")
                .header("x-forwarded-proto", "https")
                .header("x-forwarded-host", "hooks.example.com")
                .header("forwarded", "for=9.9.9.9;proto=ftp;host=ignored.example")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    let row = &harness.rows(&id).await[0];
    assert_eq!(row.client_ip, "203.0.113.50");
    assert_eq!(row.host, "hooks.example.com");
    assert_eq!(row.url, format!("https://hooks.example.com/{id}/hook?a=1"));
}

#[tokio::test]
async fn injected_source_supplies_the_capture_id_and_time() {
    let fixed = CaptureId::parse("0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b").expect("valid id");
    let at = datetime!(2026-11-21 10:16:02.511 UTC);
    let harness = Harness::new()
        .await
        .with_source(Arc::new(FixedSource { id: fixed, at }));
    let id = harness.endpoint().await;
    let reply = harness.get(&format!("/{id}")).await;
    assert_eq!(
        reply.header("x-request-id"),
        Some("0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b")
    );
    assert_eq!(harness.rows(&id).await[0].received_at, at);
}

#[tokio::test]
async fn static_routes_keep_precedence_and_other_roots_are_not_captured() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let app = Router::new()
        .route("/api/v1/instance", get(|| async { "instance" }))
        .merge(whl_capture::router(harness.state.clone()));

    let instance = app
        .clone()
        .oneshot(
            request("GET", "/api/v1/instance")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("infallible");
    assert_eq!(instance.status(), 200);
    let bytes = axum::body::to_bytes(instance.into_body(), usize::MAX)
        .await
        .expect("body reads");
    assert_eq!(&bytes[..], b"instance");

    for path in ["/api/v1/other", "/e/abc", "/settings", "/assets/app.js"] {
        let reply = common::send(
            &app,
            request("GET", path)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
        assert_eq!(reply.status, 404, "{path}");
    }
    assert!(harness.rows(&id).await.is_empty());
}
