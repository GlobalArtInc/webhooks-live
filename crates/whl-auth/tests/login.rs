mod common;

use axum::http::StatusCode;
use common::mock_issuer::{CLIENT_ID, CLIENT_SECRET};
use common::{
    Harness, Jar, LOGIN_COOKIE, Options, PUBLIC_URL, SESSION_COOKIE, assert_login_error, harness,
    harness_with,
};
use url::Url;

async fn failed_login(harness: &Harness, code: &str) {
    let (reply, jar) = harness.sign_in(None).await;
    assert_login_error(&reply, code);
    assert!(jar.get(SESSION_COOKIE).is_none());
    assert_eq!(harness.count("users").await, 0);
    assert_eq!(harness.count("sessions").await, 0);
}

#[tokio::test]
async fn happy_path_creates_the_user_a_session_and_redirects() {
    let harness = harness().await;
    let (reply, jar) = harness.sign_in(Some("/e/abcdefghjkmnpqrstvwxyz01")).await;

    assert_eq!(reply.status, StatusCode::FOUND);
    assert_eq!(reply.location(), "/e/abcdefghjkmnpqrstvwxyz01");
    assert!(jar.get(SESSION_COOKIE).is_some());
    assert!(
        jar.get(LOGIN_COOKIE).is_none(),
        "the login cookie is cleared"
    );
    assert_eq!(harness.count("users").await, 1);
    assert_eq!(harness.count("sessions").await, 1);

    let who = harness.get("/api/v1/whoami", &jar).await;
    assert_eq!(who.status, StatusCode::OK);
    assert_eq!(who.json()["email"], "user@example.test");
    assert_eq!(who.json()["name"], "Test User");

    let stored: (String, String, String) =
        sqlx::query_as("SELECT issuer, subject, email FROM users")
            .fetch_one(harness.db.pool())
            .await
            .expect("the user row exists");
    assert_eq!(
        stored.0,
        harness.issuer.issuer_url().as_str().trim_end_matches('/')
    );
    assert_eq!(stored.1, "subject-1");
    assert_eq!(stored.2, "user@example.test");
}

#[tokio::test]
async fn login_redirects_to_the_issuer_with_pkce_and_the_registered_callback() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    assert_eq!(begun.reply.status, StatusCode::FOUND);
    let location = Url::parse(begun.reply.location()).expect("the location parses");
    assert_eq!(location.path(), "/authorize");

    harness.authorize(begun.reply.location()).await;
    let request = harness.issuer.authorize_requests().remove(0);
    assert_eq!(request.client_id, CLIENT_ID);
    assert_eq!(request.redirect_uri, format!("{PUBLIC_URL}/auth/callback"));
    assert_eq!(request.code_challenge_method, "S256");
    assert!(!request.code_challenge.is_empty());
    assert_eq!(request.state.len(), 43);
    assert_eq!(request.nonce.len(), 43);
    assert_eq!(request.scope, "openid email profile");
}

#[tokio::test]
async fn token_request_carries_the_pkce_verifier_and_client_credentials() {
    let harness = harness().await;
    let (reply, _) = harness.sign_in(None).await;
    assert_eq!(reply.status, StatusCode::FOUND);

    let requests = harness.issuer.token_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].code_verifier.len() >= 43);
    assert_eq!(
        requests[0].redirect_uri,
        format!("{PUBLIC_URL}/auth/callback")
    );
    assert_eq!(
        requests[0].basic_credentials.as_deref(),
        Some(format!("{CLIENT_ID}:{CLIENT_SECRET}").as_str())
    );
}

#[tokio::test]
async fn the_login_record_is_stored_without_the_raw_state() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    harness.authorize(begun.reply.location()).await;
    let state = harness.issuer.authorize_requests().remove(0).state;

    let keys: Vec<String> = sqlx::query_scalar("SELECT state FROM login_states")
        .fetch_all(harness.db.pool())
        .await
        .expect("the login states are readable");
    assert_eq!(keys.len(), 1);
    assert_ne!(keys[0], state);
}

#[tokio::test]
async fn state_mismatch_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.tamper_state = true);
    failed_login(&harness, "invalid_state").await;
}

#[tokio::test]
async fn callback_without_the_login_cookie_is_rejected() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    let uri = harness.authorize(begun.reply.location()).await;
    let reply = harness.callback(&uri, &Jar::default()).await;
    assert_login_error(&reply, "invalid_state");
    assert_eq!(harness.count("users").await, 0);
}

#[tokio::test]
async fn login_cookie_of_another_browser_does_not_unlock_the_state() {
    let harness = harness().await;
    let victim = harness.begin(None).await;
    let uri = harness.authorize(victim.reply.location()).await;
    let attacker = harness.begin(None).await;
    let reply = harness.callback(&uri, &attacker.jar).await;
    assert_login_error(&reply, "invalid_state");
    assert_eq!(harness.count("users").await, 0);
}

#[tokio::test]
async fn callback_cannot_be_replayed() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    let uri = harness.authorize(begun.reply.location()).await;
    let first = harness.callback(&uri, &begun.jar).await;
    assert_eq!(first.location(), "/");
    assert!(first.set_cookie(SESSION_COOKIE).is_some());
    let second = harness.callback(&uri, &begun.jar).await;
    assert_login_error(&second, "invalid_state");
    assert_eq!(harness.count("sessions").await, 1);
}

#[tokio::test]
async fn expired_login_state_is_rejected() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    let uri = harness.authorize(begun.reply.location()).await;
    sqlx::query("UPDATE login_states SET expires_at = now() - interval '1 second'")
        .execute(harness.db.pool())
        .await
        .expect("the state can be aged");
    let reply = harness.callback(&uri, &begun.jar).await;
    assert_login_error(&reply, "invalid_state");
}

#[tokio::test]
async fn provider_error_is_reported_without_reflecting_it() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    harness.authorize(begun.reply.location()).await;
    let state = harness.issuer.authorize_requests().remove(0).state;
    let uri = format!("/auth/callback?error=%3Cscript%3E&state={state}");
    let reply = harness.callback(&uri, &begun.jar).await;
    assert_login_error(&reply, "provider_denied");
}

#[tokio::test]
async fn callback_without_a_code_is_rejected() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    harness.authorize(begun.reply.location()).await;
    let state = harness.issuer.authorize_requests().remove(0).state;
    let reply = harness
        .callback(&format!("/auth/callback?state={state}"), &begun.jar)
        .await;
    assert_login_error(&reply, "invalid_request");
}

#[tokio::test]
async fn nonce_mismatch_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.nonce_claim = Some("another-nonce".to_owned()));
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn wrong_issuer_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.issuer_claim = Some("https://evil.example".to_owned()));
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn wrong_audience_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.audience_claim = Some("another-client".to_owned()));
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn expired_id_token_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.expires_in_seconds = -60);
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn bad_signature_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.bad_signature = true);
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn missing_id_token_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.omit_id_token = true);
    failed_login(&harness, "invalid_token").await;
}

#[tokio::test]
async fn refused_code_exchange_is_reported() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.reject_token_request = true);
    failed_login(&harness, "exchange_failed").await;
}

#[tokio::test]
async fn explicitly_unverified_email_is_rejected() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.email_verified = Some(false));
    failed_login(&harness, "email_not_verified").await;
}

#[tokio::test]
async fn omitted_email_verified_claim_is_accepted() {
    let harness = harness().await;
    harness
        .issuer
        .behave(|behaviour| behaviour.email_verified = None);
    let (reply, jar) = harness.sign_in(None).await;
    assert_eq!(reply.location(), "/");
    assert!(jar.get(SESSION_COOKIE).is_some());
}

#[tokio::test]
async fn missing_email_is_rejected() {
    let harness = harness().await;
    harness.issuer.behave(|behaviour| behaviour.email = None);
    failed_login(&harness, "missing_email").await;
}

#[tokio::test]
async fn second_login_updates_the_same_user_row() {
    let harness = harness().await;
    harness.sign_in(None).await;
    harness.issuer.behave(|behaviour| {
        behaviour.email = Some("renamed@example.test".to_owned());
        behaviour.name = None;
    });
    let (_, jar) = harness.sign_in(None).await;

    assert_eq!(harness.count("users").await, 1);
    assert_eq!(harness.count("sessions").await, 2);
    let who = harness.get("/api/v1/whoami", &jar).await;
    assert_eq!(who.json()["email"], "renamed@example.test");
    assert!(who.json()["name"].is_null());
}

#[tokio::test]
async fn return_to_is_sanitised_through_the_whole_flow() {
    let harness = harness().await;
    let cases = [
        ("/e/abc?tab=headers", "/e/abc?tab=headers"),
        ("//evil.example", "/"),
        ("https://evil.example", "/"),
        ("/\\evil.example", "/"),
        ("/a\u{1}b", "/"),
        ("", "/"),
    ];
    for (given, expected) in cases {
        let (reply, _) = harness.sign_in(Some(given)).await;
        assert_eq!(reply.location(), expected, "return_to {given:?}");
    }
}

#[tokio::test]
async fn a_planted_session_cookie_is_never_adopted() {
    let harness = harness().await;
    let (_, first_jar) = harness.sign_in(None).await;
    let planted = first_jar.get(SESSION_COOKIE).expect("a session").to_owned();

    let mut jar = Jar::default();
    jar.set(SESSION_COOKIE, &planted);
    let (reply, jar) = harness.sign_in_with(None, jar).await;

    let fresh = reply
        .set_cookie(SESSION_COOKIE)
        .expect("a session cookie is issued");
    assert!(!fresh.starts_with(&format!("{SESSION_COOKIE}={planted};")));
    assert_ne!(jar.get(SESSION_COOKIE), Some(planted.as_str()));

    let old = {
        let mut old_jar = Jar::default();
        old_jar.set(SESSION_COOKIE, &planted);
        old_jar
    };
    let denied = harness.get("/api/v1/whoami", &old).await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    let allowed = harness.get("/api/v1/whoami", &jar).await;
    assert_eq!(allowed.status, StatusCode::OK);
}

#[tokio::test]
async fn session_and_login_cookies_have_exactly_the_specified_attributes() {
    let harness = harness().await;
    let begun = harness.begin(None).await;
    let login = begun
        .reply
        .set_cookie(LOGIN_COOKIE)
        .expect("a login cookie");
    let mut parts: Vec<&str> = login.split("; ").skip(1).collect();
    parts.sort_unstable();
    assert_eq!(
        parts,
        [
            "HttpOnly",
            "Max-Age=600",
            "Path=/",
            "SameSite=Lax",
            "Secure"
        ]
    );

    let uri = harness.authorize(begun.reply.location()).await;
    let reply = harness.callback(&uri, &begun.jar).await;
    let session = reply.set_cookie(SESSION_COOKIE).expect("a session cookie");
    let mut parts: Vec<&str> = session.split("; ").skip(1).collect();
    parts.sort_unstable();
    assert_eq!(parts, ["HttpOnly", "Path=/", "SameSite=Lax", "Secure"]);

    let cleared = reply
        .set_cookie(LOGIN_COOKIE)
        .expect("the login cookie is cleared");
    assert!(cleared.starts_with(&format!("{LOGIN_COOKIE}=; ")));
    assert!(cleared.contains("Max-Age=0"));
    assert_eq!(
        reply.headers.get("cache-control").expect("cache-control"),
        "no-store"
    );
    assert_eq!(
        reply
            .headers
            .get("referrer-policy")
            .expect("referrer-policy"),
        "no-referrer"
    );
}

#[tokio::test]
async fn http_public_url_issues_plain_cookies_without_secure() {
    let harness = harness_with(Options {
        public_url: "http://127.0.0.1:8080",
        ..Options::default()
    })
    .await;
    let (reply, _) = harness.sign_in(None).await;
    let session = reply
        .set_cookie("whl_session")
        .expect("a plain session cookie");
    let mut parts: Vec<&str> = session.split("; ").skip(1).collect();
    parts.sort_unstable();
    assert_eq!(parts, ["HttpOnly", "Path=/", "SameSite=Lax"]);
    assert!(reply.set_cookie(SESSION_COOKIE).is_none());
}

#[tokio::test]
async fn only_a_hash_of_the_session_token_is_stored() {
    let harness = harness().await;
    let (_, jar) = harness.sign_in(None).await;
    let token = jar.get(SESSION_COOKIE).expect("a session").to_owned();
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(harness.db.pool())
        .await
        .expect("the session row exists");
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());
}
