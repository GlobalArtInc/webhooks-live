mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use common::{Reply, harness_with_limits, send};
use whl_api::ApiLimits;
use whl_core::client_ip::TrustedProxies;
use whl_core::limiter::{LimiterSettings, Limiters};
use whl_core::rate::RateSpec;

const CLIENT: &str = "198.51.100.10:4000";
const OTHER_CLIENT: &str = "198.51.100.11:4000";

fn spec(text: &str) -> RateSpec {
    text.parse().expect("valid rate spec")
}

fn limits(api_ip: &str, create_ip: &str, proxies: TrustedProxies) -> ApiLimits {
    let limiters = Limiters::new(&LimiterSettings {
        capture_ip: spec("1000/s:1000"),
        capture_endpoint: spec("1000/s:1000"),
        api_ip: spec(api_ip),
        create_ip: spec(create_ip),
        max_keys: 1000,
    })
    .expect("limiters build");
    ApiLimits::new(limiters, proxies)
}

fn from(method: Method, uri: &str, peer: &str) -> Request<Body> {
    let peer: SocketAddr = peer.parse().expect("valid socket address");
    Request::builder()
        .method(method)
        .uri(uri)
        .extension(ConnectInfo(peer))
        .body(Body::empty())
        .expect("the request is valid")
}

fn number(reply: &Reply, name: &str) -> u32 {
    reply
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("{name} is a number"))
}

#[tokio::test]
async fn every_response_carries_the_limit_and_a_decreasing_remaining_count() {
    let harness =
        harness_with_limits(limits("1/s:5", "100/m:100", TrustedProxies::default())).await;
    let mut remaining = Vec::new();
    for _ in 0..3 {
        let reply = send(&harness.app, from(Method::GET, "/api/v1/instance", CLIENT)).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(number(&reply, "ratelimit-limit"), 5);
        remaining.push(number(&reply, "ratelimit-remaining"));
    }
    assert_eq!(remaining, [4, 3, 2]);

    let unknown = send(&harness.app, from(Method::GET, "/api/v1/nothing", CLIENT)).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(number(&unknown, "ratelimit-remaining"), 1);
}

#[tokio::test]
async fn the_api_limit_answers_429_with_the_json_error_and_retry_after() {
    let harness =
        harness_with_limits(limits("10/s:2", "100/m:100", TrustedProxies::default())).await;
    for _ in 0..2 {
        let reply = send(&harness.app, from(Method::GET, "/api/v1/instance", CLIENT)).await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    let limited = send(&harness.app, from(Method::GET, "/api/v1/instance", CLIENT)).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.json()["error"]["code"], "rate_limited");
    assert!(limited.json()["error"]["message"].is_string());
    assert_eq!(
        limited
            .headers
            .get(header::RETRY_AFTER)
            .expect("retry-after"),
        "1"
    );
    assert_eq!(number(&limited, "ratelimit-limit"), 2);
    assert_eq!(number(&limited, "ratelimit-remaining"), 0);
    assert_eq!(
        limited
            .headers
            .get(header::CACHE_CONTROL)
            .expect("cache-control"),
        "no-store"
    );

    let other = send(
        &harness.app,
        from(Method::GET, "/api/v1/instance", OTHER_CLIENT),
    )
    .await;
    assert_eq!(other.status, StatusCode::OK);

    tokio::time::sleep(Duration::from_millis(250)).await;
    let recovered = send(&harness.app, from(Method::GET, "/api/v1/instance", CLIENT)).await;
    assert_eq!(recovered.status, StatusCode::OK);
}

#[tokio::test]
async fn endpoint_creation_has_its_own_limit_per_client() {
    let harness =
        harness_with_limits(limits("1000/s:1000", "1/m:2", TrustedProxies::default())).await;
    for _ in 0..2 {
        let created = send(
            &harness.app,
            from(Method::POST, "/api/v1/endpoints", CLIENT),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED);
        assert_eq!(number(&created, "ratelimit-limit"), 1000);
    }
    let limited = send(
        &harness.app,
        from(Method::POST, "/api/v1/endpoints", CLIENT),
    )
    .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.json()["error"]["code"], "rate_limited");
    assert_eq!(
        limited
            .headers
            .get(header::RETRY_AFTER)
            .expect("retry-after"),
        "60"
    );
    assert_eq!(number(&limited, "ratelimit-limit"), 2);

    let read = send(&harness.app, from(Method::GET, "/api/v1/instance", CLIENT)).await;
    assert_eq!(read.status, StatusCode::OK);
    let other = send(
        &harness.app,
        from(Method::POST, "/api/v1/endpoints", OTHER_CLIENT),
    )
    .await;
    assert_eq!(other.status, StatusCode::CREATED);
}

#[tokio::test]
async fn a_trusted_proxy_peer_is_limited_per_forwarded_client() {
    let proxies = TrustedProxies::new(vec!["10.0.0.0/8".parse().expect("valid CIDR")]);
    let harness = harness_with_limits(limits("1/m:1", "100/m:100", proxies)).await;
    let forwarded = |client: &str| {
        let mut request = from(Method::GET, "/api/v1/instance", "10.1.1.1:4000");
        request
            .headers_mut()
            .insert("x-forwarded-for", client.parse().expect("valid header"));
        request
    };
    assert_eq!(
        send(&harness.app, forwarded("203.0.113.1")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        send(&harness.app, forwarded("203.0.113.1")).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        send(&harness.app, forwarded("203.0.113.2")).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_request_without_connection_info_is_refused_rather_than_admitted() {
    let harness =
        harness_with_limits(limits("10/s:10", "10/s:10", TrustedProxies::default())).await;
    let request = Request::builder()
        .uri("/api/v1/instance")
        .body(Body::empty())
        .expect("the request is valid");
    let reply = send(&harness.app, request).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.json()["error"]["code"], "internal_error");
}
