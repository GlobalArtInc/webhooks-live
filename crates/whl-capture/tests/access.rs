mod common;
mod served;

use std::sync::Arc;

use axum::body::Body;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Harness, Reply, SENDER, default_limits, request_from};
use served::{Configured, Wire, endpoint_configured, serve};
use whl_capture::CaptureLimits;
use whl_core::capture_auth::{AuthHeaderName, AuthUsername};
use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::{LimiterSettings, Limiters};
use whl_core::sender_allow::{SenderAllowList, SenderRange};
use whl_secrets::Keyring;
use whl_store::{
    AccessSettings, CaptureAuthRecord, EndpointChange, LifecycleError, LifecycleLimits, Store,
    StoredMac,
};

const USER: &str = "hook-user";
const PASSWORD: &str = "test-password-one";
const HEADER_NAME: &str = "x-hook-token";
const HEADER_VALUE: &str = "test-header-value-one";
const OUTSIDE: &str = "198.51.100.10:4000";
const INSIDE: &str = "203.0.113.20:4000";
const ALLOWED_RANGE: &str = "203.0.113.0/24";

fn keyring() -> Arc<Keyring> {
    let spec = format!("1:{}", STANDARD.encode([1_u8; 32]));
    Arc::new(Keyring::parse(&spec).expect("the key set is valid"))
}

fn allow(texts: &[&str]) -> SenderAllowList {
    SenderAllowList::new(
        texts
            .iter()
            .map(|text| SenderRange::parse(text).expect("the range parses"))
            .collect(),
    )
    .expect("the list fits")
}

fn mac(keyring: &Keyring, id: &EndpointId, text: &str) -> (whl_secrets::KeyId, StoredMac) {
    let (key_id, key) = keyring.active_capture_mac_key(id).expect("the key derives");
    let tag = key.tag(text.as_bytes()).expect("the code computes");
    (key_id, StoredMac::new(*tag.unpack()))
}

fn basic_record(keyring: &Keyring, id: &EndpointId) -> CaptureAuthRecord {
    let (key_id, password_mac) = mac(keyring, id, PASSWORD);
    CaptureAuthRecord::Basic {
        key_id,
        username: AuthUsername::parse(USER).expect("a valid user name"),
        password_mac,
    }
}

fn header_record(keyring: &Keyring, id: &EndpointId) -> CaptureAuthRecord {
    let (key_id, value_mac) = mac(keyring, id, HEADER_VALUE);
    CaptureAuthRecord::Header {
        key_id,
        name: AuthHeaderName::parse(HEADER_NAME).expect("a valid header name"),
        value_mac,
    }
}

async fn protect(
    harness: &Harness,
    id: &EndpointId,
    sender_allow: SenderAllowList,
    capture_auth: Option<CaptureAuthRecord>,
) {
    let store = Store::from_pool(harness.db.pool().clone());
    store
        .update_endpoint::<LifecycleError, _>(
            id,
            default_limits().retention,
            LifecycleLimits::NONE,
            move |current, _owner| {
                Ok(EndpointChange {
                    settings: current.settings.clone(),
                    lifecycle: current.lifecycle,
                    access: AccessSettings {
                        sender_allow,
                        capture_auth,
                    },
                })
            },
        )
        .await
        .expect("the update runs")
        .expect("the endpoint is live");
}

async fn protected_by_basic(harness: &Harness, keyring: &Keyring) -> EndpointId {
    let id = harness.endpoint().await;
    protect(
        harness,
        &id,
        SenderAllowList::default(),
        Some(basic_record(keyring, &id)),
    )
    .await;
    id
}

async fn protected_by_header(harness: &Harness, keyring: &Keyring) -> EndpointId {
    let id = harness.endpoint().await;
    protect(
        harness,
        &id,
        SenderAllowList::default(),
        Some(header_record(keyring, &id)),
    )
    .await;
    id
}

fn basic_value(user: &str, password: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{user}:{password}")))
}

async fn post_from(
    harness: &Harness,
    id: &EndpointId,
    peer: &str,
    headers: &[(&str, &str)],
) -> Reply {
    let mut builder = request_from("POST", &format!("/{id}"), peer);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    harness
        .send(builder.body(Body::from("payload")).expect("request builds"))
        .await
}

async fn post(harness: &Harness, id: &EndpointId, headers: &[(&str, &str)]) -> Reply {
    post_from(harness, id, SENDER, headers).await
}

fn assert_refused(reply: &Reply, status: u16, error: &str) {
    assert_eq!(reply.status, status);
    assert_eq!(reply.header("x-webhooks-live-error"), Some(error));
    assert_eq!(reply.header("connection"), Some("close"));
    assert!(reply.headers.get("www-authenticate").is_none());
    assert!(reply.headers.get("x-request-id").is_none());
}

fn preflight_to(id: &EndpointId, peer: &str) -> axum::http::Request<Body> {
    request_from("OPTIONS", &format!("/{id}"), peer)
        .header("origin", "https://app.example.test")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", HEADER_NAME)
        .body(Body::empty())
        .expect("request builds")
}

#[tokio::test]
async fn a_sender_outside_the_allow_list_gets_403_and_nothing_is_stored() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    protect(&harness, &id, allow(&[ALLOWED_RANGE]), None).await;

    let refused = post_from(&harness, &id, OUTSIDE, &[]).await;
    assert_refused(&refused, 403, "sender_refused");
    assert_eq!(refused.body, "Forbidden.");
    assert!(harness.rows(&id).await.is_empty());

    let admitted = post_from(&harness, &id, INSIDE, &[]).await;
    assert_eq!(admitted.status, 200);
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn the_allow_list_judges_the_client_address_behind_trusted_proxies_only() {
    let harness = Harness::with_proxies(&["198.51.100.0/24"]).await;
    let id = harness.endpoint().await;
    protect(&harness, &id, allow(&[ALLOWED_RANGE]), None).await;

    let forwarded_inside = post_from(
        &harness,
        &id,
        OUTSIDE,
        &[("x-forwarded-for", "203.0.113.9")],
    )
    .await;
    assert_eq!(forwarded_inside.status, 200);

    let forwarded_outside =
        post_from(&harness, &id, OUTSIDE, &[("x-forwarded-for", "192.0.2.9")]).await;
    assert_refused(&forwarded_outside, 403, "sender_refused");

    let spoofed = post_from(
        &harness,
        &id,
        "192.0.2.50:4000",
        &[("x-forwarded-for", "203.0.113.9")],
    )
    .await;
    assert_refused(&spoofed, 403, "sender_refused");
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn basic_credentials_are_required_and_every_failure_looks_the_same() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let id = protected_by_basic(&harness, &keyring).await;

    let long_value = format!("Basic {}", "A".repeat(5000));
    let wrong_password = basic_value(USER, "test-password-two");
    let wrong_user = basic_value("other-user", PASSWORD);
    let wrong_both = basic_value("other-user", "test-password-two");
    let no_colon = format!("Basic {}", STANDARD.encode("hook-user"));
    let right = basic_value(USER, PASSWORD);
    let cases: [(&str, Vec<(&str, &str)>); 8] = [
        ("missing", vec![]),
        (
            "wrong password",
            vec![("authorization", wrong_password.as_str())],
        ),
        ("wrong user", vec![("authorization", wrong_user.as_str())]),
        (
            "wrong user and password",
            vec![("authorization", wrong_both.as_str())],
        ),
        (
            "not base64",
            vec![("authorization", "Basic !!!not-base64!!!")],
        ),
        ("no colon", vec![("authorization", no_colon.as_str())]),
        (
            "other scheme",
            vec![("authorization", "Bearer test-password-one")],
        ),
        ("oversized", vec![("authorization", long_value.as_str())]),
    ];
    for (label, headers) in &cases {
        let reply = post(&harness, &id, headers).await;
        assert_refused(&reply, 401, "unauthorized");
        assert_eq!(reply.body, "Unauthorized.", "{label}");
    }

    let two_values = post(
        &harness,
        &id,
        &[
            ("authorization", right.as_str()),
            ("authorization", wrong_password.as_str()),
        ],
    )
    .await;
    assert_refused(&two_values, 401, "unauthorized");
    assert!(harness.rows(&id).await.is_empty());

    let scheme_in_capitals = format!("BASIC {}", right.trim_start_matches("Basic "));
    for value in [&right, &scheme_in_capitals] {
        let accepted = post(&harness, &id, &[("authorization", value.as_str())]).await;
        assert_eq!(accepted.status, 200);
    }
    assert_eq!(harness.rows(&id).await.len(), 2);
}

#[tokio::test]
async fn a_header_secret_is_required_by_name_and_by_value() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let id = protected_by_header(&harness, &keyring).await;

    let long_value = "v".repeat(257);
    let cases: [(&str, Vec<(&str, &str)>); 4] = [
        ("missing", vec![]),
        ("wrong value", vec![(HEADER_NAME, "test-header-value-two")]),
        ("other header", vec![("x-other-token", HEADER_VALUE)]),
        ("oversized", vec![(HEADER_NAME, long_value.as_str())]),
    ];
    for (label, headers) in &cases {
        let reply = post(&harness, &id, headers).await;
        assert_refused(&reply, 401, "unauthorized");
        assert_eq!(reply.body, "Unauthorized.", "{label}");
    }

    let repeated = post(
        &harness,
        &id,
        &[(HEADER_NAME, HEADER_VALUE), (HEADER_NAME, HEADER_VALUE)],
    )
    .await;
    assert_refused(&repeated, 401, "unauthorized");
    assert!(harness.rows(&id).await.is_empty());

    let accepted = post(&harness, &id, &[("X-Hook-Token", HEADER_VALUE)]).await;
    assert_eq!(accepted.status, 200);
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn a_preflight_to_a_protected_cors_endpoint_is_answered_without_credentials_and_not_stored() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let id = endpoint_configured(
        &harness,
        Configured {
            cors: true,
            ..Configured::default()
        },
    )
    .await;
    protect(
        &harness,
        &id,
        SenderAllowList::default(),
        Some(header_record(&keyring, &id)),
    )
    .await;

    let preflight = harness.send(preflight_to(&id, SENDER)).await;
    assert_eq!(preflight.status, 204);
    assert_eq!(preflight.header("access-control-allow-origin"), Some("*"));
    assert_eq!(
        preflight.header("access-control-allow-methods"),
        Some("POST")
    );
    assert_eq!(
        preflight.header("access-control-allow-headers"),
        Some(HEADER_NAME)
    );
    assert!(preflight.headers.get("x-request-id").is_none());
    assert!(harness.rows(&id).await.is_empty());

    let plain_options = harness
        .send(
            request_from("OPTIONS", &format!("/{id}"), SENDER)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(plain_options.status, 401);
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn a_preflight_without_cors_mode_needs_credentials_like_any_request() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let id = protected_by_header(&harness, &keyring).await;

    let reply = harness.send(preflight_to(&id, SENDER)).await;

    assert_eq!(reply.status, 401);
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn a_preflight_from_a_sender_outside_the_allow_list_is_refused() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let id = endpoint_configured(
        &harness,
        Configured {
            cors: true,
            ..Configured::default()
        },
    )
    .await;
    protect(
        &harness,
        &id,
        allow(&[ALLOWED_RANGE]),
        Some(header_record(&keyring, &id)),
    )
    .await;

    let refused = harness.send(preflight_to(&id, OUTSIDE)).await;
    assert_eq!(refused.status, 403);
    assert_eq!(
        refused.header("x-webhooks-live-error"),
        Some("sender_refused")
    );

    let admitted = harness.send(preflight_to(&id, INSIDE)).await;
    assert_eq!(admitted.status, 204);
}

#[tokio::test]
async fn refused_requests_do_not_spend_the_endpoint_rate_budget() {
    let limiters = Limiters::new(&LimiterSettings {
        capture_ip: "1000/s:1000".parse().expect("valid rate spec"),
        capture_endpoint: "1/m:2".parse().expect("valid rate spec"),
        api_ip: "1000/s:1000".parse().expect("valid rate spec"),
        create_ip: "1000/s:1000".parse().expect("valid rate spec"),
        max_keys: 1000,
    })
    .expect("limiters build");
    let keyring = keyring();
    let harness = Harness::new()
        .await
        .with_secrets(Arc::clone(&keyring))
        .with_limiters(limiters);
    let id = protected_by_header(&harness, &keyring).await;

    for _ in 0..5 {
        let refused = post(&harness, &id, &[(HEADER_NAME, "test-header-value-two")]).await;
        assert_eq!(refused.status, 401);
    }
    let right = [(HEADER_NAME, HEADER_VALUE)];
    assert_eq!(post(&harness, &id, &right).await.status, 200);
    assert_eq!(post(&harness, &id, &right).await.status, 200);
    assert_eq!(post(&harness, &id, &right).await.status, 429);
}

#[tokio::test]
async fn a_full_endpoint_tells_an_unauthorized_sender_nothing() {
    let keyring = keyring();
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 1,
        ..default_limits()
    })
    .await
    .with_secrets(Arc::clone(&keyring));
    let id = protected_by_header(&harness, &keyring).await;
    let right = [(HEADER_NAME, HEADER_VALUE)];
    assert_eq!(post(&harness, &id, &right).await.status, 200);

    let authorized = post(&harness, &id, &right).await;
    assert_eq!(authorized.status, 410);
    assert_eq!(
        authorized.header("x-webhooks-live-error"),
        Some("endpoint_full")
    );

    let unauthorized = post(&harness, &id, &[]).await;
    assert_refused(&unauthorized, 401, "unauthorized");
}

#[tokio::test]
async fn capture_auth_that_cannot_be_verified_refuses_everyone() {
    let keyring = keyring();
    let without_keyring = Harness::new().await;
    let id = protected_by_basic(&without_keyring, &keyring).await;
    let right = basic_value(USER, PASSWORD);
    let reply = post(&without_keyring, &id, &[("authorization", right.as_str())]).await;
    assert_refused(&reply, 401, "unauthorized");

    let other_spec = format!("2:{}", STANDARD.encode([2_u8; 32]));
    let other = Arc::new(Keyring::parse(&other_spec).expect("the key set is valid"));
    let with_other_key = Harness::new().await.with_secrets(other);
    let id = protected_by_basic(&with_other_key, &keyring).await;
    let reply = post(&with_other_key, &id, &[("authorization", right.as_str())]).await;
    assert_refused(&reply, 401, "unauthorized");
    assert!(with_other_key.rows(&id).await.is_empty());
}

#[tokio::test]
async fn the_credential_header_is_stored_redacted_on_a_protected_endpoint_only() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let by_basic = protected_by_basic(&harness, &keyring).await;
    let by_header = protected_by_header(&harness, &keyring).await;
    let unprotected = harness.endpoint().await;
    let basic = basic_value(USER, PASSWORD);

    let sent = [
        post(
            &harness,
            &by_basic,
            &[("authorization", basic.as_str()), ("x-keep", "kept")],
        )
        .await,
        post(
            &harness,
            &by_header,
            &[
                (HEADER_NAME, HEADER_VALUE),
                ("authorization", "Bearer unrelated-token"),
            ],
        )
        .await,
        post(&harness, &unprotected, &[("authorization", basic.as_str())]).await,
    ];
    assert!(sent.iter().all(|reply| reply.status == 200));

    let rows = harness.rows(&by_basic).await;
    assert_eq!(rows[0].header_values("authorization"), ["[redacted]"]);
    assert_eq!(rows[0].header_values("x-keep"), ["kept"]);
    assert!(!rows[0].search_text.contains(PASSWORD));
    assert!(!rows[0].search_text.contains(&basic.to_lowercase()));

    let rows = harness.rows(&by_header).await;
    assert_eq!(rows[0].header_values(HEADER_NAME), ["[redacted]"]);
    assert_eq!(
        rows[0].header_values("authorization"),
        ["Bearer unrelated-token"]
    );
    assert!(!rows[0].search_text.contains(HEADER_VALUE));

    let rows = harness.rows(&unprotected).await;
    assert_eq!(rows[0].header_values("authorization"), [basic.as_str()]);
}

#[tokio::test]
async fn early_refusals_close_the_connection_before_the_body_is_read() {
    let keyring = keyring();
    let harness = Harness::new().await.with_secrets(Arc::clone(&keyring));
    let unauthorized = protected_by_basic(&harness, &keyring).await;
    let forbidden = harness.endpoint().await;
    protect(&harness, &forbidden, allow(&[ALLOWED_RANGE]), None).await;
    let addr = serve(&harness).await;

    for (id, status, error) in [
        (&unauthorized, 401, "unauthorized"),
        (&forbidden, 403, "sender_refused"),
    ] {
        let mut wire = Wire::connect(addr).await;
        wire.write(
            format!("POST /{id} HTTP/1.1\r\nHost: t\r\nContent-Length: 5000\r\n\r\n").as_bytes(),
        )
        .await;
        let head = wire.head().await;
        assert_eq!(head.status, status);
        assert_eq!(head.header("x-webhooks-live-error").as_deref(), Some(error));
        assert_eq!(head.header("connection").as_deref(), Some("close"));
        assert!(head.header("www-authenticate").is_none());
        let length: usize = head
            .header("content-length")
            .and_then(|value| value.parse().ok())
            .expect("content length");
        wire.body(length).await;
        assert!(wire.closed_by_server().await);
        assert!(harness.rows(id).await.is_empty());
    }
}
