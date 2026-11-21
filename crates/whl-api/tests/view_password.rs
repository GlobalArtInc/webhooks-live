mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use common::matrix::{cast, error_code, fixture, routes, uri};
use common::tokens::mint;
use common::{
    RETENTION, Reply, SsoHarness, UNKNOWN_ID, api_settings, field_codes, harness, mixed_harness,
    request, send,
};
use serde_json::json;
use sqlx::PgPool;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_api::ApiState;
use whl_auth::{
    AccessControl, FailureSpec, GRANT_HEADER, ViewAccess, ViewGrant, ViewLimitSettings,
    ViewPasswords,
};
use whl_core::creator_net::CreatorNet;
use whl_core::rate::{RatePeriod, RateSpec};
use whl_store::Store;
use whl_testkit::TestDb;

const PASSWORD: &str = "sample-view-phrase";
const OTHER: &str = "unrelated-view-phrase";
const HOME: &str = "203.0.113.7";
const STUB_HASH: &str = "stub-hash-for-direct-inserts";
const TICK: Duration = Duration::from_millis(150);
const NEVER: Duration = Duration::from_secs(3600);

fn with_peer(mut request: Request<Body>, peer: &str) -> Request<Body> {
    let address = SocketAddr::new(peer.parse().expect("a valid peer address"), 40000);
    request.extensions_mut().insert(ConnectInfo(address));
    request
}

async fn call(app: &Router, method: Method, uri: &str, peer: &str, body: &str) -> Reply {
    send(app, with_peer(request(method, uri, body), peer)).await
}

async fn call_with_grant(
    app: &Router,
    method: Method,
    uri: &str,
    grant: &str,
    body: &str,
) -> Reply {
    let mut built = request(method, uri, body);
    built
        .headers_mut()
        .insert(GRANT_HEADER, grant.parse().expect("a header value"));
    send(app, with_peer(built, HOME)).await
}

async fn create_protected(app: &Router) -> String {
    let body = json!({ "view_password": PASSWORD }).to_string();
    let reply = call(app, Method::POST, "/api/v1/endpoints", HOME, &body).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    reply.json()["id"].as_str().expect("an id").to_owned()
}

async fn unlock(app: &Router, endpoint: &str, peer: &str, password: &str) -> Reply {
    let body = json!({ "password": password }).to_string();
    call(
        app,
        Method::POST,
        &format!("/api/v1/endpoints/{endpoint}/unlock"),
        peer,
        &body,
    )
    .await
}

async fn app_with(limits: ViewLimitSettings) -> (TestDb, Router) {
    app_with_passwords(limits, ViewPasswords::default()).await
}

async fn app_with_passwords(
    limits: ViewLimitSettings,
    passwords: ViewPasswords,
) -> (TestDb, Router) {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let views = ViewAccess::with_settings(store.clone(), RETENTION, passwords, &limits);
    let access = AccessControl::open().with_views(views);
    let app = whl_api::router(ApiState::new(store, api_settings()).with_access(access));
    (db, app)
}

fn endpoint_uri(endpoint: &str) -> String {
    format!("/api/v1/endpoints/{endpoint}")
}

async fn insert_grant(pool: &PgPool, endpoint: &str) -> String {
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

async fn protect(pool: &PgPool, endpoint: &str) -> String {
    sqlx::query("UPDATE endpoints SET view_password_hash = $2 WHERE id = $1")
        .bind(endpoint)
        .bind(STUB_HASH)
        .execute(pool)
        .await
        .expect("the endpoint is protected");
    insert_grant(pool, endpoint).await
}

async fn grants(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM endpoint_grants")
        .fetch_one(pool)
        .await
        .expect("the grants are counted")
}

async fn hash_is_null(pool: &PgPool, endpoint: &str) -> bool {
    sqlx::query_scalar("SELECT view_password_hash IS NULL FROM endpoints WHERE id = $1")
        .bind(endpoint)
        .fetch_one(pool)
        .await
        .expect("the endpoint row exists")
}

async fn mixed_create(harness: &SsoHarness, body: &str) -> Reply {
    harness
        .send_with(
            None,
            Some(HOME),
            &[],
            Method::POST,
            "/api/v1/endpoints",
            body,
        )
        .await
}

async fn mixed_unlock(harness: &SsoHarness, endpoint: &str, password: &str) -> Reply {
    harness
        .send_with(
            None,
            Some(HOME),
            &[],
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/unlock"),
            &json!({ "password": password }).to_string(),
        )
        .await
}

async fn mixed_get(harness: &SsoHarness, endpoint: &str, grant: Option<&str>) -> Reply {
    let extra: Vec<(&str, &str)> = grant
        .map(|grant| (GRANT_HEADER, grant))
        .into_iter()
        .collect();
    harness
        .send_with(
            None,
            Some(HOME),
            &extra,
            Method::GET,
            &endpoint_uri(endpoint),
            "",
        )
        .await
}

#[tokio::test]
async fn an_endpoint_created_with_a_password_is_protected_and_nothing_returns_the_password() {
    let harness = harness().await;
    let body = json!({ "view_password": PASSWORD }).to_string();

    let created = call(&harness.app, Method::POST, "/api/v1/endpoints", HOME, &body).await;

    assert_eq!(created.status, StatusCode::CREATED);
    let text = String::from_utf8_lossy(&created.body).into_owned();
    assert_eq!(created.json()["view_protected"], true);
    assert!(
        !text.contains(PASSWORD) && !text.contains("argon2"),
        "{text}"
    );
    let stored: String = sqlx::query_scalar("SELECT view_password_hash FROM endpoints")
        .fetch_one(harness.db.pool())
        .await
        .expect("the hash is stored");
    assert!(stored.starts_with("$argon2id$"), "{stored}");
    assert!(!stored.contains(PASSWORD));
    let id = created.json()["id"].as_str().expect("an id").to_owned();
    let grant = created.json()["view_grant"]["grant"]
        .as_str()
        .expect("the setter receives a grant")
        .to_owned();
    assert!(created.json()["view_grant"]["expires_at"].is_string());
    let opened = call_with_grant(&harness.app, Method::GET, &endpoint_uri(&id), &grant, "").await;
    assert_eq!(opened.status, StatusCode::OK);
}

#[tokio::test]
async fn the_right_password_unlocks_a_twelve_hour_grant_and_a_wrong_one_does_not() {
    let harness = harness().await;
    let endpoint = create_protected(&harness.app).await;
    let target = endpoint_uri(&endpoint);

    let locked = call(&harness.app, Method::GET, &target, HOME, "").await;
    let wrong = unlock(&harness.app, &endpoint, HOME, OTHER).await;
    let unlocked = unlock(&harness.app, &endpoint, HOME, PASSWORD).await;

    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&locked), "grant_required");
    assert!(locked.headers.get(header::WWW_AUTHENTICATE).is_none());
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&wrong), "wrong_password");
    assert_eq!(unlocked.status, StatusCode::OK);
    let grant = unlocked.json()["grant"]
        .as_str()
        .expect("a grant")
        .to_owned();
    assert_eq!(grant.len(), 43);
    let expires = OffsetDateTime::parse(
        unlocked.json()["expires_at"].as_str().expect("an expiry"),
        &Rfc3339,
    )
    .expect("an RFC 3339 expiry");
    let lifetime = expires - OffsetDateTime::now_utc();
    assert!(
        lifetime > time::Duration::minutes(11 * 60 + 59) && lifetime <= time::Duration::hours(12),
        "{lifetime}"
    );
    let opened = call_with_grant(&harness.app, Method::GET, &target, &grant, "").await;
    assert_eq!(opened.status, StatusCode::OK);
    assert_eq!(opened.json()["view_protected"], true);
    let other = ViewGrant::generate().expect("entropy is available");
    let forged = call_with_grant(&harness.app, Method::GET, &target, other.unpack(), "").await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_wrong_password_an_unknown_endpoint_and_an_unprotected_one_answer_alike() {
    let harness = harness().await;
    let protected = create_protected(&harness.app).await;
    let plain = common::create(&harness.app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();

    let wrong = unlock(&harness.app, &protected, "198.51.100.1", OTHER).await;
    let unknown = unlock(&harness.app, UNKNOWN_ID, "198.51.100.2", PASSWORD).await;
    let unprotected = unlock(&harness.app, &plain, "198.51.100.3", PASSWORD).await;

    for reply in [&wrong, &unknown, &unprotected] {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(reply), "wrong_password");
        assert_eq!(reply.body, wrong.body);
    }
}

#[tokio::test]
async fn five_wrong_attempts_block_that_address_and_a_correct_password_elsewhere_still_unlocks() {
    let harness = harness().await;
    let endpoint = create_protected(&harness.app).await;
    for attempt in 1..=5 {
        let wrong = unlock(&harness.app, &endpoint, "198.51.100.1", "short").await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "attempt {attempt}");
    }

    let blocked = unlock(&harness.app, &endpoint, "198.51.100.1", PASSWORD).await;
    let elsewhere = unlock(&harness.app, &endpoint, "198.51.100.2", PASSWORD).await;

    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&blocked), "rate_limited");
    assert!(blocked.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(elsewhere.status, StatusCode::OK);
}

#[tokio::test]
async fn correct_passwords_never_consume_the_wrong_attempt_budget() {
    let harness = harness().await;
    let endpoint = create_protected(&harness.app).await;

    for attempt in 1..=6 {
        let right = unlock(&harness.app, &endpoint, "198.51.100.1", PASSWORD).await;
        assert_eq!(right.status, StatusCode::OK, "attempt {attempt}");
    }

    let wrong = unlock(&harness.app, &endpoint, "198.51.100.1", "short").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_endpoint_wide_fallback_blocks_every_address_after_enough_wrong_attempts() {
    let (_db, app) = app_with(ViewLimitSettings {
        endpoint_failures: FailureSpec {
            max_failures: 3,
            window: Duration::from_secs(60),
        },
        ..ViewLimitSettings::default()
    })
    .await;
    let endpoint = create_protected(&app).await;
    for last in 1..=3 {
        let wrong = unlock(&app, &endpoint, &format!("198.51.100.{last}"), "short").await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "attempt {last}");
    }

    let blocked = unlock(&app, &endpoint, "198.51.100.99", PASSWORD).await;

    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&blocked), "rate_limited");
}

#[tokio::test]
async fn unlocking_an_unknown_endpoint_takes_no_hash_slot_and_setting_a_password_has_its_own() {
    let (_db, app) = app_with_passwords(
        ViewLimitSettings::default(),
        ViewPasswords::with_capacity(1, 0, Duration::from_millis(50)),
    )
    .await;
    let protected = create_protected(&app).await;

    let unknown = unlock(&app, UNKNOWN_ID, "198.51.100.1", PASSWORD).await;
    let real = unlock(&app, &protected, "198.51.100.2", PASSWORD).await;
    let body = json!({ "view_password": PASSWORD }).to_string();
    let another = call(&app, Method::POST, "/api/v1/endpoints", HOME, &body).await;

    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&unknown), "wrong_password");
    assert_eq!(real.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_code(&real), "password_busy");
    assert_eq!(another.status, StatusCode::CREATED);
}

#[tokio::test]
async fn the_address_limit_stops_a_guesser_who_rotates_endpoints() {
    let harness = harness().await;
    for number in 0..20 {
        let attempt = unlock(&harness.app, &format!("{number:024}"), HOME, "short").await;
        assert_eq!(attempt.status, StatusCode::UNAUTHORIZED, "attempt {number}");
    }

    let limited = unlock(&harness.app, &format!("{:024}", 99), HOME, "short").await;
    let elsewhere = unlock(
        &harness.app,
        &format!("{:024}", 98),
        "198.51.100.9",
        "short",
    )
    .await;

    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(elsewhere.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_unlock_body_must_be_a_json_object_with_only_a_password_string() {
    let harness = harness().await;
    let target = format!("/api/v1/endpoints/{UNKNOWN_ID}/unlock");
    let post = |body: &'static str| {
        let app = harness.app.clone();
        let target = target.clone();
        async move { call(&app, Method::POST, &target, HOME, body).await }
    };

    let missing = post("{}").await;
    let wrong_type = post(r#"{"password": 5}"#).await;
    let extra = post(r#"{"password": "short", "extra": 1}"#).await;
    let array = post("[1]").await;

    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&missing.json()),
        [("password".to_owned(), "required".to_owned())]
    );
    assert_eq!(
        field_codes(&wrong_type.json()),
        [("password".to_owned(), "invalid_format".to_owned())]
    );
    assert_eq!(
        field_codes(&extra.json()),
        [("extra".to_owned(), "not_allowed".to_owned())]
    );
    assert_eq!(array.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn password_length_is_validated_before_anything_is_stored() {
    let harness = mixed_harness().await;
    for (value, code) in [
        (json!("1234567"), "out_of_range"),
        (json!("x".repeat(129)), "too_long"),
        (json!(5), "invalid_format"),
    ] {
        let body = json!({ "view_password": value }).to_string();
        let reply = mixed_create(&harness, &body).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            field_codes(&reply.json()),
            [("view_password".to_owned(), code.to_owned())]
        );
    }

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoints")
        .fetch_one(harness.db.pool())
        .await
        .expect("the endpoints are counted");
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn every_endpoint_route_needs_the_grant_and_works_with_it() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    for route in routes().iter().filter(|route| !route.members) {
        let fixture = fixture(&harness, &cast, false).await;
        let grant = protect(harness.db.pool(), &fixture.endpoint).await;
        let target = uri(route, &fixture, &cast);
        let label = format!("{} {}", route.method, route.path);

        let locked = harness
            .send_with(
                None,
                Some(HOME),
                &[],
                route.method.clone(),
                &target,
                route.body,
            )
            .await;
        let granted = harness
            .send_with(
                None,
                Some(HOME),
                &[(GRANT_HEADER, &grant)],
                route.method.clone(),
                &target,
                route.body,
            )
            .await;

        assert_eq!(locked.status, StatusCode::UNAUTHORIZED, "{label}");
        assert_eq!(error_code(&locked), "grant_required", "{label}");
        assert_eq!(granted.status, route.success, "{label}");
    }
}

#[tokio::test]
async fn a_grant_stops_working_when_it_expires() {
    let harness = mixed_harness().await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    assert_eq!(
        mixed_get(&harness, &endpoint, Some(&grant)).await.status,
        StatusCode::OK
    );

    sqlx::query("UPDATE endpoint_grants SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the grant is aged");

    let expired = mixed_get(&harness, &endpoint, Some(&grant)).await;
    let malformed = mixed_get(&harness, &endpoint, Some("not-a-grant")).await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&expired), "grant_required");
    assert_eq!(malformed.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changing_the_password_revokes_every_grant() {
    let harness = mixed_harness().await;
    let endpoint = mixed_create(&harness, &json!({ "view_password": PASSWORD }).to_string())
        .await
        .json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let first = mixed_unlock(&harness, &endpoint, PASSWORD).await.json()["grant"]
        .as_str()
        .expect("a grant")
        .to_owned();
    let second = mixed_unlock(&harness, &endpoint, PASSWORD).await.json()["grant"]
        .as_str()
        .expect("a grant")
        .to_owned();

    let changed = harness
        .send_with(
            None,
            Some(HOME),
            &[(GRANT_HEADER, &first)],
            Method::PATCH,
            &endpoint_uri(&endpoint),
            &json!({ "view_password": OTHER }).to_string(),
        )
        .await;

    assert_eq!(changed.status, StatusCode::OK, "{:?}", changed.json());
    assert_eq!(changed.json()["view_protected"], true);
    let fresh = changed.json()["view_grant"]["grant"]
        .as_str()
        .expect("the setter receives a fresh grant")
        .to_owned();
    assert_eq!(grants(harness.db.pool()).await, 1);
    for grant in [&first, &second] {
        let reply = mixed_get(&harness, &endpoint, Some(grant)).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        mixed_get(&harness, &endpoint, Some(&fresh)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        mixed_unlock(&harness, &endpoint, PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mixed_unlock(&harness, &endpoint, OTHER).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn removing_the_password_revokes_every_grant_and_opens_the_endpoint() {
    let harness = mixed_harness().await;
    let endpoint = mixed_create(&harness, &json!({ "view_password": PASSWORD }).to_string())
        .await
        .json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let grant = insert_grant(harness.db.pool(), &endpoint).await;

    let removed = harness
        .send_with(
            None,
            Some(HOME),
            &[(GRANT_HEADER, &grant)],
            Method::PATCH,
            &endpoint_uri(&endpoint),
            r#"{"view_password": null}"#,
        )
        .await;

    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(removed.json()["view_protected"], false);
    assert_eq!(grants(harness.db.pool()).await, 0);
    assert!(hash_is_null(harness.db.pool(), &endpoint).await);
    assert_eq!(
        mixed_get(&harness, &endpoint, None).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_password_can_be_added_to_an_open_endpoint_without_a_grant() {
    let harness = mixed_harness().await;
    let endpoint = harness.anonymous_endpoint(None).await;

    let set = harness
        .send_with(
            None,
            Some(HOME),
            &[],
            Method::PATCH,
            &endpoint_uri(&endpoint),
            &json!({ "view_password": PASSWORD }).to_string(),
        )
        .await;

    assert_eq!(set.status, StatusCode::OK, "{:?}", set.json());
    assert_eq!(set.json()["view_protected"], true);
    let grant = set.json()["view_grant"]["grant"]
        .as_str()
        .expect("the setter receives a grant")
        .to_owned();
    let locked = mixed_get(&harness, &endpoint, None).await;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        mixed_get(&harness, &endpoint, Some(&grant)).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_password_is_hashed_only_for_a_valid_change_to_a_live_endpoint() {
    let (_db, app) = app_with(ViewLimitSettings {
        password_set_ip: RateSpec {
            count: 1,
            period: RatePeriod::Minute,
            burst: 1,
        },
        ..ViewLimitSettings::default()
    })
    .await;
    let endpoint = common::create(&app, "{}").await["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let body = json!({ "view_password": PASSWORD }).to_string();
    let invalid = json!({ "view_password": PASSWORD, "title": 5 }).to_string();

    let gone = call(&app, Method::PATCH, &endpoint_uri(UNKNOWN_ID), HOME, &body).await;
    let refused = call(
        &app,
        Method::PATCH,
        &endpoint_uri(&endpoint),
        HOME,
        &invalid,
    )
    .await;
    let accepted = call(&app, Method::PATCH, &endpoint_uri(&endpoint), HOME, &body).await;

    assert_eq!(gone.status, StatusCode::GONE);
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&refused.json()),
        [("title".to_owned(), "invalid_format".to_owned())]
    );
    assert_eq!(accepted.status, StatusCode::OK, "{:?}", accepted.json());
}

#[tokio::test]
async fn an_endpoint_with_an_owner_refuses_a_view_password() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let owned = harness.endpoint_for(&owner).await;
    let create_body = json!({ "view_password": PASSWORD }).to_string();

    let created = harness
        .send_with(
            Some(&owner),
            Some(HOME),
            &[],
            Method::POST,
            "/api/v1/endpoints",
            &create_body,
        )
        .await;
    let set = harness
        .send_with(
            Some(&owner),
            Some(HOME),
            &[],
            Method::PATCH,
            &endpoint_uri(&owned),
            &create_body,
        )
        .await;
    let removed = harness
        .send_with(
            Some(&owner),
            Some(HOME),
            &[],
            Method::PATCH,
            &endpoint_uri(&owned),
            r#"{"view_password": null}"#,
        )
        .await;

    for reply in [&created, &set, &removed] {
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            field_codes(&reply.json()),
            [("view_password".to_owned(), "not_allowed".to_owned())]
        );
    }
    assert!(hash_is_null(harness.db.pool(), &owned).await);
}

#[tokio::test]
async fn members_of_an_owned_endpoint_need_no_grant_and_strangers_still_get_gone() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast, true).await;
    let target = endpoint_uri(&fixture.endpoint);

    for (actor, expected) in [
        (Some(&cast.owner), StatusCode::OK),
        (Some(&cast.editor), StatusCode::OK),
        (Some(&cast.viewer), StatusCode::OK),
        (Some(&cast.stranger), StatusCode::GONE),
        (None, StatusCode::GONE),
    ] {
        let reply = harness.send_as(actor, Method::GET, &target, "").await;
        assert_eq!(reply.status, expected);
    }
}

#[tokio::test]
async fn neither_a_session_nor_an_api_token_bypasses_the_password() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let minted = mint(&harness, &user, &["read"], None).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let target = endpoint_uri(&endpoint);
    let bearer = format!("Bearer {}", minted.token);

    let session = harness.send_as(Some(&user), Method::GET, &target, "").await;
    let token = harness
        .send_with(
            None,
            None,
            &[("authorization", &bearer)],
            Method::GET,
            &target,
            "",
        )
        .await;
    let token_with_grant = harness
        .send_with(
            None,
            None,
            &[("authorization", &bearer), (GRANT_HEADER, &grant)],
            Method::GET,
            &target,
            "",
        )
        .await;

    for reply in [&session, &token] {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(reply), "grant_required");
    }
    assert_eq!(token_with_grant.status, StatusCode::OK);
}

#[tokio::test]
async fn claiming_a_protected_endpoint_needs_the_grant_and_drops_the_password() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let network = CreatorNet::from_ip(HOME.parse().expect("a valid address"));
    let endpoint = harness.anonymous_endpoint(Some(&network)).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let target = format!("/api/v1/endpoints/{endpoint}/claim");

    let refused = harness
        .send_from(Some(&user), Some(HOME), Method::POST, &target, "")
        .await;
    let claimed = harness
        .send_with(
            Some(&user),
            Some(HOME),
            &[(GRANT_HEADER, &grant)],
            Method::POST,
            &target,
            "",
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&refused), "grant_required");
    assert_eq!(claimed.status, StatusCode::OK, "{:?}", claimed.json());
    assert_eq!(claimed.json()["view_protected"], false);
    assert!(hash_is_null(harness.db.pool(), &endpoint).await);
    assert_eq!(grants(harness.db.pool()).await, 0);
    let owner_read = harness
        .send_as(Some(&user), Method::GET, &endpoint_uri(&endpoint), "")
        .await;
    assert_eq!(owner_read.status, StatusCode::OK);
}

#[tokio::test]
async fn the_stream_takes_the_grant_in_a_header_or_the_query_and_no_other_route_takes_the_query() {
    let harness = mixed_harness().await;
    let app = harness.live_app(NEVER).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let stream = format!("/api/v1/endpoints/{endpoint}/stream");

    let locked = app.open_with_headers(&stream, &[]).await;
    let malformed = app
        .open_with_headers(&format!("{stream}?grant=not-a-grant"), &[])
        .await;
    let mut by_query = app
        .open_with_headers(&format!("{stream}?last_event_id=0&grant={grant}"), &[])
        .await;
    let mut by_header = app.open_with_headers(&stream, &[(GRANT_HEADER, &grant)]).await;
    let elsewhere = harness
        .send_with(
            None,
            Some(HOME),
            &[],
            Method::GET,
            &format!("{}?grant={grant}", endpoint_uri(&endpoint)),
            "",
        )
        .await;

    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert_eq!(malformed.status, StatusCode::UNAUTHORIZED);
    assert_eq!(by_query.status, StatusCode::OK);
    by_query.expect_event("ready").await;
    assert_eq!(by_header.status, StatusCode::OK);
    by_header.expect_event("ready").await;
    assert_eq!(elsewhere.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changing_the_password_ends_a_stream_that_was_opened_with_the_old_grant() {
    let harness = mixed_harness().await;
    let app = harness.live_app(TICK).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let grant = protect(harness.db.pool(), &endpoint).await;
    let mut stream = app
        .open_with_headers(
            &format!("/api/v1/endpoints/{endpoint}/stream?grant={grant}"),
            &[],
        )
        .await;
    assert_eq!(stream.status, StatusCode::OK);
    stream.expect_event("ready").await;

    let changed = harness
        .send_with(
            None,
            Some(HOME),
            &[(GRANT_HEADER, &grant)],
            Method::PATCH,
            &endpoint_uri(&endpoint),
            &json!({ "view_password": OTHER }).to_string(),
        )
        .await;

    assert_eq!(changed.status, StatusCode::OK, "{:?}", changed.json());
    stream.expect_end_after_bye().await;
}

#[tokio::test]
async fn setting_passwords_is_limited_per_address() {
    let (db, app) = app_with(ViewLimitSettings {
        password_set_ip: RateSpec {
            count: 2,
            period: RatePeriod::Minute,
            burst: 2,
        },
        ..ViewLimitSettings::default()
    })
    .await;
    let body = json!({ "view_password": PASSWORD }).to_string();

    let mut statuses = Vec::new();
    for _ in 0..3 {
        statuses.push(call(&app, Method::POST, "/api/v1/endpoints", HOME, &body).await);
    }
    let elsewhere = call(
        &app,
        Method::POST,
        "/api/v1/endpoints",
        "198.51.100.9",
        &body,
    )
    .await;

    assert_eq!(statuses[0].status, StatusCode::CREATED);
    assert_eq!(statuses[1].status, StatusCode::CREATED);
    assert_eq!(statuses[2].status, StatusCode::TOO_MANY_REQUESTS);
    assert!(statuses[2].headers.contains_key(header::RETRY_AFTER));
    assert_eq!(elsewhere.status, StatusCode::CREATED);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoints")
        .fetch_one(db.pool())
        .await
        .expect("the endpoints are counted");
    assert_eq!(stored, 3);
}

#[tokio::test]
async fn claiming_an_endpoint_ends_the_stream_of_its_anonymous_viewers_at_once() {
    let harness = mixed_harness().await;
    let app = harness.live_app(NEVER).await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let network = CreatorNet::from_ip(HOME.parse().expect("a valid address"));
    let endpoint = harness.anonymous_endpoint(Some(&network)).await;
    let mut stream = app
        .open_with_headers(&format!("/api/v1/endpoints/{endpoint}/stream"), &[])
        .await;
    assert_eq!(stream.status, StatusCode::OK);
    stream.expect_event("ready").await;

    let claimed = harness
        .send_from(
            Some(&user),
            Some(HOME),
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/claim"),
            "",
        )
        .await;

    assert_eq!(claimed.status, StatusCode::OK, "{:?}", claimed.json());
    stream.expect_end_after_bye().await;
}
