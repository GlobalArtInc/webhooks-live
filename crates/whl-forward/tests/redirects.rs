mod support;

use std::net::IpAddr;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method};
use support::{Reply, TestServer, client, client_with, get, loopback_config};
use whl_forward::{
    ForwardError, GuardConfig, OutboundRequest, RefusalReason, StaticLookup, validate_target_url,
};

#[tokio::test]
async fn follows_a_relative_redirect_on_a_fresh_connection_per_hop() {
    let server = TestServer::start("127.0.0.1:0", |request| match request.target.as_str() {
        "/start" => Reply::redirect(302, "/final?x=1"),
        _ => Reply::ok("done"),
    })
    .await;
    let config = loopback_config();
    let guarded = client(config.clone());
    let request = get(&server.url("/start"), &config);
    let response = tokio::spawn(async move { guarded.send(request).await })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(response.status.as_u16(), 200);
    assert_eq!(response.body.as_ref(), b"done");
    assert_eq!(response.redirects_followed, 1);
    assert_eq!(response.final_url.unpack().path(), "/final");
    assert_eq!(response.final_url.unpack().query(), Some("x=1"));
    assert_eq!(server.seen().len(), 2);
    assert_eq!(server.connections(), 2);
    let names: Vec<&str> = response.headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["content-type", "content-length"]);
}

#[tokio::test]
async fn stops_after_the_maximum_number_of_redirects() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::redirect(302, "/again")).await;
    let config = GuardConfig {
        max_redirects: 3,
        ..loopback_config()
    };
    let error = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap_err();

    assert_eq!(error, ForwardError::TooManyRedirects { limit: 3 });
    assert_eq!(server.seen().len(), 4);
}

#[tokio::test]
async fn drops_credentials_on_a_cross_port_redirect_and_keeps_other_headers() {
    let target = TestServer::start("127.0.0.1:0", |_| Reply::ok("target")).await;
    let target_url = target.url("/landing");
    let origin = TestServer::start("127.0.0.1:0", move |_| Reply::redirect(307, &target_url)).await;
    let config = loopback_config();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
    headers.insert("cookie", HeaderValue::from_static("session=1"));
    headers.insert("x-trace", HeaderValue::from_static("abc"));
    let request = OutboundRequest::new(
        Method::GET,
        validate_target_url(&origin.url("/"), &config).unwrap(),
        headers,
        Bytes::new(),
    );
    let response = client(config).send(request).await.unwrap();

    assert_eq!(response.status.as_u16(), 200);
    let first = &origin.seen()[0];
    assert_eq!(first.header("authorization"), Some("Bearer secret"));
    assert_eq!(first.header("cookie"), Some("session=1"));
    let second = &target.seen()[0];
    assert_eq!(second.header("authorization"), None);
    assert_eq!(second.header("cookie"), None);
    assert_eq!(second.header("x-trace"), Some("abc"));
}

#[tokio::test]
async fn converts_post_to_get_on_302_and_keeps_it_on_307() {
    let server = TestServer::start("127.0.0.1:0", |request| match request.target.as_str() {
        "/found" => Reply::redirect(302, "/target"),
        "/temporary" => Reply::redirect(307, "/target"),
        _ => Reply::ok("done"),
    })
    .await;
    let config = loopback_config();
    let guarded = client(config.clone());
    for path in ["/found", "/temporary"] {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let request = OutboundRequest::new(
            Method::POST,
            validate_target_url(&server.url(path), &config).unwrap(),
            headers,
            Bytes::from_static(b"{\"a\":1}"),
        );
        guarded.send(request).await.unwrap();
    }

    let seen = server.seen();
    assert_eq!(seen[1].method, "GET");
    assert!(seen[1].body.is_empty());
    assert_eq!(seen[1].header("content-type"), None);
    assert_eq!(seen[3].method, "POST");
    assert_eq!(seen[3].body, b"{\"a\":1}");
    assert_eq!(seen[3].header("content-type"), Some("application/json"));
}

#[tokio::test]
async fn refuses_redirects_to_refused_targets_without_any_connection() {
    let loopback_v6 =
        TestServer::start_if_bindable("[::1]:0", |_| Reply::ok("must not be reached")).await;
    let v6_url = loopback_v6
        .as_ref()
        .map_or_else(|| "http://[::1]:9/".to_owned(), |server| server.url("/"));
    let hostname_url = "http://rebind.test:9/".to_owned();
    let origin = TestServer::start("127.0.0.1:0", move |request| {
        match request.target.as_str() {
            "/metadata" => Reply::redirect(302, "http://169.254.169.254/latest/meta-data/"),
            "/loopback" => Reply::redirect(302, &v6_url),
            "/rebind" => Reply::redirect(302, &hostname_url),
            _ => Reply::redirect(302, "http://10.0.0.1/"),
        }
    })
    .await;
    let rebind: [IpAddr; 2] = [
        "169.254.169.254".parse().unwrap(),
        "10.0.0.5".parse().unwrap(),
    ];
    let lookup = Arc::new(StaticLookup::new().with_answer("rebind.test", &rebind));
    let config = loopback_config();
    let guarded = client_with(config.clone(), Arc::clone(&lookup));
    let mut paths = vec!["/metadata", "/rebind", "/other"];
    if loopback_v6.is_some() {
        paths.push("/loopback");
    }
    for path in &paths {
        let error = guarded
            .send(get(&origin.url(path), &config))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ForwardError::RedirectRefused(RefusalReason::RefusedAddress),
            "{path}"
        );
    }

    assert_eq!(origin.seen().len(), paths.len());
    if let Some(server) = &loopback_v6 {
        assert!(server.seen().is_empty());
        assert_eq!(server.connections(), 0);
    }
    assert_eq!(lookup.calls(), 1);
}

#[tokio::test]
async fn re_checks_the_initial_target_against_the_client_configuration() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("must not be reached")).await;
    let permissive = loopback_config();
    let request = get(&server.url("/"), &permissive);
    let strict = GuardConfig {
        allow: Vec::new(),
        ..permissive
    };
    let error = client(strict).send(request).await.unwrap_err();

    assert_eq!(
        error,
        ForwardError::TargetRefused(RefusalReason::RefusedAddress)
    );
    assert_eq!(server.connections(), 0);
}

#[tokio::test]
async fn returns_the_redirect_response_itself_when_redirects_are_disabled() {
    let server = TestServer::start("127.0.0.1:0", |_| Reply::redirect(302, "/elsewhere")).await;
    let config = GuardConfig {
        max_redirects: 0,
        ..loopback_config()
    };
    let response = client(config.clone())
        .send(get(&server.url("/"), &config))
        .await
        .unwrap();

    assert_eq!(response.status.as_u16(), 302);
    assert_eq!(response.redirects_followed, 0);
    assert!(
        response
            .headers
            .iter()
            .any(|header| header.name.as_str() == "location" && header.value == "/elsewhere")
    );
    assert_eq!(server.seen().len(), 1);
}
