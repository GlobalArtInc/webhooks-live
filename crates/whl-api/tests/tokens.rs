mod common;

use axum::http::{Method, StatusCode, header};
use common::matrix::{Cast, cast};
use common::tokens::{ALL_SCOPES, mint};
use common::{RETENTION, SsoHarness, UNKNOWN_ID, harness, mixed_harness, sso_harness};
use serde_json::Value;
use sha2::{Digest, Sha256};
use whl_core::endpoint_id::EndpointId;
use whl_store::GrantedRole;

fn code(reply: &common::Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn shared_endpoint(harness: &SsoHarness, cast: &Cast, role: GrantedRole) -> String {
    let endpoint = harness.endpoint_for(&cast.owner).await;
    harness
        .store
        .set_member(
            &EndpointId::parse(&endpoint).expect("a valid id"),
            RETENTION,
            &cast.editor.email,
            role,
        )
        .await
        .expect("the member is stored");
    endpoint
}

async fn patch_title(harness: &SsoHarness, token: &str, endpoint: &str) -> common::Reply {
    harness
        .send_bearer(
            token,
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}"),
            r#"{"title":"by token"}"#,
        )
        .await
}

async fn create_endpoint(harness: &SsoHarness, token: &str) -> common::Reply {
    harness
        .send_bearer(token, Method::POST, "/api/v1/endpoints", "{}")
        .await
}

#[tokio::test]
async fn creation_shows_the_token_once_and_listing_never_does() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let created = harness
        .send_as(
            Some(&cast.owner),
            Method::POST,
            "/api/v1/tokens",
            r#"{"name":"laptop cli","scopes":["read","send"],"expires_in_days":30}"#,
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.headers[header::CACHE_CONTROL], "no-store");
    let json = created.json();
    let token = json["token"].as_str().expect("the token is shown once");
    assert!(token.starts_with("whl_"));
    assert_eq!(token.len(), 56);
    assert_eq!(json["prefix"], token[..12]);
    assert_eq!(json["name"], "laptop cli");
    assert_eq!(json["scopes"], serde_json::json!(["read", "send"]));
    assert_eq!(json["endpoint_ids"], Value::Null);
    assert_eq!(json["last_used_at"], Value::Null);
    assert_eq!(json["revoked_at"], Value::Null);

    let listed = harness
        .send_as(Some(&cast.owner), Method::GET, "/api/v1/tokens", "")
        .await;
    let text = String::from_utf8(listed.body.clone()).expect("text");
    assert_eq!(listed.status, StatusCode::OK);
    assert!(!text.contains(&token[12..]));
    let items = listed.json()["items"].as_array().expect("items").clone();
    assert_eq!(items.len(), 1);
    assert!(items[0].get("token").is_none());
    assert_eq!(items[0]["prefix"], token[..12]);
    assert_eq!(items[0]["id"], json["id"]);

    let other = harness
        .send_as(Some(&cast.viewer), Method::GET, "/api/v1/tokens", "")
        .await;
    assert_eq!(other.json()["items"], serde_json::json!([]));
}

#[tokio::test]
async fn the_table_holds_only_the_sha256_of_the_token() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let minted = mint(&harness, &cast.owner, &["read"], None).await;

    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM api_tokens")
        .fetch_one(harness.db.pool())
        .await
        .expect("one token row");
    let row_text: String = sqlx::query_scalar("SELECT api_tokens::text FROM api_tokens")
        .fetch_one(harness.db.pool())
        .await
        .expect("one token row");

    assert_eq!(stored, Sha256::digest(minted.token.as_bytes()).to_vec());
    assert!(!row_text.contains(&minted.token[12..]));
    assert!(!row_text.contains(&minted.token));
}

#[tokio::test]
async fn a_token_works_inside_its_scopes_and_endpoint_limit_and_is_refused_outside() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let first = harness.endpoint_for(&cast.owner).await;
    let second = harness.endpoint_for(&cast.owner).await;
    let reader = mint(&harness, &cast.owner, &["read"], Some(&[first.as_str()])).await;
    let writer = mint(&harness, &cast.owner, &["endpoints:write"], None).await;
    let get = |token: &str, uri: String| {
        let token = token.to_owned();
        let harness = &harness;
        async move { harness.send_bearer(&token, Method::GET, &uri, "").await }
    };

    for tail in ["", "/captures", "/forward-rules"] {
        let reply = get(&reader.token, format!("/api/v1/endpoints/{first}{tail}")).await;
        assert_eq!(reply.status, StatusCode::OK, "{tail}");
        let outside = get(&reader.token, format!("/api/v1/endpoints/{second}{tail}")).await;
        assert_eq!(outside.status, StatusCode::FORBIDDEN, "{tail}");
        assert_eq!(code(&outside), "insufficient_scope", "{tail}");
    }
    let denied = patch_title(&harness, &reader.token, &first).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&denied), "insufficient_scope");
    assert_eq!(
        patch_title(&harness, &writer.token, &first).await.status,
        StatusCode::OK
    );
    let no_read = get(&writer.token, format!("/api/v1/endpoints/{first}")).await;
    assert_eq!(code(&no_read), "insufficient_scope");

    let listing = get(&reader.token, "/api/v1/endpoints".to_owned()).await;
    assert_eq!(listing.json()["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(listing.json()["items"][0]["id"], first);

    assert_eq!(
        code(&create_endpoint(&harness, &reader.token).await),
        "insufficient_scope"
    );
    assert_eq!(
        create_endpoint(&harness, &writer.token).await.status,
        StatusCode::CREATED
    );
    let limited_writer = mint(
        &harness,
        &cast.owner,
        &["endpoints:write"],
        Some(&[first.as_str()]),
    )
    .await;
    assert_eq!(
        code(&create_endpoint(&harness, &limited_writer.token).await),
        "insufficient_scope"
    );
    let unknown = get(
        &mint(&harness, &cast.owner, &["read"], None).await.token,
        format!("/api/v1/endpoints/{UNKNOWN_ID}"),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::GONE);
}

#[tokio::test]
async fn expired_revoked_unknown_and_wrong_tokens_all_answer_the_same_401() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let live = mint(&harness, &cast.owner, &["read"], None).await;
    let revoked = mint(&harness, &cast.owner, &["read"], None).await;
    let expired = mint(&harness, &cast.owner, &["read"], None).await;
    let revoke = harness
        .send_as(
            Some(&cast.owner),
            Method::DELETE,
            &format!("/api/v1/tokens/{}", revoked.id),
            "",
        )
        .await;
    assert_eq!(revoke.status, StatusCode::NO_CONTENT);
    sqlx::query(
        "UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE id = $1::uuid",
    )
    .bind(&expired.id)
    .execute(harness.db.pool())
    .await
    .expect("the update runs");
    let swapped = if live.token.ends_with('a') { 'b' } else { 'a' };
    let wrong = format!("{}{swapped}", &live.token[..live.token.len() - 1]);
    let unknown = format!("whl_{}", "q".repeat(52));

    let accepted = harness
        .send_bearer(&live.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    let mut replies = Vec::new();
    for token in [
        &revoked.token,
        &expired.token,
        &wrong,
        &unknown,
        &"junk".to_owned(),
    ] {
        replies.push(
            harness
                .send_bearer(token, Method::GET, "/api/v1/instance", "")
                .await,
        );
    }
    for reply in &replies {
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(code(reply), "invalid_token");
        assert_eq!(reply.body, replies[0].body);
    }
}

#[tokio::test]
async fn losing_membership_removes_a_tokens_access_at_once_and_regaining_it_restores_it() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let endpoint = shared_endpoint(&harness, &cast, GrantedRole::Editor).await;
    let minted = mint(&harness, &cast.editor, &["read", "endpoints:write"], None).await;
    let uri = format!("/api/v1/endpoints/{endpoint}");
    let status = |method: Method| {
        let uri = uri.clone();
        let token = minted.token.clone();
        let harness = &harness;
        async move {
            let reply = harness
                .send_bearer(&token, method, &uri, r#"{"title":"x"}"#)
                .await;
            (reply.status, code(&reply))
        }
    };

    assert_eq!(status(Method::GET).await.0, StatusCode::OK);
    assert_eq!(status(Method::PATCH).await.0, StatusCode::OK);
    sqlx::query("DELETE FROM endpoint_members WHERE endpoint_id = $1")
        .bind(&endpoint)
        .execute(harness.db.pool())
        .await
        .expect("the membership is deleted");
    assert_eq!(
        status(Method::GET).await,
        (StatusCode::GONE, "endpoint_gone".to_owned())
    );
    assert_eq!(status(Method::PATCH).await.0, StatusCode::GONE);

    harness
        .store
        .set_member(
            &EndpointId::parse(&endpoint).expect("a valid id"),
            RETENTION,
            &cast.editor.email,
            GrantedRole::Viewer,
        )
        .await
        .expect("the member is stored");
    assert_eq!(status(Method::GET).await.0, StatusCode::OK);
    assert_eq!(
        status(Method::PATCH).await,
        (StatusCode::FORBIDDEN, "forbidden".to_owned())
    );
}

#[tokio::test]
async fn a_token_cannot_manage_tokens_members_or_ownership() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let endpoint = harness.endpoint_for(&cast.owner).await;
    let minted = mint(&harness, &cast.owner, &ALL_SCOPES, None).await;
    let viewer = cast.viewer.id.hyphenated().to_string();
    let calls: [(Method, String, &str); 6] = [
        (Method::GET, "/api/v1/tokens".to_owned(), ""),
        (
            Method::POST,
            "/api/v1/tokens".to_owned(),
            r#"{"name":"child","scopes":["read"]}"#,
        ),
        (Method::DELETE, format!("/api/v1/tokens/{}", minted.id), ""),
        (
            Method::GET,
            format!("/api/v1/endpoints/{endpoint}/members"),
            "",
        ),
        (
            Method::PUT,
            format!("/api/v1/endpoints/{endpoint}/members"),
            r#"{"email":"viewer@example.test","role":"viewer"}"#,
        ),
        (
            Method::DELETE,
            format!("/api/v1/endpoints/{endpoint}/members/{viewer}"),
            "",
        ),
    ];

    for (method, uri, body) in calls {
        let reply = harness
            .send_bearer(&minted.token, method.clone(), &uri, body)
            .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(code(&reply), "session_required", "{method} {uri}");
    }
    let claim = harness
        .send_bearer(
            &minted.token,
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/claim"),
            "",
        )
        .await;
    assert_eq!(code(&claim), "session_required");

    let listed = harness
        .send_as(Some(&cast.owner), Method::GET, "/api/v1/tokens", "")
        .await;
    let items = listed.json()["items"].as_array().expect("items").clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["revoked_at"], Value::Null);
    let still = harness
        .send_bearer(&minted.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(still.status, StatusCode::OK);
}

#[tokio::test]
async fn creation_is_limited_per_hour_and_by_the_number_of_active_tokens() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    for _ in 0..10 {
        mint(&harness, &cast.owner, &["read"], None).await;
    }
    let body = r#"{"name":"one more","scopes":["read"]}"#;
    let fast = harness
        .send_as(Some(&cast.owner), Method::POST, "/api/v1/tokens", body)
        .await;
    assert_eq!(fast.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(code(&fast), "quota_exceeded");
    assert!(fast.headers.contains_key(header::RETRY_AFTER));
    let limit = &fast.json()["error"]["limit"];
    assert_eq!(limit["name"], "token_creation");
    assert_eq!(limit["window"], "hour");
    assert_eq!(limit["max"], 10);
    assert!(limit["resets_at"].is_string());

    let colleague = harness
        .send_as(Some(&cast.viewer), Method::POST, "/api/v1/tokens", body)
        .await;
    assert_eq!(colleague.status, StatusCode::CREATED);

    sqlx::query(
        "INSERT INTO api_tokens (user_id, name, prefix, token_hash, scopes, expires_at, created_at) \
         SELECT $1, 'bulk', 'whl_' || lpad(g::text, 8, '0'), sha256(g::text::bytea), \
                ARRAY['read'], now() + interval '1 day', now() - interval '2 hours' \
         FROM generate_series(1, 20) g",
    )
    .bind(cast.editor.id)
    .execute(harness.db.pool())
    .await
    .expect("the bulk insert runs");
    let full = harness
        .send_as(Some(&cast.editor), Method::POST, "/api/v1/tokens", body)
        .await;
    assert_eq!(full.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&full), "limit_reached");
    assert_eq!(full.json()["error"]["limit"]["name"], "tokens");
    assert_eq!(full.json()["error"]["limit"]["max"], 20);
}

#[tokio::test]
async fn a_token_can_be_revoked_only_by_its_owner_and_revoking_is_repeatable() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let minted = mint(&harness, &cast.owner, &["read"], None).await;
    let uri = format!("/api/v1/tokens/{}", minted.id);

    let stranger = harness
        .send_as(Some(&cast.viewer), Method::DELETE, &uri, "")
        .await;
    assert_eq!(
        (stranger.status, code(&stranger)),
        (StatusCode::NOT_FOUND, "token_not_found".to_owned())
    );
    let still_works = harness
        .send_bearer(&minted.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(still_works.status, StatusCode::OK);
    for _ in 0..2 {
        let reply = harness
            .send_as(Some(&cast.owner), Method::DELETE, &uri, "")
            .await;
        assert_eq!(reply.status, StatusCode::NO_CONTENT);
    }
    let gone = harness
        .send_bearer(&minted.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(gone.status, StatusCode::UNAUTHORIZED);
    let malformed = harness
        .send_as(
            Some(&cast.owner),
            Method::DELETE,
            "/api/v1/tokens/not-a-uuid",
            "",
        )
        .await;
    assert_eq!(code(&malformed), "token_not_found");
    let listed = harness
        .send_as(Some(&cast.owner), Method::GET, "/api/v1/tokens", "")
        .await;
    assert!(listed.json()["items"][0]["revoked_at"].is_string());
}

#[tokio::test]
async fn the_instance_and_the_listing_honour_tokens() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let own = harness.endpoint_for(&cast.owner).await;
    let shared = shared_endpoint(&harness, &cast, GrantedRole::Viewer).await;
    let reader = mint(&harness, &cast.editor, &["read"], None).await;
    let sender = mint(&harness, &cast.editor, &["send"], None).await;

    let instance = harness
        .send_bearer(&reader.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(instance.status, StatusCode::OK);
    assert_eq!(instance.json()["user"]["email"], cast.editor.email);
    let denied = harness
        .send_bearer(&sender.token, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(code(&denied), "insufficient_scope");

    let listing = harness
        .send_bearer(&reader.token, Method::GET, "/api/v1/endpoints", "")
        .await;
    let ids: Vec<String> = listing.json()["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["id"].as_str().expect("an id").to_owned())
        .collect();
    assert_eq!(ids, std::slice::from_ref(&shared));
    assert_eq!(listing.json()["items"][0]["my_role"], "viewer");
    assert!(!ids.contains(&own));
}

#[tokio::test]
async fn mixed_mode_lets_a_token_act_on_an_ownerless_endpoint_but_never_claim_it() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let ownerless = harness.anonymous_endpoint(None).await;
    let minted = mint(&harness, &cast.owner, &["endpoints:write"], None).await;

    let patched = harness
        .send_bearer(
            &minted.token,
            Method::PATCH,
            &format!("/api/v1/endpoints/{ownerless}"),
            r#"{"title":"anyone with the id"}"#,
        )
        .await;
    let claim = harness
        .send_bearer(
            &minted.token,
            Method::POST,
            &format!("/api/v1/endpoints/{ownerless}/claim"),
            "",
        )
        .await;
    let created = harness
        .send_bearer(&minted.token, Method::POST, "/api/v1/endpoints", "{}")
        .await;

    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(code(&claim), "session_required");
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.json()["owner"]["email"], cast.owner.email);
    assert_eq!(created.json()["my_role"], "owner");
}

#[tokio::test]
async fn open_mode_ignores_a_bearer_header_and_has_no_token_routes() {
    let harness = harness().await;
    let endpoint = common::create(&harness.app, "{}").await;
    let id = endpoint["id"].as_str().expect("an id");
    let token = format!("whl_{}", "a".repeat(52));

    let with_header = common::send(
        &harness.app,
        axum::http::Request::builder()
            .uri(format!("/api/v1/endpoints/{id}"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(axum::body::Body::empty())
            .expect("the request is valid"),
    )
    .await;
    let routes = common::send(
        &harness.app,
        common::bodiless(Method::GET, "/api/v1/tokens"),
    )
    .await;

    assert_eq!(with_header.status, StatusCode::OK);
    assert_eq!(routes.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&routes), "not_available_in_mode");
}
