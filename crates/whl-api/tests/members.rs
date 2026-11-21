mod common;

use axum::http::{Method, StatusCode};
use common::{Actor, Reply, SsoHarness, UNKNOWN_ID, sso_harness};
use serde_json::{Value, json};

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn put_member(
    harness: &SsoHarness,
    caller: &Actor,
    endpoint: &str,
    email: &str,
    role: &str,
) -> Reply {
    harness
        .send_as(
            Some(caller),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            &json!({ "email": email, "role": role }).to_string(),
        )
        .await
}

#[tokio::test]
async fn the_owner_adds_changes_lists_and_removes_a_member() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let guest = harness.actor("gue", "Guest@Example.Test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let members = format!("/api/v1/endpoints/{endpoint}/members");

    let added = put_member(&harness, &owner, &endpoint, "guest@example.test", "viewer").await;
    assert_eq!(added.status, StatusCode::OK);
    let body = added.json();
    assert_eq!(body["role"], "viewer");
    assert_eq!(
        body["user"],
        json!({
            "id": guest.id.hyphenated().to_string(),
            "email": "Guest@Example.Test",
            "name": "User gue",
        })
    );
    assert!(
        body["added_at"]
            .as_str()
            .is_some_and(|text| text.ends_with('Z'))
    );

    let changed = put_member(&harness, &owner, &endpoint, "GUEST@EXAMPLE.TEST", "editor").await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.json()["role"], "editor");

    let listed = harness
        .send_as(Some(&owner), Method::GET, &members, "")
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    let items = listed.json()["items"].as_array().expect("items").clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["role"], "editor");

    let removed = harness
        .send_as(
            Some(&owner),
            Method::DELETE,
            &format!("{members}/{}", guest.id.hyphenated()),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let after = harness
        .send_as(Some(&owner), Method::GET, &members, "")
        .await;
    assert_eq!(after.json()["items"], json!([]));
    let gone = harness
        .send_as(
            Some(&guest),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}"),
            "",
        )
        .await;
    assert_eq!(gone.status, StatusCode::GONE);
}

#[tokio::test]
async fn adding_a_member_matches_only_verified_emails_of_known_users() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    harness.actor("unv", "unverified@example.test", false).await;
    let endpoint = harness.endpoint_for(&owner).await;

    for email in ["unverified@example.test", "nobody@example.test"] {
        let reply = put_member(&harness, &owner, &endpoint, email, "viewer").await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{email}");
        assert_eq!(code(&reply), "user_not_found", "{email}");
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoint_members")
        .fetch_one(harness.db.pool())
        .await
        .expect("the count runs");
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn two_subjects_with_one_email_resolve_to_the_most_recent_verified_login() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let stale = harness.actor("old", "shared@example.test", true).await;
    let fresh = harness.actor("new", "SHARED@example.test", true).await;
    let unverified = harness.actor("unv", "shared@example.test", false).await;
    sqlx::query("UPDATE users SET last_login_at = now() - interval '1 day' WHERE id = $1")
        .bind(stale.id)
        .execute(harness.db.pool())
        .await
        .expect("the login time is aged");
    sqlx::query("UPDATE users SET last_login_at = now() + interval '1 day' WHERE id = $1")
        .bind(unverified.id)
        .execute(harness.db.pool())
        .await
        .expect("the login time is moved");
    let endpoint = harness.endpoint_for(&owner).await;

    let reply = put_member(&harness, &owner, &endpoint, "shared@example.test", "viewer").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.json()["user"]["id"],
        Value::String(fresh.id.hyphenated().to_string())
    );
    let again = put_member(&harness, &owner, &endpoint, "shared@example.test", "editor").await;
    assert_eq!(again.json()["user"]["id"], reply.json()["user"]["id"]);
}

#[tokio::test]
async fn the_owner_cannot_be_added_or_removed_as_a_member() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    let added = put_member(&harness, &owner, &endpoint, "OWNER@example.test", "editor").await;
    assert_eq!(added.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&added), "validation_failed");
    assert_eq!(added.json()["error"]["fields"][0]["field"], "email");

    let removed = harness
        .send_as(
            Some(&owner),
            Method::DELETE,
            &format!(
                "/api/v1/endpoints/{endpoint}/members/{}",
                owner.id.hyphenated()
            ),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&removed), "member_not_found");
    let still_owner = harness
        .send_as(
            Some(&owner),
            Method::DELETE,
            &format!("/api/v1/endpoints/{endpoint}"),
            "",
        )
        .await;
    assert_eq!(still_owner.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn removing_a_stranger_or_a_malformed_id_is_refused() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let stranger = harness.actor("str", "stranger@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let members = format!("/api/v1/endpoints/{endpoint}/members");

    for id in [
        stranger.id.hyphenated().to_string(),
        "00000000-0000-0000-0000-000000000000".to_owned(),
    ] {
        let reply = harness
            .send_as(Some(&owner), Method::DELETE, &format!("{members}/{id}"), "")
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{id}");
        assert_eq!(code(&reply), "member_not_found", "{id}");
    }
    for id in ["not-a-uuid", &stranger.id.simple().to_string()] {
        let reply = harness
            .send_as(Some(&owner), Method::DELETE, &format!("{members}/{id}"), "")
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{id}");
        assert_eq!(code(&reply), "not_found", "{id}");
    }
}

#[tokio::test]
async fn only_the_owner_manages_members_and_a_member_cannot_promote_themselves() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let editor = harness.actor("edi", "editor@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let added = put_member(&harness, &owner, &endpoint, "editor@example.test", "editor").await;
    assert_eq!(added.status, StatusCode::OK);

    let promote = put_member(
        &harness,
        &editor,
        &endpoint,
        "editor@example.test",
        "editor",
    )
    .await;
    assert_eq!(promote.status, StatusCode::FORBIDDEN);
    let leave = harness
        .send_as(
            Some(&editor),
            Method::DELETE,
            &format!(
                "/api/v1/endpoints/{endpoint}/members/{}",
                editor.id.hyphenated()
            ),
            "",
        )
        .await;
    assert_eq!(leave.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn member_bodies_are_validated_and_the_endpoint_must_be_live() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    harness.actor("gue", "guest@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let target = format!("/api/v1/endpoints/{endpoint}/members");

    let empty = harness
        .send_as(Some(&owner), Method::PUT, &target, "")
        .await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    for body in [
        r#"{"email":"guest@example.test","role":"owner"}"#,
        r#"{"email":"guest@example.test"}"#,
        r#"{"role":"viewer"}"#,
        r#"{"email":"guest@example.test","role":"viewer","extra":1}"#,
        r#"{"email":"not-an-email","role":"viewer"}"#,
    ] {
        let reply = harness
            .send_as(Some(&owner), Method::PUT, &target, body)
            .await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(code(&reply), "validation_failed", "{body}");
    }
    let unknown = put_member(&harness, &owner, UNKNOWN_ID, "guest@example.test", "viewer").await;
    assert_eq!(unknown.status, StatusCode::GONE);
}

#[tokio::test]
async fn the_listing_shows_owned_and_shared_endpoints_with_the_role_and_pages_by_activity() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let guest = harness.actor("gue", "guest@example.test", true).await;
    let first = harness.endpoint_for(&guest).await;
    let second = harness.endpoint_for(&guest).await;
    let shared = harness.endpoint_for(&owner).await;
    let hidden = harness.endpoint_for(&owner).await;
    let removed = harness.endpoint_for(&guest).await;
    for (id, age) in [(&first, 30), (&second, 20), (&shared, 10), (&removed, 5)] {
        sqlx::query(
            "UPDATE endpoints SET last_active_at = now() - make_interval(mins => $2) WHERE id = $1",
        )
        .bind(id)
        .bind(age)
        .execute(harness.db.pool())
        .await
        .expect("the activity time is set");
    }
    let shared_reply = put_member(&harness, &owner, &shared, "guest@example.test", "viewer").await;
    assert_eq!(shared_reply.status, StatusCode::OK);
    sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
        .bind(&removed)
        .execute(harness.db.pool())
        .await
        .expect("the endpoint is soft-deleted");

    let all = harness
        .send_as(Some(&guest), Method::GET, "/api/v1/endpoints", "")
        .await;
    assert_eq!(all.status, StatusCode::OK);
    let body = all.json();
    let seen: Vec<(String, String)> = body["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| {
            (
                item["id"].as_str().expect("id").to_owned(),
                item["my_role"].as_str().expect("role").to_owned(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (shared.clone(), "viewer".to_owned()),
            (second.clone(), "owner".to_owned()),
            (first.clone(), "owner".to_owned()),
        ]
    );
    assert!(!seen.iter().any(|(id, _)| *id == hidden || *id == removed));
    assert_eq!(body["next_cursor"], Value::Null);
    assert_eq!(body["items"][0]["owner"]["email"], "owner@example.test");

    let owned = harness
        .send_as(
            Some(&guest),
            Method::GET,
            "/api/v1/endpoints?role=owner",
            "",
        )
        .await;
    assert_eq!(owned.json()["items"].as_array().expect("items").len(), 2);
    let only_shared = harness
        .send_as(
            Some(&guest),
            Method::GET,
            "/api/v1/endpoints?role=shared",
            "",
        )
        .await;
    assert_eq!(only_shared.json()["items"][0]["id"], shared.as_str());
    assert_eq!(
        only_shared.json()["items"].as_array().expect("items").len(),
        1
    );

    let page_one = harness
        .send_as(Some(&guest), Method::GET, "/api/v1/endpoints?limit=2", "")
        .await;
    let page_one = page_one.json();
    assert_eq!(page_one["items"].as_array().expect("items").len(), 2);
    let cursor = page_one["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();
    let page_two = harness
        .send_as(
            Some(&guest),
            Method::GET,
            &format!("/api/v1/endpoints?limit=2&cursor={cursor}"),
            "",
        )
        .await;
    let page_two = page_two.json();
    assert_eq!(page_two["items"].as_array().expect("items").len(), 1);
    assert_eq!(page_two["items"][0]["id"], first.as_str());
    assert_eq!(page_two["next_cursor"], Value::Null);

    let mismatched = harness
        .send_as(
            Some(&guest),
            Method::GET,
            &format!("/api/v1/endpoints?limit=2&role=owner&cursor={cursor}"),
            "",
        )
        .await;
    assert_eq!(mismatched.status, StatusCode::BAD_REQUEST);
    assert_eq!(code(&mismatched), "invalid_cursor");
}

#[tokio::test]
async fn the_listing_needs_a_session_and_validates_its_query() {
    let harness = sso_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;

    let anonymous = harness
        .send_as(None, Method::GET, "/api/v1/endpoints", "")
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&anonymous), "unauthenticated");
    for (query, status, expected) in [
        (
            "limit=0",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "limit=201",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "limit=x",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        (
            "role=admin",
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
        ),
        ("cursor=%21%21", StatusCode::BAD_REQUEST, "invalid_cursor"),
    ] {
        let reply = harness
            .send_as(
                Some(&owner),
                Method::GET,
                &format!("/api/v1/endpoints?{query}"),
                "",
            )
            .await;
        assert_eq!(reply.status, status, "{query}");
        assert_eq!(code(&reply), expected, "{query}");
    }
}

#[tokio::test]
async fn open_mode_refuses_the_listing_and_the_members_routes_and_ignores_cookies() {
    let harness = common::harness().await;
    let created = common::create(&harness.app, "").await;
    let id = created["id"].as_str().expect("an id");
    assert_eq!(created["my_role"], Value::Null);
    assert_eq!(created["owner"], Value::Null);

    for (method, uri, body) in [
        (Method::GET, "/api/v1/endpoints".to_owned(), ""),
        (Method::GET, format!("/api/v1/endpoints/{id}/members"), ""),
        (
            Method::PUT,
            format!("/api/v1/endpoints/{id}/members"),
            r#"{"email":"a@b.test","role":"viewer"}"#,
        ),
        (
            Method::DELETE,
            format!("/api/v1/endpoints/{id}/members/00000000-0000-0000-0000-000000000000"),
            "",
        ),
    ] {
        let reply = common::send(&harness.app, common::request(method.clone(), &uri, body)).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(code(&reply), "not_available_in_mode", "{method} {uri}");
    }

    let instance = common::send(
        &harness.app,
        common::bodiless(Method::GET, "/api/v1/instance"),
    )
    .await;
    assert_eq!(instance.json()["auth_mode"], "open");
    assert_eq!(instance.json()["user"], Value::Null);
    assert_eq!(instance.json()["auth"]["provider_name"], Value::Null);
}
