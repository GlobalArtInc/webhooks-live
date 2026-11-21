mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use common::{Harness, Jar, Options, SESSION_COOKIE, harness, harness_with, request, send};
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointRole, SessionLifetime, Uuid};

const ORIGIN: &str = "https://whl.test";

struct OriginCase<'a>(
    &'static str,
    &'a Jar,
    Vec<(header::HeaderName, &'static str)>,
    StatusCode,
);

async fn signed_in(harness: &Harness) -> Jar {
    let (reply, jar) = harness.sign_in(None).await;
    assert_eq!(reply.status, StatusCode::FOUND);
    jar
}

async fn post(
    harness: &Harness,
    uri: &str,
    jar: &Jar,
    headers: &[(header::HeaderName, &str)],
) -> common::Reply {
    send(&harness.app, request(Method::POST, uri, jar, headers)).await
}

#[tokio::test]
async fn protected_route_without_a_session_is_unauthenticated() {
    let harness = harness().await;
    let reply = harness.get("/api/v1/whoami", &Jar::default()).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.json()["error"]["code"], "unauthenticated");

    let maybe = harness.get("/api/v1/maybe", &Jar::default()).await;
    assert_eq!(maybe.json()["signed_in"], false);
}

#[tokio::test]
async fn malformed_and_unknown_session_cookies_are_anonymous() {
    let harness = harness().await;
    for value in ["x", &"a".repeat(43), &"a".repeat(500)] {
        let mut jar = Jar::default();
        jar.set(SESSION_COOKIE, value);
        let reply = harness.get("/api/v1/whoami", &jar).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "cookie {value:?}");
    }
}

#[tokio::test]
async fn optional_extractor_resolves_a_signed_in_user() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    let reply = harness.get("/api/v1/maybe", &jar).await;
    assert_eq!(reply.json()["signed_in"], true);
}

#[tokio::test]
async fn logout_deletes_the_session_and_clears_the_cookie() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::OK
    );

    let reply = post(&harness, "/auth/logout", &jar, &[(header::ORIGIN, ORIGIN)]).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let cleared = reply
        .set_cookie(SESSION_COOKIE)
        .expect("the cookie is cleared");
    assert!(cleared.contains("Max-Age=0"));
    assert!(cleared.contains("Secure"));
    assert_eq!(harness.count("sessions").await, 0);

    let again = harness.get("/api/v1/whoami", &jar).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_without_a_session_still_answers_204() {
    let harness = harness().await;
    let reply = post(&harness, "/auth/logout", &Jar::default(), &[]).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn logout_is_origin_checked() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    let reply = post(
        &harness,
        "/auth/logout",
        &jar,
        &[(header::ORIGIN, "https://evil.example")],
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.json()["error"]["code"], "csrf_rejected");
    assert_eq!(harness.count("sessions").await, 1);
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn logout_by_get_is_not_allowed() {
    let harness = harness().await;
    let reply = harness.get("/auth/logout", &Jar::default()).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn idle_timeout_expires_an_unused_session_and_activity_extends_it() {
    let harness = harness_with(Options {
        idle: Duration::from_secs(2),
        max: Duration::from_secs(3600),
        ..Options::default()
    })
    .await;
    let jar = signed_in(&harness).await;

    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::OK,
        "the touch at 1.2 s kept the session alive past the idle timeout since sign-in"
    );

    tokio::time::sleep(Duration::from_millis(2300)).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn absolute_lifetime_ends_a_session_that_is_still_active() {
    let harness = harness_with(Options {
        idle: Duration::from_secs(3600),
        max: Duration::from_secs(2),
        ..Options::default()
    })
    .await;
    let jar = signed_in(&harness).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::OK
    );

    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn expired_sessions_and_login_states_are_purged() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    harness.begin(None).await;
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the session can be aged");
    sqlx::query("UPDATE login_states SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the login state can be aged");

    assert_eq!(
        harness
            .store
            .delete_expired_sessions(Duration::from_secs(3600))
            .await
            .expect("sessions purge"),
        1
    );
    assert_eq!(
        harness
            .store
            .delete_expired_login_states()
            .await
            .expect("login states purge"),
        1
    );
    assert_eq!(
        harness.get("/api/v1/whoami", &jar).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn origin_check_table_on_a_mutating_route() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    let anonymous = Jar::default();
    let cases: [OriginCase; 7] = [
        OriginCase(
            "same origin",
            &jar,
            vec![(header::ORIGIN, ORIGIN)],
            StatusCode::NO_CONTENT,
        ),
        OriginCase(
            "other origin",
            &jar,
            vec![(header::ORIGIN, "https://evil.example")],
            StatusCode::FORBIDDEN,
        ),
        OriginCase(
            "missing origin, same-origin referer",
            &jar,
            vec![(header::REFERER, "https://whl.test/e/abc")],
            StatusCode::NO_CONTENT,
        ),
        OriginCase(
            "missing origin, foreign referer",
            &jar,
            vec![(header::REFERER, "https://evil.example/")],
            StatusCode::FORBIDDEN,
        ),
        OriginCase("missing both", &jar, vec![], StatusCode::FORBIDDEN),
        OriginCase(
            "no session cookie is not checked",
            &anonymous,
            vec![(header::ORIGIN, "https://evil.example")],
            StatusCode::NO_CONTENT,
        ),
        OriginCase(
            "null origin",
            &jar,
            vec![(header::ORIGIN, "null")],
            StatusCode::FORBIDDEN,
        ),
    ];
    for OriginCase(name, cookies, headers, expected) in cases {
        let reply = post(&harness, "/api/v1/change", cookies, &headers).await;
        assert_eq!(reply.status, expected, "{name}");
        if expected == StatusCode::FORBIDDEN {
            assert_eq!(reply.json()["error"]["code"], "csrf_rejected", "{name}");
        }
    }
}

#[tokio::test]
async fn origin_check_covers_every_mutating_method_and_never_get() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    let foreign = [(header::ORIGIN, "https://evil.example")];
    for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
        let reply = send(
            &harness.app,
            request(method.clone(), "/api/v1/change", &jar, &foreign),
        )
        .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method}");
    }
    let read = send(
        &harness.app,
        request(Method::GET, "/api/v1/whoami", &jar, &foreign),
    )
    .await;
    assert_eq!(read.status, StatusCode::OK);
}

#[tokio::test]
async fn endpoint_roles_follow_ownership_and_membership() {
    let harness = harness().await;
    let pool = harness.db.pool();
    let mut users = Vec::new();
    for subject in ["owner", "editor", "viewer", "stranger"] {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (id, issuer, subject, email) \
             VALUES (gen_random_uuid(), 'issuer', $1, $2) RETURNING id",
        )
        .bind(subject)
        .bind(format!("{subject}@example.test"))
        .fetch_one(pool)
        .await
        .expect("the user inserts");
        users.push(id);
    }
    let endpoint = EndpointId::generate().expect("an id is generated");
    sqlx::query("INSERT INTO endpoints (id, owner_id) VALUES ($1, $2)")
        .bind(endpoint.unpack())
        .bind(users[0])
        .execute(pool)
        .await
        .expect("the endpoint inserts");
    for (user, role) in [(users[1], "editor"), (users[2], "viewer")] {
        sqlx::query(
            "INSERT INTO endpoint_members (endpoint_id, user_id, role) VALUES ($1, $2, $3)",
        )
        .bind(endpoint.unpack())
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .expect("the member inserts");
    }

    let retention = Duration::from_secs(3600);
    let role = |user: Uuid| {
        let store = harness.store.clone();
        let endpoint = endpoint.clone();
        async move {
            store
                .endpoint_role(&endpoint, user, retention)
                .await
                .expect("the role query runs")
        }
    };
    assert_eq!(role(users[0]).await, Some(EndpointRole::Owner));
    assert_eq!(role(users[1]).await, Some(EndpointRole::Editor));
    assert_eq!(role(users[2]).await, Some(EndpointRole::Viewer));
    assert_eq!(role(users[3]).await, None);

    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(endpoint.unpack())
        .execute(pool)
        .await
        .expect("the endpoint soft-deletes");
    assert_eq!(role(users[0]).await, None);
}

#[tokio::test]
async fn session_lifetime_values_are_applied_in_sql() {
    let harness = harness().await;
    let jar = signed_in(&harness).await;
    assert_eq!(jar.get(SESSION_COOKIE).map(str::len), Some(43));
    let lifetime = SessionLifetime {
        idle: Duration::from_secs(1),
        touch_after: Duration::from_secs(1),
    };
    let hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(harness.db.pool())
        .await
        .expect("the session row exists");
    assert!(
        harness
            .store
            .session_user(&hash, lifetime)
            .await
            .expect("the lookup runs")
            .is_some()
    );
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '5 seconds'")
        .execute(harness.db.pool())
        .await
        .expect("the session can be aged");
    assert!(
        harness
            .store
            .session_user(&hash, lifetime)
            .await
            .expect("the lookup runs")
            .is_none()
    );
}
