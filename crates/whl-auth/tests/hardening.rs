mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{Method, StatusCode, header};
use common::{
    Harness, Jar, LOGIN_COOKIE_PREFIX, Options, PUBLIC_URL, SESSION_COOKIE, assert_login_error,
    harness, harness_with, request, send,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use whl_store::VerifiedIdentity;

const FOREIGN: &str = "https://evil.example";

fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_secs(),
    )
    .expect("the time fits in i64")
}

async fn whoami_with(harness: &Harness, cookies: &[&str]) -> StatusCode {
    let headers: Vec<(header::HeaderName, &str)> = cookies
        .iter()
        .map(|cookie| (header::COOKIE, *cookie))
        .collect();
    send(
        &harness.app,
        request(Method::GET, "/api/v1/whoami", &Jar::default(), &headers),
    )
    .await
    .status
}

async fn signed_in_token(harness: &Harness) -> String {
    let (_, jar) = harness.sign_in(None).await;
    jar.get(SESSION_COOKIE).expect("a session").to_owned()
}

#[tokio::test]
async fn smuggled_session_cookie_names_never_win_over_or_replace_the_real_one() {
    let harness = harness().await;
    let real = signed_in_token(&harness).await;
    let other = "u".repeat(43);
    let name = SESSION_COOKIE;

    let accepted = [
        format!("{name}={real}"),
        format!("{name}={real}; %5F%5FHost-whl_session={other}"),
        format!("%5F%5FHost-whl_session={other}; {name}={real}"),
        format!("__Host-whl%5Fsession={other}; {name}={real}"),
        format!("__host-whl_session={other}; {name}={real}"),
        format!("{name}={real};__Host-whl_session_x={other}"),
    ];
    for cookie in &accepted {
        assert_eq!(
            whoami_with(&harness, &[cookie]).await,
            StatusCode::OK,
            "{cookie}"
        );
    }

    let refused = [
        format!("{name}={real}; {name}={real}"),
        format!("{name}={real}; {name}={other}"),
        format!("{name}={other}; {name}={real}"),
        format!("{name}={real};{name}={real}"),
        format!("%5F%5FHost-whl_session={real}"),
        format!("__Host-whl%5Fsession={real}"),
        format!("__%48ost-whl_session={real}"),
        format!("__host-whl_session={real}"),
        format!("__HOST-WHL_SESSION={real}"),
        format!("{name} ={real}"),
        format!("{name}\t={real}"),
        format!("x{name}={real}"),
        format!("{name}x={real}"),
        format!("{name}=\"{real}\""),
        format!("{name}={real}%41"),
    ];
    for cookie in &refused {
        assert_eq!(
            whoami_with(&harness, &[cookie]).await,
            StatusCode::UNAUTHORIZED,
            "{cookie}"
        );
    }
    let first = format!("{name}={real}");
    assert_eq!(
        whoami_with(&harness, &[&first, &first]).await,
        StatusCode::UNAUTHORIZED,
        "the same cookie in two Cookie headers"
    );
}

#[tokio::test]
async fn a_duplicated_or_encoded_login_cookie_rejects_the_callback_and_keeps_the_state() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    let uri = harness.authorize(begun.reply.location()).await;
    let name = begun.jar.names_starting(LOGIN_COOKIE_PREFIX).remove(0);
    let value = begun.jar.get(&name).expect("a binding").to_owned();
    let encoded = name.replace("whl_login", "whl%5Flogin");

    for cookie in [
        format!("{name}={value}; {name}={value}"),
        format!("{encoded}={value}"),
        format!("{}={value}", name.to_lowercase()),
    ] {
        let reply = send(
            &harness.app,
            request(
                Method::GET,
                &uri,
                &Jar::default(),
                &[(header::COOKIE, &cookie)],
            ),
        )
        .await;
        assert_login_error(&reply, "invalid_state");
    }
    assert_eq!(harness.count("login_states").await, 1);
    let accepted = harness.callback(&uri, &begun.jar).await;
    assert_eq!(accepted.status, StatusCode::FOUND);
    assert!(accepted.set_cookie(SESSION_COOKIE).is_some());
}

#[tokio::test]
async fn pending_logins_are_capped_and_the_cap_counts_only_unexpired_states() {
    let harness = harness_with(Options {
        max_pending_logins: Some(2),
        ..Options::default()
    })
    .await;
    assert_eq!(harness.begin(None).await.reply.status, StatusCode::FOUND);
    assert_eq!(harness.begin(None).await.reply.status, StatusCode::FOUND);

    let refused = harness.begin(None).await;
    assert_eq!(refused.reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refused.reply.json()["error"]["code"], "login_busy");
    assert!(refused.reply.headers.get(header::RETRY_AFTER).is_some());
    assert!(refused.reply.set_cookies().is_empty());
    assert_eq!(harness.count("login_states").await, 2);

    sqlx::query("UPDATE login_states SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the states can be aged");
    assert_eq!(harness.begin(None).await.reply.status, StatusCode::FOUND);
}

#[tokio::test]
async fn one_network_cannot_exhaust_the_pending_logins_of_everyone_else() {
    let harness = harness_with(Options {
        max_pending_per_network: Some(2),
        ..Options::default()
    })
    .await;
    for peer in ["203.0.113.5", "203.0.113.200"] {
        assert_eq!(harness.begin_from(peer).await.status, StatusCode::FOUND);
    }

    let refused = harness.begin_from("203.0.113.77").await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.json()["error"]["code"], "login_rate_limited");
    assert!(refused.headers.get(header::RETRY_AFTER).is_some());
    assert!(refused.set_cookies().is_empty());
    assert_eq!(harness.count("login_states").await, 2);

    for peer in ["203.0.114.5", "2001:db8:1:2::1"] {
        assert_eq!(harness.begin_from(peer).await.status, StatusCode::FOUND);
    }
    assert_eq!(harness.begin(None).await.reply.status, StatusCode::FOUND);

    sqlx::query("UPDATE login_states SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the states can be aged");
    assert_eq!(
        harness.begin_from("203.0.113.5").await.status,
        StatusCode::FOUND
    );
}

#[tokio::test]
async fn the_global_cap_still_answers_busy_before_the_network_cap() {
    let harness = harness_with(Options {
        max_pending_logins: Some(1),
        max_pending_per_network: Some(1),
        ..Options::default()
    })
    .await;
    assert_eq!(
        harness.begin_from("203.0.113.5").await.status,
        StatusCode::FOUND
    );
    let refused = harness.begin_from("203.0.113.5").await;
    assert_eq!(refused.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refused.json()["error"]["code"], "login_busy");
}

#[tokio::test]
async fn the_purge_removes_expired_login_states_and_sessions_and_keeps_live_ones() {
    let harness = harness().await;
    let (_, _jar) = harness.sign_in(None).await;
    harness.begin(None).await;
    harness.begin(None).await;
    sqlx::query(
        "UPDATE login_states SET expires_at = now() - interval '1 second' \
         WHERE state = (SELECT state FROM login_states LIMIT 1)",
    )
    .execute(harness.db.pool())
    .await
    .expect("one state can be aged");
    harness.sign_in(None).await;
    sqlx::query(
        "UPDATE sessions SET expires_at = now() - interval '1 second' \
         WHERE token_hash = (SELECT token_hash FROM sessions LIMIT 1)",
    )
    .execute(harness.db.pool())
    .await
    .expect("one session can be aged");

    let idle = std::time::Duration::from_secs(3600);
    assert_eq!(
        harness
            .store
            .delete_expired_login_states()
            .await
            .expect("purges"),
        1
    );
    assert_eq!(
        harness
            .store
            .delete_expired_sessions(idle)
            .await
            .expect("purges"),
        1
    );
    assert_eq!(harness.count("login_states").await, 1);
    assert_eq!(harness.count("sessions").await, 1);
}

#[tokio::test]
async fn an_unreachable_provider_answers_503_on_sign_in_but_keeps_sessions_and_logout_working() {
    let harness = harness_with(Options {
        establish: false,
        ..Options::default()
    })
    .await;
    assert!(!harness.auth.is_available());

    let login = harness.get("/auth/login", &Jar::default()).await;
    assert_eq!(login.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(login.json()["error"]["code"], "auth_unavailable");
    assert!(login.headers.get(header::RETRY_AFTER).is_some());
    assert!(login.set_cookies().is_empty());
    let callback = harness
        .get("/auth/callback?code=x&state=y", &Jar::default())
        .await;
    assert_eq!(callback.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(callback.json()["error"]["code"], "auth_unavailable");
    assert_eq!(harness.count("login_states").await, 0);

    let user = harness
        .store
        .upsert_user(&VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: "offline".to_owned(),
            email: "offline@example.test".to_owned(),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored");
    let token = "t".repeat(43);
    harness
        .store
        .create_session(
            Sha256::digest(token.as_bytes()).as_slice(),
            user.id,
            std::time::Duration::from_secs(3600),
        )
        .await
        .expect("the session is stored");
    let cookie = format!("{SESSION_COOKIE}={token}");
    assert_eq!(whoami_with(&harness, &[&cookie]).await, StatusCode::OK);

    let logout = send(
        &harness.app,
        request(
            Method::POST,
            "/auth/logout",
            &Jar::default(),
            &[(header::COOKIE, &cookie), (header::ORIGIN, PUBLIC_URL)],
        ),
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert_eq!(harness.count("sessions").await, 0);

    harness
        .auth
        .establish()
        .await
        .expect("the provider is now reachable");
    assert!(harness.auth.is_available());
    let login = harness.get("/auth/login", &Jar::default()).await;
    assert_eq!(login.status, StatusCode::FOUND);
}

#[tokio::test]
async fn every_method_that_is_not_known_safe_needs_the_public_origin() {
    let harness = harness().await;
    let (_, jar) = harness.sign_in(None).await;

    for name in [
        "POST", "post", "get", "PURGE", "PROPFIND", "DELETE", "X-ODD",
    ] {
        let method = Method::from_bytes(name.as_bytes()).expect("a valid method token");
        let refused = send(
            &harness.app,
            request(
                method.clone(),
                "/api/v1/change",
                &jar,
                &[(header::ORIGIN, FOREIGN)],
            ),
        )
        .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(refused.json()["error"]["code"], "csrf_rejected", "{name}");
        let accepted = send(
            &harness.app,
            request(
                method,
                "/api/v1/change",
                &jar,
                &[(header::ORIGIN, PUBLIC_URL)],
            ),
        )
        .await;
        assert_eq!(accepted.status, StatusCode::NO_CONTENT, "{name}");
    }
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        let reply = send(
            &harness.app,
            request(
                method.clone(),
                "/api/v1/change",
                &jar,
                &[(header::ORIGIN, FOREIGN)],
            ),
        )
        .await;
        assert_eq!(reply.status, StatusCode::NO_CONTENT, "{method}");
    }
}

#[tokio::test]
async fn a_request_with_two_origin_headers_is_refused() {
    let harness = harness().await;
    let (_, jar) = harness.sign_in(None).await;
    let doubled = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/change",
            &jar,
            &[(header::ORIGIN, PUBLIC_URL), (header::ORIGIN, PUBLIC_URL)],
        ),
    )
    .await;
    assert_eq!(doubled.status, StatusCode::FORBIDDEN);
    let mixed = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/change",
            &Jar::default(),
            &[(header::ORIGIN, PUBLIC_URL), (header::ORIGIN, FOREIGN)],
        ),
    )
    .await;
    assert_eq!(mixed.status, StatusCode::FORBIDDEN);
}

async fn failed_with(harness: &Harness, overrides: Vec<(&str, Value)>, code: &str) {
    harness.issuer.behave(|behaviour| {
        behaviour.claim_overrides = overrides
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
    });
    let (reply, jar) = harness.sign_in(None).await;
    assert_login_error(&reply, code);
    assert!(jar.get(SESSION_COOKIE).is_none());
    assert_eq!(harness.count("sessions").await, 0);
}

async fn succeeded_with(harness: &Harness, overrides: Vec<(&str, Value)>) {
    harness.issuer.behave(|behaviour| {
        behaviour.claim_overrides = overrides
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
    });
    let (reply, jar) = harness.sign_in(None).await;
    assert_eq!(reply.status, StatusCode::FOUND);
    assert_eq!(reply.location(), "/");
    assert!(jar.get(SESSION_COOKIE).is_some());
}

#[tokio::test]
async fn an_id_token_issued_in_the_future_beyond_the_leeway_is_rejected() {
    let harness = harness().await;
    failed_with(
        &harness,
        vec![("iat", json!(now() + 3600))],
        "invalid_token",
    )
    .await;
    failed_with(&harness, vec![("iat", json!(now() + 120))], "invalid_token").await;
    succeeded_with(&harness, vec![("iat", json!(now() + 20))]).await;
}

#[tokio::test]
async fn an_id_token_that_is_not_yet_valid_is_rejected_when_nbf_is_present() {
    let harness = harness().await;
    failed_with(
        &harness,
        vec![("nbf", json!(now() + 3600))],
        "invalid_token",
    )
    .await;
    failed_with(&harness, vec![("nbf", json!("soon"))], "invalid_token").await;
    succeeded_with(&harness, vec![("nbf", json!(now() + 20))]).await;
    succeeded_with(&harness, vec![("nbf", json!(now() - 3600))]).await;
}

#[tokio::test]
async fn fractional_numeric_dates_are_accepted_for_nbf_iat_and_exp() {
    let harness = harness().await;
    let at = now();
    failed_with(
        &harness,
        vec![("nbf", json!(at as f64 + 3600.5))],
        "invalid_token",
    )
    .await;
    succeeded_with(
        &harness,
        vec![
            ("nbf", json!(at as f64 - 3600.5)),
            ("iat", json!(at as f64 - 5.25)),
            ("exp", json!(at as f64 + 3600.75)),
        ],
    )
    .await;
}

#[tokio::test]
async fn the_authorized_party_must_be_this_client_and_foreign_audiences_are_refused() {
    let harness = harness().await;
    let client = common::mock_issuer::CLIENT_ID;
    let two = json!([client, "another-client"]);
    failed_with(&harness, vec![("aud", two.clone())], "invalid_token").await;
    failed_with(
        &harness,
        vec![("aud", two.clone()), ("azp", json!(client))],
        "invalid_token",
    )
    .await;
    failed_with(
        &harness,
        vec![("aud", two), ("azp", json!("another-client"))],
        "invalid_token",
    )
    .await;
    failed_with(
        &harness,
        vec![("azp", json!("another-client"))],
        "invalid_token",
    )
    .await;
    succeeded_with(&harness, vec![("azp", json!(client))]).await;
    succeeded_with(&harness, vec![("aud", json!([client]))]).await;
}

#[tokio::test]
async fn an_email_verified_claim_that_is_not_a_boolean_has_its_own_error_code() {
    let harness = harness().await;
    for claim in [json!("true"), json!("false"), json!(1), json!(["true"])] {
        failed_with(
            &harness,
            vec![("email_verified", claim.clone())],
            "email_verified_invalid",
        )
        .await;
    }
    failed_with(
        &harness,
        vec![("email_verified", json!(false))],
        "email_not_verified",
    )
    .await;
}

#[tokio::test]
async fn the_verified_flag_of_the_stored_user_follows_the_claim() {
    let harness = harness().await;
    let flag = || async {
        sqlx::query_scalar::<_, bool>("SELECT email_verified FROM users")
            .fetch_one(harness.db.pool())
            .await
            .expect("the user row exists")
    };
    succeeded_with(&harness, vec![("email_verified", json!(true))]).await;
    assert!(flag().await);
    succeeded_with(&harness, vec![("email_verified", Value::Null)]).await;
    assert!(
        !flag().await,
        "an absent claim is accepted but never counts as verified"
    );
    succeeded_with(&harness, vec![("email_verified", json!(true))]).await;
    assert!(flag().await);
}

#[tokio::test]
async fn several_logins_can_be_in_flight_in_one_browser() {
    let harness = harness().await;
    let first = harness.begin(Some("/a")).await;
    let second = harness.begin_with(Some("/b"), first.jar.clone()).await;
    let jar = second.jar;
    assert_eq!(jar.names_starting(LOGIN_COOKIE_PREFIX).len(), 2);

    let first_uri = harness.authorize(first.reply.location()).await;
    let second_uri = harness.authorize(second.reply.location()).await;
    let second_done = harness.callback(&second_uri, &jar).await;
    assert_eq!(second_done.location(), "/b");
    let mut jar = jar;
    jar.absorb(&second_done);
    assert_eq!(jar.names_starting(LOGIN_COOKIE_PREFIX).len(), 1);
    let first_done = harness.callback(&first_uri, &jar).await;
    assert_eq!(first_done.location(), "/a");
    assert!(first_done.set_cookie(SESSION_COOKIE).is_some());
}

#[tokio::test]
async fn a_stray_or_failed_callback_clears_only_its_own_cookie() {
    let harness = harness().await;
    let begun = harness.begin(Some("/keep")).await;
    let uri = harness.authorize(begun.reply.location()).await;
    let name = begun.jar.names_starting(LOGIN_COOKIE_PREFIX).remove(0);

    for stray in [
        "/auth/callback?state=x",
        "/auth/callback",
        "/auth/callback?error=denied",
    ] {
        let reply = harness.callback(stray, &begun.jar).await;
        assert_login_error(&reply, "invalid_state");
        assert!(
            reply
                .set_cookies()
                .iter()
                .all(|cookie| !cookie.starts_with(&name)),
            "{stray} must not touch {name}"
        );
    }
    let unknown_state = format!("/auth/callback?code=x&state={}", "a".repeat(43));
    let reply = harness.callback(&unknown_state, &begun.jar).await;
    assert_login_error(&reply, "invalid_state");
    assert!(
        reply
            .set_cookies()
            .iter()
            .all(|cookie| !cookie.starts_with(&name))
    );

    let done = harness.callback(&uri, &begun.jar).await;
    assert_eq!(done.location(), "/keep");
}

#[tokio::test]
async fn a_failed_login_clears_the_cookie_of_that_attempt() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.reject_token_request = true);
    let begun = harness.begin(None).await;
    let uri = harness.authorize(begun.reply.location()).await;
    let name = begun.jar.names_starting(LOGIN_COOKIE_PREFIX).remove(0);
    let reply = harness.callback(&uri, &begun.jar).await;
    assert_login_error(&reply, "exchange_failed");
    let cleared = reply
        .set_cookie(&name)
        .expect("the attempt's own cookie is cleared");
    assert!(cleared.contains("Max-Age=0"));
}

#[tokio::test]
async fn login_cookies_are_bounded_per_browser() {
    let harness = harness().await;
    let mut jar = Jar::default();
    for _ in 0..12 {
        jar = harness.begin_with(None, jar).await.jar;
    }
    let held = jar.names_starting(LOGIN_COOKIE_PREFIX).len();
    assert!((1..=8).contains(&held), "{held} login cookies held");
}
