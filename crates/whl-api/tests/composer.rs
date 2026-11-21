mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use common::composer::{
    MockLocal, PEER, captured_logs, composer, error_code, field_code, limits, open_harness,
    post_send, send_request, simple, target,
};
use common::{create, send};
use serde_json::{Value, json};
use tokio::task::JoinSet;
use whl_api::ComposerSettings;

const SELF_ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

fn self_url(path: &str) -> String {
    format!("https://whl.test/{SELF_ID}{path}")
}

#[tokio::test]
async fn an_external_request_is_sent_fresh_and_its_response_comes_back_as_data() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let reply = post_send(
        &harness.app,
        PEER,
        &json!({
            "method": "POST",
            "url": remote.url("/ok?a=1"),
            "headers": [{"name": "X-Custom", "value": "v"}],
            "body": {"encoding": "utf8", "data": "hello"},
        }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    let result = reply.json();
    assert_eq!(result["status"], 200);
    assert_eq!(result["body"]["data"], "pong");
    assert_eq!(result["capture_id"], Value::Null);
    let seen = remote.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, Method::POST);
    assert_eq!(seen[0].path_and_query, "/ok?a=1");
    assert_eq!(seen[0].body, b"hello");
    assert_eq!(seen[0].headers["x-custom"], "v");
    assert_eq!(seen[0].headers["x-webhooks-live-hop"], "1");
    assert!(seen[0].headers.get("x-webhooks-live-composer").is_none());
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn the_callers_own_credentials_are_never_forwarded() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let mut request = send_request(
        PEER,
        Method::POST,
        "/api/v1/send",
        &simple(&remote.url("/ok")).to_string(),
    );
    request.headers_mut().insert(
        header::COOKIE,
        "__Host-whl_session=secret".parse().expect("a value"),
    );
    request.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer caller-token".parse().expect("a value"),
    );

    let reply = send(&harness.app, request).await;

    assert_eq!(reply.status, StatusCode::OK);
    let seen = remote.seen();
    assert!(seen[0].headers.get(header::COOKIE).is_none());
    assert!(seen[0].headers.get(header::AUTHORIZATION).is_none());
}

#[tokio::test]
async fn ssrf_targets_are_refused_before_anything_is_sent() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let cases = [
        ("http://127.0.0.2:9/", "target_refused"),
        ("http://169.254.169.254/latest/meta-data", "target_refused"),
        ("http://localhost/", "target_unresolvable"),
        ("http://[::1]/", "target_refused"),
        ("http://[::ffff:10.0.0.1]/", "target_refused"),
        ("http://0xa.1/", "target_refused"),
        ("http://metadata.internal/", "target_unresolvable"),
        ("https://user:pw@public.test/", "target_refused"),
        ("http://private.test/", "target_refused"),
        ("http://unknown.test/", "target_unresolvable"),
        ("ftp://public.test/", "invalid_format"),
        ("file:///etc/passwd", "invalid_format"),
    ];
    for (url, code) in cases {
        let reply = post_send(&harness.app, PEER, &simple(url)).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(field_code(&reply), code, "{url}");
    }
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn a_redirect_is_returned_as_data_unless_following_is_requested() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let reply = post_send(
        &harness.app,
        PEER,
        &simple(&remote.url("/redirect-private")),
    )
    .await;

    assert_eq!(reply.json()["status"], 302);
    let headers = reply.json()["headers"].to_string();
    assert!(headers.contains("169.254.169.254"), "{headers}");
    assert_eq!(remote.seen().len(), 1);
}

#[tokio::test]
async fn a_followed_redirect_into_a_refused_range_ends_in_a_refusal_error() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let followed = post_send(
        &harness.app,
        PEER,
        &json!({"method": "GET", "url": remote.url("/redirect-private"), "follow_redirects": true}),
    )
    .await;

    assert_eq!(followed.status, StatusCode::OK);
    assert_eq!(followed.json()["status"], Value::Null);
    let error = followed.json()["error"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(error.starts_with("refused_redirect"), "{error}");
    assert_eq!(remote.seen().len(), 1);
}

#[tokio::test]
async fn a_response_over_one_mebibyte_is_cut_and_flagged() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let reply = post_send(&harness.app, PEER, &simple(&remote.url("/big"))).await;

    let body = &reply.json()["body"];
    assert_eq!(body["truncated"], true);
    assert_eq!(body["data"].as_str().map(str::len), Some(1024 * 1024));
}

#[tokio::test]
async fn request_bodies_are_capped_at_one_mebibyte_and_the_route_at_one_and_a_half() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let url = remote.url("/ok");

    let fits = json!({
        "method": "POST", "url": url,
        "body": {"encoding": "utf8", "data": "a".repeat(1024 * 1024)},
    });
    let over = json!({
        "method": "POST", "url": url,
        "body": {"encoding": "utf8", "data": "a".repeat(1024 * 1024 + 1)},
    });
    let huge = json!({
        "method": "POST", "url": url,
        "body": {"encoding": "utf8", "data": "a".repeat(1536 * 1024)},
    });

    assert_eq!(
        post_send(&harness.app, PEER, &fits).await.status,
        StatusCode::OK
    );
    let refused = post_send(&harness.app, PEER, &over).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(field_code(&refused), "too_long");
    let rejected = post_send(&harness.app, PEER, &huge).await;
    assert_eq!(rejected.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(remote.seen().len(), 1);
}

#[tokio::test]
async fn a_slow_target_ends_with_a_timeout_error_inside_the_requested_time() {
    let local = MockLocal::slow(Duration::from_secs(5));
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let started = std::time::Instant::now();
    let reply = post_send(
        &harness.app,
        PEER,
        &json!({"method": "GET", "url": self_url(""), "timeout_ms": 200}),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.json()["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("timeout"))
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn an_own_capture_url_is_handled_in_process_with_the_callers_address() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let reply = post_send(
        &harness.app,
        PEER,
        &json!({
            "method": "POST",
            "url": self_url("/hook?x=1"),
            "headers": [{"name": "X-Custom", "value": "v"}],
            "body": {"encoding": "utf8", "data": "payload"},
        }),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json());
    let result = reply.json();
    assert!(result["capture_id"].is_string());
    assert_eq!(result["body"]["data"], "stored");
    let delivered = local.delivered();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].host, "whl.test");
    assert_eq!(delivered[0].path_and_query, format!("/{SELF_ID}/hook?x=1"));
    assert_eq!(delivered[0].client_ip.to_string(), PEER);
    assert_eq!(delivered[0].headers["x-webhooks-live-composer"], "1");
    assert_eq!(delivered[0].headers["x-webhooks-live-hop"], "1");
    assert_eq!(delivered[0].headers["x-custom"], "v");
    assert_eq!(delivered[0].body.as_ref(), b"payload");
}

#[tokio::test]
async fn the_public_host_is_never_reached_outside_its_capture_paths() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    for url in [
        "https://whl.test/".to_owned(),
        "https://whl.test/api/v1/instance".to_owned(),
        format!("https://whl.test/{SELF_ID}/../api/v1/instance"),
        "https://whl.test/auth/login".to_owned(),
    ] {
        let reply = post_send(&harness.app, PEER, &simple(&url)).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(field_code(&reply), "target_refused", "{url}");
    }
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn an_own_url_without_in_process_delivery_is_refused_not_sent_over_the_network() {
    let harness = open_harness(ComposerSettings::default()).await;

    let reply = post_send(&harness.app, PEER, &simple(&self_url(""))).await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(field_code(&reply), "target_refused");
}

#[tokio::test]
async fn injected_and_managed_headers_are_refused() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    for (name, value) in [
        ("Host", "evil.test"),
        ("X-Forwarded-For", "10.1.1.1"),
        ("X-Webhooks-Live-Composer", "0"),
        ("X-Webhooks-Live-Hop", "0"),
        ("X-Injected", "a\r\nHost: evil.test"),
    ] {
        let reply = post_send(
            &harness.app,
            PEER,
            &json!({
                "method": "GET", "url": self_url(""),
                "headers": [{"name": name, "value": value}],
            }),
        )
        .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{name}");
    }
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn the_endpoint_quota_refuses_with_its_name_and_a_retry_hint() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(2, 100, 100))).await;
    let endpoint = create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let body = json!({"method": "GET", "url": self_url(""), "endpoint_id": endpoint});

    for _ in 0..2 {
        assert_eq!(
            post_send(&harness.app, PEER, &body).await.status,
            StatusCode::OK
        );
    }
    let refused = post_send(&harness.app, PEER, &body).await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&refused), "quota_exceeded");
    assert_eq!(
        refused.json()["error"]["limit"]["name"],
        "composer_endpoint_hour"
    );
    assert_eq!(refused.json()["error"]["limit"]["max"], 2);
    assert!(refused.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(local.delivered().len(), 2);
}

#[tokio::test]
async fn fifty_sends_in_parallel_waves_never_pass_the_network_quota() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 10, 100))).await;
    let body = simple(&self_url(""));

    let mut allowed = 0;
    let mut refused = 0;
    for wave in 0..13 {
        let mut tasks = JoinSet::new();
        for _ in 0..(if wave == 12 { 2 } else { 4 }) {
            let app = harness.app.clone();
            let body = body.clone();
            tasks.spawn(async move { post_send(&app, PEER, &body).await });
        }
        while let Some(reply) = tasks.join_next().await {
            let reply = reply.expect("the task finishes");
            match (reply.status, error_code(&reply).as_str()) {
                (StatusCode::OK, _) => allowed += 1,
                (StatusCode::TOO_MANY_REQUESTS, "quota_exceeded") => refused += 1,
                other => panic!("unexpected answer {other:?}"),
            }
        }
    }

    assert_eq!((allowed, refused), (10, 40));
    assert_eq!(local.delivered().len(), 10);
}

#[tokio::test]
async fn a_zero_limit_blocks_every_send() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 0, 100))).await;

    let reply = post_send(&harness.app, PEER, &simple(&self_url(""))).await;

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn too_many_sends_in_flight_are_refused_instead_of_queued() {
    let local = MockLocal::slow(Duration::from_millis(600));
    let settings = ComposerSettings::new(true, limits(100, 100, 100), 1)
        .with_local(local.clone() as std::sync::Arc<dyn whl_api::LocalDelivery>);
    let harness = open_harness(settings).await;
    let body = simple(&self_url(""));

    let first = {
        let app = harness.app.clone();
        let body = body.clone();
        tokio::spawn(async move { post_send(&app, PEER, &body).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let second = post_send(&harness.app, "203.0.113.8", &body).await;

    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&second), "rate_limited");
    assert_eq!(
        first.await.expect("the task finishes").status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_disabled_composer_does_not_exist() {
    let harness = open_harness(ComposerSettings::new(false, limits(1, 1, 1), 1)).await;

    let reply = post_send(&harness.app, PEER, &simple(&self_url(""))).await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_methods_and_fields_are_refused() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    for body in [
        json!({"method": "CONNECT", "url": self_url("")}),
        json!({"method": "GET", "url": self_url(""), "proxy": "x"}),
        json!({"method": "GET", "url": self_url(""), "timeout_ms": 15001}),
    ] {
        let reply = post_send(&harness.app, PEER, &body).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
}

#[tokio::test]
async fn every_send_logs_who_where_and_how_it_went_without_path_or_query() {
    let captured = captured_logs();
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;

    let reply = post_send(
        &harness.app,
        "203.0.113.99",
        &simple(&self_url("/private-path?token=QUERY-SECRET")),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    let logs = captured.text();
    let line = logs
        .lines()
        .find(|line| line.contains("composer request sent") && line.contains("203.0.113.99"))
        .expect("the send is logged with the caller address");
    assert!(line.contains("whl.test") && line.contains("local") && line.contains("anonymous"));
    assert!(line.contains("200"), "{line}");
    assert!(
        !line.contains("private-path") && !line.contains("QUERY-SECRET"),
        "{line}"
    );
}

#[tokio::test]
async fn dotless_host_names_and_denied_ports_are_refused_for_the_composer() {
    let harness = open_harness(ComposerSettings::new(true, limits(100, 100, 100), 8)).await;
    for url in [
        "http://vault/",
        "http://postgres./x",
        "http://public.test:22/",
        "http://public.test:6379/",
        "https://public.test:5432/",
        "http://8.8.8.8:25/",
    ] {
        let reply = post_send(&harness.app, PEER, &simple(url)).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert_eq!(field_code(&reply), "target_refused", "{url}");
    }
}

#[tokio::test]
async fn the_incoming_hop_count_is_incremented_and_stops_at_three() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let with_hop = |hop: &str| {
        let mut request = send_request(
            PEER,
            Method::POST,
            "/api/v1/send",
            &simple(&remote.url("/ok")).to_string(),
        );
        request
            .headers_mut()
            .insert("x-webhooks-live-hop", hop.parse().expect("a value"));
        request
    };

    let relayed = send(&harness.app, with_hop("2")).await;
    let stopped = send(&harness.app, with_hop("3")).await;

    assert_eq!(relayed.status, StatusCode::OK);
    assert_eq!(remote.seen()[0].headers["x-webhooks-live-hop"], "3");
    assert_eq!(stopped.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&stopped), "hop_limit");
    assert_eq!(remote.seen().len(), 1);
}

#[tokio::test]
async fn one_caller_cannot_hold_more_than_four_sends_in_flight() {
    let local = MockLocal::slow(Duration::from_millis(800));
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let body = simple(&self_url(""));

    let mut tasks = JoinSet::new();
    for _ in 0..5 {
        let app = harness.app.clone();
        let body = body.clone();
        tasks.spawn(async move { post_send(&app, PEER, &body).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let mut ok = 0;
    let mut busy = 0;
    while let Some(reply) = tasks.join_next().await {
        let reply = reply.expect("the task finishes");
        match (reply.status, error_code(&reply).as_str()) {
            (StatusCode::OK, _) => ok += 1,
            (StatusCode::TOO_MANY_REQUESTS, "rate_limited") => busy += 1,
            other => panic!("unexpected answer {other:?}"),
        }
    }
    let after = post_send(&harness.app, PEER, &body).await;

    assert_eq!((ok, busy), (4, 1));
    assert_eq!(after.status, StatusCode::OK);
}

#[tokio::test]
async fn a_stalled_request_body_is_cut_after_ten_seconds() {
    let local = MockLocal::new();
    let harness = open_harness(composer(&local, limits(100, 100, 100))).await;
    let stalled = futures_util::stream::pending::<Result<axum::body::Bytes, std::io::Error>>();
    let mut request = send_request(PEER, Method::POST, "/api/v1/send", "");
    *request.body_mut() = axum::body::Body::from_stream(stalled);

    let reply = send(&harness.app, request).await;

    assert_eq!(reply.status, StatusCode::REQUEST_TIMEOUT);
    assert_eq!(error_code(&reply), "request_timeout");
}
