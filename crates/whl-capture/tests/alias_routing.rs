mod common;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use axum::routing::get;
use common::{Harness, default_limits, request, send};
use whl_capture::{AliasCacheSettings, CaptureLimits, router, router_with_fallback};
use whl_core::endpoint_id::EndpointId;

async fn alias_for(harness: &Harness, alias: &str, endpoint: &EndpointId) {
    sqlx::query("INSERT INTO endpoint_aliases (alias, endpoint_id) VALUES ($1, $2)")
        .bind(alias)
        .bind(endpoint.unpack())
        .execute(harness.db.pool())
        .await
        .expect("the alias inserts");
}

async fn drop_alias(harness: &Harness, alias: &str) {
    sqlx::query("DELETE FROM endpoint_aliases WHERE alias = $1")
        .bind(alias)
        .execute(harness.db.pool())
        .await
        .expect("the alias is removed");
}

fn stub() -> Router {
    Router::new().fallback(get(|| async { (StatusCode::IM_A_TEAPOT, "ui fallback") }))
}

#[tokio::test]
async fn an_alias_captures_into_its_endpoint_with_the_rest_of_the_path() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    alias_for(&harness, "my-hook", &id).await;

    let bare = harness.get("/a/my-hook").await;
    assert_eq!(bare.status, 200);
    assert_eq!(bare.header("x-endpoint-id"), None);
    let overridden = harness.post("/a/my-hook/404/deeper?x=1", "payload").await;
    assert_eq!(overridden.status, 404);

    let rows = harness.rows(&id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path, "");
    assert_eq!(rows[1].path, "/404/deeper");
    assert_eq!(rows[1].method, "POST");
}

#[tokio::test]
async fn an_unknown_or_malformed_alias_answers_gone_and_stores_nothing() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;

    for path in ["/a/nobody-here", "/a/Bad_Name", "/a/ab", "/a/admin"] {
        let reply = harness.get(path).await;
        assert_eq!(reply.status, 410, "{path}");
        assert_eq!(
            reply.header("x-webhooks-live-error"),
            Some("endpoint_gone"),
            "{path}"
        );
        assert_eq!(reply.header("x-endpoint-id"), None, "{path}");
    }
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn a_released_alias_no_longer_routes() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    sqlx::query(
        "INSERT INTO endpoint_aliases (alias, endpoint_id, released_at) VALUES ('old-name', NULL, now())",
    )
    .execute(harness.db.pool())
    .await
    .expect("the released alias inserts");
    alias_for(&harness, "live-name", &id).await;
    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(id.unpack())
        .execute(harness.db.pool())
        .await
        .expect("the endpoint is soft deleted");

    assert_eq!(harness.get("/a/old-name").await.status, 410);
    assert_eq!(harness.get("/a/live-name").await.status, 410);
}

#[tokio::test]
async fn an_alias_answer_is_cached_for_its_lifetime_in_both_directions() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    alias_for(&harness, "cached-name", &id).await;

    assert_eq!(harness.get("/a/cached-name").await.status, 200);
    assert_eq!(harness.get("/a/later-name").await.status, 410);
    drop_alias(&harness, "cached-name").await;
    alias_for(&harness, "later-name", &id).await;

    assert_eq!(harness.get("/a/cached-name").await.status, 200);
    assert_eq!(harness.get("/a/later-name").await.status, 410);
    assert_eq!(harness.rows(&id).await.len(), 2);
}

#[tokio::test]
async fn a_cached_alias_answer_expires_after_its_lifetime() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    alias_for(&harness, "short-lived", &id).await;
    let state = harness.state.clone().with_alias_cache(AliasCacheSettings {
        positive_ttl: Duration::from_millis(50),
        negative_ttl: Duration::from_millis(50),
        capacity: 100,
        sweep_interval: Duration::from_secs(1),
    });
    let app = router(state);
    let get = |path: &'static str| {
        send(
            &app,
            request("GET", path)
                .body(Body::empty())
                .expect("request builds"),
        )
    };

    assert_eq!(get("/a/short-lived").await.status, 200);
    drop_alias(&harness, "short-lived").await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(get("/a/short-lived").await.status, 410);
}

#[tokio::test]
async fn the_fallback_still_serves_a_bare_prefix_and_the_alias_never_reaches_it() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    alias_for(&harness, "routed-name", &id).await;
    let app = router_with_fallback(harness.state.clone(), stub());
    let reach = |path: &'static str| {
        send(
            &app,
            request("GET", path)
                .body(Body::empty())
                .expect("request builds"),
        )
    };

    assert_eq!(reach("/a/routed-name").await.status, 200);
    for path in ["/a", "/a/", "/a//x"] {
        let reply = reach(path).await;
        assert_eq!(reply.status, 418, "{path}");
        assert_eq!(reply.body, "ui fallback", "{path}");
    }
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn a_response_through_an_alias_neither_carries_nor_exposes_the_endpoint_id() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    sqlx::query("UPDATE endpoints SET cors = true WHERE id = $1")
        .bind(id.unpack())
        .execute(harness.db.pool())
        .await
        .expect("cors switches on");
    alias_for(&harness, "hidden-id", &id).await;

    let direct = harness.get(&format!("/{id}")).await;
    assert_eq!(direct.header("x-endpoint-id"), Some(id.unpack()));
    assert_eq!(
        direct.header("access-control-expose-headers"),
        Some("X-Request-Id, X-Endpoint-Id")
    );

    let through_alias = harness.get("/a/hidden-id").await;
    assert_eq!(through_alias.status, 200);
    assert_eq!(through_alias.header("x-endpoint-id"), None);
    assert_eq!(
        through_alias.header("access-control-expose-headers"),
        Some("X-Request-Id")
    );
    assert!(through_alias.header("x-request-id").is_some());
}

#[tokio::test]
async fn a_full_or_expired_endpoint_behind_an_alias_answers_without_its_id() {
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 1,
        ..default_limits()
    })
    .await;
    let full = harness.endpoint().await;
    let expired = harness.endpoint().await;
    alias_for(&harness, "full-name", &full).await;
    alias_for(&harness, "expired-name", &expired).await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '30 days' WHERE id = $1")
        .bind(expired.unpack())
        .execute(harness.db.pool())
        .await
        .expect("the endpoint ages");

    assert_eq!(harness.get("/a/full-name").await.status, 200);
    let refused = harness.get("/a/full-name").await;
    assert_eq!(refused.status, 410);
    assert_eq!(
        refused.header("x-webhooks-live-error"),
        Some("endpoint_full")
    );
    assert_eq!(refused.header("x-endpoint-id"), None);

    let gone = harness.get("/a/expired-name").await;
    let unknown = harness.get("/a/unknown-name").await;
    assert_eq!(gone.status, 410);
    assert_eq!(gone.status, unknown.status);
    assert_eq!(gone.body, unknown.body);
    assert_eq!(gone.headers, unknown.headers);
    assert_eq!(gone.header("x-endpoint-id"), None);
}

#[tokio::test]
async fn an_unavailable_store_answers_an_alias_request_with_503_and_retry_after() {
    let harness = Harness::new().await;
    harness.db.pool().close().await;

    let reply = harness.get("/a/some-name").await;

    assert_eq!(reply.status, 503);
    assert_eq!(reply.header("retry-after"), Some("5"));
    assert_eq!(
        reply.header("x-webhooks-live-error"),
        Some("store_unavailable")
    );
}
