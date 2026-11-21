mod support;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method};
use support::{Reply, TestServer, client, client_with, get, loopback_config};
use whl_forward::{
    ForwardError, GuardConfig, OutboundRequest, RefusalReason, StaticLookup, validate_target_url,
};

#[tokio::test]
async fn truncates_a_large_body_without_reading_it_all() {
    const TOTAL: usize = 256 * 1024 * 1024;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Flood { total: TOTAL }).await;
    let config = GuardConfig {
        max_response_bytes: 1024,
        ..loopback_config()
    };
    let response = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap();

    assert_eq!(response.status.as_u16(), 200);
    assert_eq!(response.body.len(), 1024);
    assert!(response.truncated);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(server.flooded_bytes() < TOTAL / 8);
}

#[tokio::test]
async fn reports_truncation_only_when_the_body_exceeds_the_cap() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok(&"a".repeat(100))).await;
    for (cap, truncated) in [(100, false), (99, true), (101, false), (0, true)] {
        let config = GuardConfig {
            max_response_bytes: cap,
            ..loopback_config()
        };
        let response = client(config.clone())
            .send(get(&server.url("/"), &config))
            .await
            .unwrap();
        assert_eq!(response.truncated, truncated, "cap {cap}");
        assert_eq!(response.body.len(), cap.min(100), "cap {cap}");
    }
}

#[tokio::test]
async fn fails_with_hop_timeout_when_the_server_never_answers() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Stall).await;
    let config = GuardConfig {
        hop_timeout: Duration::from_millis(400),
        total_timeout: Duration::from_millis(800),
        ..loopback_config()
    };
    let started = Instant::now();
    let error = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap_err();

    assert_eq!(error, ForwardError::HopTimeout);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn hop_timeout_also_covers_a_body_that_stalls() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::StallAfterHead).await;
    let config = GuardConfig {
        hop_timeout: Duration::from_millis(400),
        ..loopback_config()
    };
    let error = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap_err();

    assert_eq!(error, ForwardError::HopTimeout);
}

#[tokio::test]
async fn fails_with_total_timeout_when_several_hops_exhaust_the_overall_budget() {
    let server = TestServer::start("127.0.0.1:0", |_| {
        Reply::redirect(302, "/next").after(Duration::from_millis(300))
    })
    .await;
    let config = GuardConfig {
        hop_timeout: Duration::from_secs(60),
        total_timeout: Duration::from_millis(700),
        ..loopback_config()
    };
    let started = Instant::now();
    let error = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap_err();

    assert_eq!(error, ForwardError::TotalTimeout);
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(server.seen().len() >= 2);
}

#[tokio::test]
async fn reports_connect_timeout_when_name_resolution_outlasts_it() {
    let answer: [IpAddr; 1] = ["127.0.0.1".parse().unwrap()];
    let lookup = Arc::new(
        StaticLookup::new()
            .with_answer("slow.test", &answer)
            .with_delay(Duration::from_secs(120)),
    );
    let config = GuardConfig {
        connect_timeout: Duration::from_millis(200),
        hop_timeout: Duration::from_secs(60),
        ..loopback_config()
    };
    let started = Instant::now();
    let error = client_with(config.clone(), lookup)
        .send(get("http://slow.test:9/", &config))
        .await
        .unwrap_err();

    assert_eq!(error, ForwardError::ConnectTimeout);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn connects_only_to_vetted_addresses_of_a_mixed_answer() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("vetted")).await;
    let answer: [IpAddr; 2] = ["10.0.0.5".parse().unwrap(), "127.0.0.1".parse().unwrap()];
    let lookup = Arc::new(StaticLookup::new().with_answer("mixed.test", &answer));
    let config = GuardConfig {
        connect_timeout: Duration::from_secs(60),
        hop_timeout: Duration::from_secs(60),
        total_timeout: Duration::from_secs(120),
        ..loopback_config()
    };
    let url = format!("http://mixed.test:{}/", server.addr.port());
    let started = Instant::now();
    let response = client_with(config.clone(), lookup)
        .send(get(&url, &config))
        .await
        .unwrap();

    assert_eq!(response.body.as_ref(), b"vetted");
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn refuses_a_host_that_resolves_only_to_refused_addresses_without_connecting() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("must not be reached")).await;
    let answer: [IpAddr; 2] = [
        "169.254.169.254".parse().unwrap(),
        "::ffff:10.0.0.5".parse().unwrap(),
    ];
    let lookup = Arc::new(StaticLookup::new().with_answer("internal.test", &answer));
    let config = loopback_config();
    let url = format!("http://internal.test:{}/", server.addr.port());
    let error = client_with(config.clone(), Arc::clone(&lookup))
        .send(get(&url, &config))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ForwardError::TargetRefused(RefusalReason::RefusedAddress)
    );
    assert_eq!(lookup.calls(), 1);
    assert_eq!(server.connections(), 0);
}

#[tokio::test]
async fn reports_a_dns_failure_for_an_unknown_host() {
    let config = loopback_config();
    let error = client(config.clone())
        .send(get("http://unknown.test/", &config))
        .await
        .unwrap_err();

    assert!(matches!(error, ForwardError::DnsFailure(_)), "{error:?}");
}

#[tokio::test]
async fn sends_the_configured_user_agent_and_strips_managed_headers() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("done")).await;
    let config = loopback_config();
    let guarded = client(config.clone());
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("evil.example"));
    headers.insert("connection", HeaderValue::from_static("upgrade"));
    headers.insert("upgrade", HeaderValue::from_static("websocket"));
    headers.insert("x-custom", HeaderValue::from_static("kept"));
    let request = OutboundRequest::new(
        Method::POST,
        validate_target_url(&server.url("/"), &config).unwrap(),
        headers,
        Bytes::from_static(b"payload"),
    );
    guarded.send(request).await.unwrap();

    let mut overridden = HeaderMap::new();
    overridden.insert("user-agent", HeaderValue::from_static("custom/1"));
    let request = OutboundRequest::new(
        Method::GET,
        validate_target_url(&server.url("/"), &config).unwrap(),
        overridden,
        Bytes::new(),
    );
    guarded.send(request).await.unwrap();

    let seen = server.seen();
    let expected_host = server.addr.to_string();
    assert_eq!(seen[0].header("host"), Some(expected_host.as_str()));
    assert_eq!(
        seen[0].header("user-agent"),
        Some(config.user_agent.as_str())
    );
    assert_eq!(seen[0].header("x-custom"), Some("kept"));
    assert_eq!(seen[0].header("upgrade"), None);
    assert_eq!(seen[0].header("content-length"), Some("7"));
    assert_eq!(seen[1].header("user-agent"), Some("custom/1"));
}

#[tokio::test]
async fn vets_host_names_by_address_when_the_allow_list_is_not_empty() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("vetted by address")).await;
    let allowed: [IpAddr; 1] = ["127.0.0.1".parse().unwrap()];
    let refused: [IpAddr; 1] = ["10.0.0.5".parse().unwrap()];
    let lookup = Arc::new(
        StaticLookup::new()
            .with_answer("app.internal", &allowed)
            .with_answer("db.internal", &refused),
    );
    let config = loopback_config();
    let guarded = client_with(config.clone(), lookup);
    let port = server.addr.port();

    let response = guarded
        .send(get(&format!("http://app.internal:{port}/"), &config))
        .await
        .unwrap();
    assert_eq!(response.body.as_ref(), b"vetted by address");

    let error = guarded
        .send(get(&format!("http://db.internal:{port}/"), &config))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ForwardError::TargetRefused(RefusalReason::RefusedAddress)
    );
    assert_eq!(server.seen().len(), 1);
}
