mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::composer::{MockLocal, PEER, composer, limits};
use common::matrix::error_code;
use common::{Reply, SsoHarness, UNKNOWN_ID, mixed_harness, mixed_harness_with};
use sqlx::PgPool;
use whl_auth::{GRANT_HEADER, ViewGrant};

const HOME: &str = "203.0.113.7";
const STUB_HASH: &str = "stub-hash-for-direct-inserts";
const NEVER: Duration = Duration::from_secs(3600);

async fn protect(pool: &PgPool, endpoint: &str) -> String {
    sqlx::query("UPDATE endpoints SET view_password_hash = $2 WHERE id = $1")
        .bind(endpoint)
        .bind(STUB_HASH)
        .execute(pool)
        .await
        .expect("the endpoint is protected");
    let grant = ViewGrant::generate().expect("entropy is available");
    let digest = grant.digest();
    sqlx::query(
        "INSERT INTO endpoint_grants (token_hash, endpoint_id, expires_at) \
         VALUES ($1, $2, now() + interval '1 hour')",
    )
    .bind(digest.unpack())
    .bind(endpoint)
    .execute(pool)
    .await
    .expect("the grant is stored");
    grant.unpack().to_owned()
}

async fn get(harness: &SsoHarness, grant: Option<&str>, uri: &str, peer: &str) -> Reply {
    let extra: Vec<(&str, &str)> = grant
        .map(|grant| (GRANT_HEADER, grant))
        .into_iter()
        .collect();
    harness
        .send_with(None, Some(peer), &extra, Method::GET, uri, "")
        .await
}

#[tokio::test]
async fn the_combined_inbox_list_treats_a_locked_endpoint_like_an_unknown_id_until_a_grant_is_shown()
 {
    let harness = mixed_harness().await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let uri = format!("/api/v1/inbox/captures?endpoints={endpoint}");

    let locked = get(&harness, None, &uri, HOME).await;
    let wrong = get(&harness, Some("not-a-grant"), &uri, HOME).await;
    let unknown = get(
        &harness,
        None,
        &format!("/api/v1/inbox/captures?endpoints={UNKNOWN_ID}"),
        HOME,
    )
    .await;
    let unlocked = get(&harness, Some(&grant), &uri, HOME).await;

    assert_eq!(unknown.status, StatusCode::GONE);
    for refused in [&locked, &wrong] {
        assert_eq!(refused.status, unknown.status);
        assert_eq!(error_code(refused), error_code(&unknown));
    }
    assert_eq!(unlocked.status, StatusCode::OK, "{:?}", unlocked.json());
}

#[tokio::test]
async fn the_combined_inbox_stream_refuses_a_locked_endpoint_and_opens_with_a_grant_in_the_query() {
    let harness = mixed_harness().await;
    let app = harness.live_app(NEVER).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;

    let locked = app
        .open_inbox(None, &format!("endpoints={endpoint}"), None)
        .await;
    let mut unlocked = app
        .open_inbox(None, &format!("endpoints={endpoint}&grant={grant}"), None)
        .await;

    assert_eq!(locked.status, StatusCode::GONE);
    assert_eq!(unlocked.status, StatusCode::OK);
    unlocked.expect_event("ready").await;
}

#[tokio::test]
async fn the_composer_history_of_a_locked_endpoint_needs_its_grant() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let uri = format!("/api/v1/send/history?endpoint_id={endpoint}");

    let locked = get(&harness, None, &uri, PEER).await;
    let unlocked = get(&harness, Some(&grant), &uri, PEER).await;

    assert_eq!(locked.status, StatusCode::GONE, "{:?}", locked.json());
    assert_eq!(unlocked.status, StatusCode::OK, "{:?}", unlocked.json());
}
