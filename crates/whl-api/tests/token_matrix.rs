mod common;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware::from_fn;
use common::matrix::{Route, cast, error_code, fixture, routes, snapshot, store_capture, uri};
use common::tokens::{ALL_SCOPES, mint};
use common::{harness, mixed_harness, send, sso_harness};
use tower::ServiceExt;
use whl_store::EndpointRole;

const TICK: Duration = Duration::from_millis(150);

fn scope_of(route: &Route) -> Option<&'static str> {
    if route.members {
        return None;
    }
    if route.method == Method::GET {
        return Some("read");
    }
    match (route.method.as_str(), route.path) {
        ("PATCH" | "DELETE", "") => Some("endpoints:write"),
        (_, path) if path.starts_with("/forward-rules") => Some("rules:write"),
        ("DELETE", path) if path.starts_with("/captures") => Some("captures:write"),
        ("POST", path) if path.ends_with("/forward") || path.ends_with("/resend") => Some("send"),
        _ => None,
    }
}

#[tokio::test]
async fn a_full_scope_token_gets_exactly_its_owners_role_on_every_route() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let callers = [
        ("stranger", &cast.stranger, None),
        ("viewer", &cast.viewer, Some(EndpointRole::Viewer)),
        ("editor", &cast.editor, Some(EndpointRole::Editor)),
        ("owner", &cast.owner, Some(EndpointRole::Owner)),
    ];
    let mut tokens = Vec::new();
    for (name, actor, role) in callers {
        tokens.push((name, mint(&harness, actor, &ALL_SCOPES, None).await, role));
    }
    for route in routes() {
        assert!(
            route.members || scope_of(&route).is_some(),
            "{} {} has no scope in this test",
            route.method,
            route.path
        );
        for (name, minted, role) in &tokens {
            let fixture = fixture(&harness, &cast, true).await;
            let before = snapshot(&harness, &fixture.endpoint).await;
            let target = uri(&route, &fixture, &cast);
            let reply = harness
                .send_bearer(&minted.token, route.method.clone(), &target, route.body)
                .await;
            let label = format!("{} {} as {name}", route.method, route.path);
            match role {
                _ if route.members => {
                    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{label}");
                    assert_eq!(error_code(&reply), "session_required", "{label}");
                }
                None => {
                    assert_eq!(reply.status, StatusCode::GONE, "{label}");
                    assert_eq!(error_code(&reply), "endpoint_gone", "{label}");
                }
                Some(held) if *held < route.needed => {
                    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{label}");
                    assert_eq!(error_code(&reply), "forbidden", "{label}");
                }
                Some(_) => {
                    assert_eq!(reply.status, route.success, "{label}");
                    continue;
                }
            }
            assert_eq!(
                snapshot(&harness, &fixture.endpoint).await,
                before,
                "{label} left the data untouched"
            );
        }
    }
}

#[tokio::test]
async fn every_route_needs_its_own_scope_and_no_other_scope_will_do() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let mut singles = Vec::new();
    for scope in ALL_SCOPES {
        singles.push((scope, mint(&harness, &cast.owner, &[scope], None).await));
    }
    for route in routes() {
        let needed = scope_of(&route);
        for (scope, minted) in &singles {
            let fixture = fixture(&harness, &cast, true).await;
            let before = snapshot(&harness, &fixture.endpoint).await;
            let target = uri(&route, &fixture, &cast);
            let reply = harness
                .send_bearer(&minted.token, route.method.clone(), &target, route.body)
                .await;
            let label = format!("{} {} with only {scope}", route.method, route.path);
            if needed == Some(*scope) {
                assert_eq!(reply.status, route.success, "{label}");
                continue;
            }
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{label}");
            let expected = if route.members {
                "session_required"
            } else {
                "insufficient_scope"
            };
            assert_eq!(error_code(&reply), expected, "{label}");
            assert_eq!(
                snapshot(&harness, &fixture.endpoint).await,
                before,
                "{label} left the data untouched"
            );
        }
    }
}

#[tokio::test]
async fn in_mixed_mode_a_token_is_the_owner_of_an_ownerless_endpoint_except_on_person_routes() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let minted = mint(&harness, &cast.stranger, &ALL_SCOPES, None).await;
    for route in routes() {
        let fixture = fixture(&harness, &cast, false).await;
        let target = uri(&route, &fixture, &cast);
        let reply = harness
            .send_bearer(&minted.token, route.method.clone(), &target, route.body)
            .await;
        let label = format!("{} {}", route.method, route.path);
        if route.members {
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{label}");
            assert_eq!(error_code(&reply), "session_required", "{label}");
        } else {
            assert_eq!(reply.status, route.success, "{label}");
        }
    }
}

#[tokio::test]
async fn a_token_stream_ends_when_the_token_is_revoked_or_the_membership_is_lost() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast, true).await;
    let app = harness.live_app(TICK).await;
    let viewer = mint(
        &harness,
        &cast.viewer,
        &["read"],
        Some(&[fixture.endpoint.as_str()]),
    )
    .await;
    let editor = mint(&harness, &cast.editor, &["read"], None).await;

    let mut revoked = app.open_bearer(&viewer.token, &fixture.endpoint).await;
    assert_eq!(revoked.status, StatusCode::OK);
    revoked.expect_event("ready").await;
    let mut member = app.open_bearer(&editor.token, &fixture.endpoint).await;
    assert_eq!(member.status, StatusCode::OK);
    member.expect_event("ready").await;

    store_capture(&harness.store, &fixture.endpoint).await;
    assert_eq!(revoked.expect_event("capture").await["seq"], 2);
    assert_eq!(member.expect_event("capture").await["seq"], 2);

    let revoke = harness
        .send_as(
            Some(&cast.viewer),
            Method::DELETE,
            &format!("/api/v1/tokens/{}", viewer.id),
            "",
        )
        .await;
    assert_eq!(revoke.status, StatusCode::NO_CONTENT);
    revoked.expect_end_after_bye().await;

    sqlx::query("DELETE FROM endpoint_members WHERE endpoint_id = $1 AND user_id = $2")
        .bind(&fixture.endpoint)
        .bind(cast.editor.id)
        .execute(harness.db.pool())
        .await
        .expect("the membership is deleted");
    member.expect_end_after_bye().await;
}

#[tokio::test]
async fn a_stream_refuses_a_token_without_read_or_outside_its_endpoint_limit() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast, true).await;
    let other = harness.endpoint_for(&cast.owner).await;
    let app = harness.live_app(TICK).await;
    let no_read = mint(&harness, &cast.owner, &["send"], None).await;
    let elsewhere = mint(&harness, &cast.owner, &["read"], Some(&[other.as_str()])).await;
    let bogus = format!("whl_{}", "a".repeat(52));

    let denied = app.open_bearer(&no_read.token, &fixture.endpoint).await;
    let limited = app.open_bearer(&elsewhere.token, &fixture.endpoint).await;
    let unknown = app.open_bearer(&bogus, &fixture.endpoint).await;

    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(limited.status, StatusCode::FORBIDDEN);
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
}

async fn cors_app() -> (Router, common::Harness) {
    let harness = harness().await;
    let app = harness.app.clone().layer(from_fn(whl_api::api_cors));
    (app, harness)
}

#[tokio::test]
async fn a_preflight_is_answered_with_the_documented_headers_and_never_allows_credentials() {
    let (app, _harness) = cors_app().await;
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/v1/endpoints")
        .header(header::ORIGIN, "https://tools.example")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "authorization")
        .body(Body::empty())
        .expect("the request is valid");

    let reply = send(&app, request).await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let header_of = |name: &str| {
        reply
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    assert_eq!(
        header_of("access-control-allow-origin").as_deref(),
        Some("*")
    );
    assert_eq!(
        header_of("access-control-allow-methods").as_deref(),
        Some("GET, POST, PUT, PATCH, DELETE")
    );
    assert_eq!(
        header_of("access-control-allow-headers").as_deref(),
        Some("Authorization, Content-Type, If-None-Match, Last-Event-ID, X-Webhooks-Live-Grant")
    );
    assert_eq!(header_of("access-control-max-age").as_deref(), Some("600"));
    assert!(
        !reply
            .headers
            .contains_key("access-control-allow-credentials")
    );
    assert!(reply.body.is_empty());
}

#[tokio::test]
async fn api_responses_carry_the_allow_origin_and_expose_headers_but_other_paths_do_not() {
    let (app, _harness) = cors_app().await;

    let api = send(&app, common::bodiless(Method::GET, "/api/v1/instance")).await;
    let error = send(&app, common::bodiless(Method::GET, "/api/v1/nowhere")).await;
    let outside = app
        .clone()
        .oneshot(common::bodiless(Method::GET, "/auth/login"))
        .await
        .expect("the router does not fail");

    for reply in [&api, &error] {
        assert_eq!(reply.headers["access-control-allow-origin"], "*");
        assert_eq!(
            reply.headers["access-control-expose-headers"],
            "ETag, Location, RateLimit-Limit, RateLimit-Remaining, Retry-After"
        );
        assert!(
            !reply
                .headers
                .contains_key("access-control-allow-credentials")
        );
    }
    assert!(
        !outside
            .headers()
            .contains_key("access-control-allow-origin")
    );
}
