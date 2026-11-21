mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use axum::routing::{get, post};
use common::{Harness, Jar, Reply, send};
use serde_json::{Value, json};
use whl_auth::{
    AccessControl, ApiTokenSecret, Caller, EndpointParam, Requirement, TokenPolicy, TokenScope,
};
use whl_core::limiter::{LimiterSettings, Limiters};
use whl_store::{CreateTokenOutcome, NewApiToken, TokenCaps, Uuid};

const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);
const ORIGIN: &str = "https://whl.test";
const CAPS: TokenCaps = TokenCaps {
    max_active: 50,
    max_per_hour: 50,
};

#[derive(Clone)]
struct Gate {
    access: AccessControl,
    requirement: Requirement,
    policy: TokenPolicy,
}

async fn gate(
    State(gate): State<Gate>,
    params: Result<axum::extract::RawPathParams, axum::extract::rejection::RawPathParamsRejection>,
    request: Request,
    next: Next,
) -> Response {
    let id = EndpointParam::from_path(&params, "id");
    gate.access
        .enforce_for(gate.requirement, gate.policy, id, request, next)
        .await
}

async fn who(caller: Caller) -> axum::Json<Value> {
    let kind = match &caller {
        Caller::Anonymous { .. } => "anonymous",
        Caller::Session { .. } => "session",
        Caller::Token { .. } => "token",
    };
    axum::Json(json!({
        "kind": kind,
        "email": caller.user().map(|user| user.email.clone()),
    }))
}

fn guarded(access: &AccessControl, requirement: Requirement, policy: TokenPolicy) -> Gate {
    Gate {
        access: access.clone(),
        requirement,
        policy,
    }
}

fn probes(access: &AccessControl, harness: &Harness) -> Router {
    let read = TokenPolicy::Scope(TokenScope::Read);
    let write = TokenPolicy::Scope(TokenScope::CapturesWrite);
    Router::new()
        .merge(
            Router::new()
                .route("/api/v1/read", get(who))
                .route_layer(from_fn_with_state(
                    guarded(access, Requirement::SignedIn, read),
                    gate,
                )),
        )
        .merge(
            Router::new()
                .route("/api/v1/write", post(who))
                .route_layer(from_fn_with_state(
                    guarded(access, Requirement::SignedIn, write),
                    gate,
                )),
        )
        .merge(
            Router::new()
                .route("/api/v1/session-only", post(who))
                .route_layer(from_fn_with_state(
                    guarded(access, Requirement::SignedIn, TokenPolicy::SessionOnly),
                    gate,
                )),
        )
        .merge(whl_auth::router(harness.auth.clone()))
        .layer(harness.auth.origin_check())
}

async fn session_user(harness: &Harness) -> (Jar, Uuid) {
    let (reply, jar) = harness.sign_in(None).await;
    assert_eq!(reply.status, StatusCode::FOUND);
    let id: Uuid = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(harness.db.pool())
        .await
        .expect("the signed-in user exists");
    (jar, id)
}

async fn other_user(harness: &Harness) -> Uuid {
    harness
        .store
        .upsert_user(&whl_store::VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: "subject-other".to_owned(),
            email: "other@example.test".to_owned(),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored")
        .id
}

struct Issued {
    secret: ApiTokenSecret,
    id: Uuid,
}

async fn issue(harness: &Harness, user: Uuid, scopes: &[TokenScope]) -> Issued {
    let secret = ApiTokenSecret::generate().expect("entropy is available");
    issue_with(harness, user, scopes, &secret).await
}

async fn issue_with(
    harness: &Harness,
    user: Uuid,
    scopes: &[TokenScope],
    secret: &ApiTokenSecret,
) -> Issued {
    let outcome = harness
        .store
        .create_api_token(
            user,
            &NewApiToken {
                name: "probe",
                prefix: secret.lookup_prefix(),
                token_hash: secret.digest().unpack(),
                scopes,
                endpoint_ids: None,
                lifetime: Duration::from_secs(3600),
            },
            CAPS,
        )
        .await
        .expect("the token is stored");
    let CreateTokenOutcome::Created(record) = outcome else {
        panic!("the token was not created");
    };
    Issued {
        secret: secret.clone(),
        id: record.id,
    }
}

fn call(
    method: Method,
    uri: &str,
    jar: Option<&Jar>,
    bearer: Option<&str>,
    origin: Option<&str>,
) -> Request {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = jar.and_then(Jar::header) {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    let mut request = builder.body(Body::empty()).expect("the request is valid");
    let address = SocketAddr::new("203.0.113.9".parse().expect("an address"), 50000);
    request.extensions_mut().insert(ConnectInfo(address));
    request
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn mixed(harness: &Harness) -> AccessControl {
    AccessControl::mixed(harness.auth.clone(), RETENTION)
}

#[tokio::test]
async fn a_bearer_post_needs_no_origin_while_a_cookie_post_still_does() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (jar, user) = session_user(&harness).await;
    let token = issue(&harness, user, &[TokenScope::CapturesWrite]).await;
    let uri = "/api/v1/write";

    let bearer = send(
        &app,
        call(Method::POST, uri, None, Some(token.secret.unpack()), None),
    )
    .await;
    let cookie_bare = send(&app, call(Method::POST, uri, Some(&jar), None, None)).await;
    let cookie_origin = send(
        &app,
        call(Method::POST, uri, Some(&jar), None, Some(ORIGIN)),
    )
    .await;

    assert_eq!(bearer.status, StatusCode::OK);
    assert_eq!(bearer.json()["kind"], "token");
    assert_eq!(cookie_bare.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&cookie_bare), "csrf_rejected");
    assert_eq!(cookie_origin.status, StatusCode::OK);
    assert_eq!(cookie_origin.json()["kind"], "session");
}

#[tokio::test]
async fn a_bearer_header_does_not_lift_the_origin_check_outside_the_api() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (jar, user) = session_user(&harness).await;
    let token = issue(&harness, user, &[TokenScope::Read]).await;

    let logout = send(
        &app,
        call(
            Method::POST,
            "/auth/logout",
            Some(&jar),
            Some(token.secret.unpack()),
            None,
        ),
    )
    .await;

    assert_eq!(logout.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&logout), "csrf_rejected");
    assert_eq!(harness.count("sessions").await, 1);
}

#[tokio::test]
async fn a_request_with_both_a_cookie_and_a_bearer_is_judged_by_the_bearer_alone() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (jar, session_owner) = session_user(&harness).await;
    let other = other_user(&harness).await;
    let narrow = issue(&harness, other, &[TokenScope::Read]).await;
    let uri_read = "/api/v1/read";
    let uri_write = "/api/v1/write";
    let uri_session = "/api/v1/session-only";

    let session_alone = send(
        &app,
        call(Method::POST, uri_write, Some(&jar), None, Some(ORIGIN)),
    )
    .await;
    let acts_as_token = send(
        &app,
        call(
            Method::GET,
            uri_read,
            Some(&jar),
            Some(narrow.secret.unpack()),
            None,
        ),
    )
    .await;
    let no_session_power = send(
        &app,
        call(
            Method::POST,
            uri_write,
            Some(&jar),
            Some(narrow.secret.unpack()),
            Some(ORIGIN),
        ),
    )
    .await;
    let session_route = send(
        &app,
        call(
            Method::POST,
            uri_session,
            Some(&jar),
            Some(narrow.secret.unpack()),
            Some(ORIGIN),
        ),
    )
    .await;
    let session_route_alone = send(
        &app,
        call(Method::POST, uri_session, Some(&jar), None, Some(ORIGIN)),
    )
    .await;
    let bad_bearer = send(
        &app,
        call(
            Method::GET,
            uri_read,
            Some(&jar),
            Some(&format!("whl_{}", "a".repeat(52))),
            None,
        ),
    )
    .await;

    assert_eq!(session_alone.status, StatusCode::OK);
    assert_eq!(acts_as_token.json()["kind"], "token");
    assert_eq!(acts_as_token.json()["email"], "other@example.test");
    assert_eq!(no_session_power.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&no_session_power), "insufficient_scope");
    assert_eq!(session_route.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&session_route), "session_required");
    assert_eq!(session_route_alone.status, StatusCode::OK);
    assert_eq!(bad_bearer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&bad_bearer), "invalid_token");
    assert_ne!(session_owner, other);
}

#[tokio::test]
async fn unknown_wrong_revoked_expired_and_malformed_tokens_are_indistinguishable() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (_, user) = session_user(&harness).await;
    let live = issue(&harness, user, &[TokenScope::Read]).await;
    let revoked = issue(&harness, user, &[TokenScope::Read]).await;
    let expired = issue(&harness, user, &[TokenScope::Read]).await;
    harness
        .store
        .revoke_api_token(user, revoked.id)
        .await
        .expect("revokes");
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(expired.id)
        .execute(harness.db.pool())
        .await
        .expect("the update runs");
    let live_text = live.secret.unpack().to_owned();
    let swapped = if live_text.ends_with('a') { 'b' } else { 'a' };
    let wrong_hash = format!("{}{swapped}", &live_text[..live_text.len() - 1]);
    let unknown = ApiTokenSecret::generate().expect("entropy is available");

    let candidates = [
        wrong_hash,
        unknown.unpack().to_owned(),
        revoked.secret.unpack().to_owned(),
        expired.secret.unpack().to_owned(),
        "junk".to_owned(),
        String::new(),
    ];
    let mut replies = Vec::new();
    for token in &candidates {
        replies.push(
            send(
                &app,
                call(Method::GET, "/api/v1/read", None, Some(token), None),
            )
            .await,
        );
    }
    let accepted = send(
        &app,
        call(Method::GET, "/api/v1/read", None, Some(&live_text), None),
    )
    .await;

    assert_eq!(accepted.status, StatusCode::OK);
    for reply in &replies {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.body, replies[0].body);
        assert_eq!(reply.headers, replies[0].headers);
    }
    assert_eq!(code(&replies[0]), "invalid_token");
    assert!(replies[0].headers.contains_key(header::WWW_AUTHENTICATE));
}

#[tokio::test]
async fn every_row_sharing_a_lookup_prefix_is_compared_not_just_the_first() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (_, user) = session_user(&harness).await;
    let first = ApiTokenSecret::generate().expect("entropy is available");
    let donor = ApiTokenSecret::generate().expect("entropy is available");
    let prefix = first.lookup_prefix().to_owned();
    let second = ApiTokenSecret::parse(&format!("{prefix}{}", &donor.unpack()[prefix.len()..]))
        .expect("a token sharing the prefix");
    issue_with(&harness, user, &[TokenScope::Read], &first).await;
    issue_with(&harness, user, &[TokenScope::Read], &second).await;
    let forged = format!("{prefix}{}", "a".repeat(44));

    for token in [first.unpack(), second.unpack()] {
        let reply = send(
            &app,
            call(Method::GET, "/api/v1/read", None, Some(token), None),
        )
        .await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "a token behind its prefix twin"
        );
    }
    let reply = send(
        &app,
        call(Method::GET, "/api/v1/read", None, Some(&forged), None),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

async fn last_used(harness: &Harness, id: Uuid) -> Option<time::OffsetDateTime> {
    sqlx::query_scalar("SELECT last_used_at FROM api_tokens WHERE id = $1")
        .bind(id)
        .fetch_one(harness.db.pool())
        .await
        .expect("the token row exists")
}

#[tokio::test]
async fn last_used_is_written_at_most_once_a_minute() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness), &harness);
    let (_, user) = session_user(&harness).await;
    let token = issue(&harness, user, &[TokenScope::Read]).await;
    let request = || {
        call(
            Method::GET,
            "/api/v1/read",
            None,
            Some(token.secret.unpack()),
            None,
        )
    };

    assert_eq!(last_used(&harness, token.id).await, None);
    assert_eq!(send(&app, request()).await.status, StatusCode::OK);
    let first = last_used(&harness, token.id)
        .await
        .expect("a first use is recorded");
    assert_eq!(send(&app, request()).await.status, StatusCode::OK);
    assert_eq!(last_used(&harness, token.id).await, Some(first));

    sqlx::query("UPDATE api_tokens SET last_used_at = now() - interval '2 minutes' WHERE id = $1")
        .bind(token.id)
        .execute(harness.db.pool())
        .await
        .expect("the update runs");
    let aged = last_used(&harness, token.id).await.expect("the aged value");
    assert_eq!(send(&app, request()).await.status, StatusCode::OK);
    assert!(last_used(&harness, token.id).await.expect("a value") > aged);
}

#[tokio::test]
async fn each_token_has_its_own_rate_limit_and_sessions_are_not_counted_against_it() {
    let harness = common::harness().await;
    let limiters = Limiters::new(&LimiterSettings {
        capture_ip: "30/s:60".parse().expect("a rate"),
        capture_endpoint: "20/s:100".parse().expect("a rate"),
        api_ip: "1/m:2".parse().expect("a rate"),
        create_ip: "10/m:10".parse().expect("a rate"),
        max_keys: 1000,
    })
    .expect("the limiters build");
    let access = mixed(&harness).with_token_limits(limiters);
    let app = probes(&access, &harness);
    let (jar, user) = session_user(&harness).await;
    let busy = issue(&harness, user, &[TokenScope::Read]).await;
    let idle = issue(&harness, user, &[TokenScope::Read]).await;
    let hit = |token: &Issued| {
        call(
            Method::GET,
            "/api/v1/read",
            None,
            Some(token.secret.unpack()),
            None,
        )
    };

    assert_eq!(send(&app, hit(&busy)).await.status, StatusCode::OK);
    assert_eq!(send(&app, hit(&busy)).await.status, StatusCode::OK);
    let limited = send(&app, hit(&busy)).await;
    let other_token = send(&app, hit(&idle)).await;
    let mut session_replies = Vec::new();
    for _ in 0..4 {
        session_replies.push(
            send(
                &app,
                call(Method::GET, "/api/v1/read", Some(&jar), None, None),
            )
            .await,
        );
    }

    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(code(&limited), "rate_limited");
    assert!(limited.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(other_token.status, StatusCode::OK);
    assert!(
        session_replies
            .iter()
            .all(|reply| reply.status == StatusCode::OK)
    );
}

#[tokio::test]
async fn the_resolved_caller_and_the_secret_never_print_the_token() {
    let harness = common::harness().await;
    let access = mixed(&harness);
    let (_, user) = session_user(&harness).await;
    let token = issue(&harness, user, &[TokenScope::Read]).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token.secret.unpack()))
            .expect("a header value"),
    );

    let caller = access
        .caller(&headers, Some("203.0.113.9".parse().expect("an address")))
        .await
        .expect("the token resolves");
    let rendered = format!("{caller:?} {:?} {:?}", token.secret, token.secret.digest());

    assert!(matches!(caller, Caller::Token { .. }));
    assert!(caller.token().is_some_and(|grant| grant.id() == token.id));
    assert!(!rendered.contains(&token.secret.unpack()[4..]));
    assert!(!rendered.contains(token.secret.lookup_prefix()));
}
