mod common;

use axum::http::{Method, StatusCode};
use common::matrix::error_code;
use common::{Actor, Reply, SsoHarness, harness, mixed_harness, mixed_harness_behind, sso_harness};
use serde_json::{Value, json};
use whl_core::client_ip::TrustedProxies;
use whl_store::Uuid;

async fn create_from(
    harness: &SsoHarness,
    actor: Option<&Actor>,
    peer: Option<&str>,
    extra: &[(&str, &str)],
) -> Reply {
    harness
        .send_with(
            actor,
            peer,
            extra,
            Method::POST,
            "/api/v1/endpoints",
            r#"{"title":"made"}"#,
        )
        .await
}

async fn recorded(harness: &SsoHarness, reply: &Reply) -> (Option<Uuid>, Option<String>) {
    sqlx::query_as("SELECT owner_id, creator_net::text FROM endpoints WHERE id = $1")
        .bind(reply.json()["id"].as_str().expect("an id"))
        .fetch_one(harness.db.pool())
        .await
        .expect("the endpoint row exists")
}

fn trusted(nets: &[&str]) -> TrustedProxies {
    TrustedProxies::new(
        nets.iter()
            .map(|net| net.parse().expect("a CIDR"))
            .collect(),
    )
}

#[tokio::test]
async fn an_anonymous_endpoint_records_the_creator_network_prefix_and_not_the_address() {
    let harness = mixed_harness().await;

    let v4 = create_from(&harness, None, Some("203.0.113.77"), &[]).await;
    let v6 = create_from(&harness, None, Some("2001:db8:aa:bb11:1:2:3:4"), &[]).await;
    let mapped = create_from(&harness, None, Some("::ffff:203.0.113.9"), &[]).await;
    let unknown = create_from(&harness, None, None, &[]).await;

    for reply in [&v4, &v6, &mapped, &unknown] {
        assert_eq!(reply.status, StatusCode::CREATED);
        assert!(reply.json()["owner"].is_null());
        assert_eq!(reply.json()["my_role"], "owner");
    }
    assert_eq!(
        recorded(&harness, &v4).await,
        (None, Some("203.0.113.0/24".to_owned()))
    );
    assert_eq!(
        recorded(&harness, &v6).await,
        (None, Some("2001:db8:aa:bb00::/56".to_owned()))
    );
    assert_eq!(
        recorded(&harness, &mapped).await,
        (None, Some("203.0.113.0/24".to_owned()))
    );
    assert_eq!(recorded(&harness, &unknown).await, (None, None));
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM endpoints WHERE creator_net::text LIKE '%203.0.113.77%'",
    )
    .fetch_one(harness.db.pool())
    .await
    .expect("the count runs");
    assert_eq!(leaked, 0);
}

#[tokio::test]
async fn a_signed_in_creator_owns_the_endpoint_and_no_network_is_recorded() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;

    let reply = create_from(&harness, Some(&user), Some("203.0.113.77"), &[]).await;

    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.json()["my_role"], "owner");
    assert_eq!(reply.json()["owner"]["email"], "owner@example.test");
    assert_eq!(recorded(&harness, &reply).await, (Some(user.id), None));
}

#[tokio::test]
async fn the_creator_network_follows_the_forwarded_chain_only_behind_a_trusted_proxy() {
    let harness = mixed_harness_behind(trusted(&["10.0.0.0/8"])).await;
    let forwarded = [("x-forwarded-for", "198.51.100.40")];

    let behind_proxy = create_from(&harness, None, Some("10.1.2.3"), &forwarded).await;
    let spoofed = create_from(&harness, None, Some("203.0.113.9"), &forwarded).await;

    assert_eq!(
        recorded(&harness, &behind_proxy).await.1,
        Some("198.51.100.0/24".to_owned())
    );
    assert_eq!(
        recorded(&harness, &spoofed).await.1,
        Some("203.0.113.0/24".to_owned())
    );
}

#[tokio::test]
async fn open_mode_records_no_creator_network_and_sso_still_refuses_anonymous_creation() {
    let open = harness().await;
    let created = common::create(&open.app, "{}").await;
    let net: Option<String> =
        sqlx::query_scalar("SELECT creator_net::text FROM endpoints WHERE id = $1")
            .bind(created["id"].as_str().expect("an id"))
            .fetch_one(open.db.pool())
            .await
            .expect("the endpoint row exists");
    assert_eq!(net, None);

    let sso = sso_harness().await;
    let refused = create_from(&sso, None, Some("203.0.113.77"), &[]).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_listing_shows_a_signed_in_user_only_owned_and_shared_endpoints() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let other = harness.actor("oth", "other@example.test", true).await;
    let mine = create_from(&harness, Some(&user), None, &[]).await.json();
    let theirs = create_from(&harness, Some(&other), None, &[]).await.json();
    let anonymous = create_from(&harness, None, Some("203.0.113.77"), &[])
        .await
        .json();

    let reply = harness
        .send_as(Some(&user), Method::GET, "/api/v1/endpoints", "")
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    let items = reply.json()["items"].as_array().expect("items").clone();
    let ids: Vec<&str> = items
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert_eq!(ids, [mine["id"].as_str().expect("an id")]);
    assert_eq!(items[0]["my_role"], "owner");
    assert!(!ids.contains(&theirs["id"].as_str().expect("an id")));
    assert!(!ids.contains(&anonymous["id"].as_str().expect("an id")));
}

#[tokio::test]
async fn a_claimed_endpoint_joins_the_listing_and_an_anonymous_caller_cannot_list() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let created = create_from(&harness, None, Some("203.0.113.77"), &[])
        .await
        .json();
    let endpoint = created["id"].as_str().expect("an id");
    let claim = harness
        .send_from(
            Some(&user),
            Some("203.0.113.78"),
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/claim"),
            "",
        )
        .await;
    assert_eq!(claim.status, StatusCode::OK);

    let listed = harness
        .send_as(Some(&user), Method::GET, "/api/v1/endpoints", "")
        .await;
    let refused = harness
        .send_as(None, Method::GET, "/api/v1/endpoints", "")
        .await;

    assert_eq!(listed.json()["items"][0]["id"], endpoint);
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&refused), "sign_in_required");
}

#[tokio::test]
async fn the_instance_reports_the_mode_the_sign_in_availability_and_the_callers_user() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;

    let anonymous = harness
        .send_as(None, Method::GET, "/api/v1/instance", "")
        .await;
    let signed_in = harness
        .send_as(Some(&user), Method::GET, "/api/v1/instance", "")
        .await;

    assert_eq!(anonymous.status, StatusCode::OK);
    let body = anonymous.json();
    assert_eq!(body["auth_mode"], "mixed");
    assert_eq!(body["user"], Value::Null);
    assert_eq!(body["auth"]["provider_name"], "Test SSO");
    assert_eq!(body["auth"]["sign_in_available"], false);
    assert_eq!(
        signed_in.json()["user"],
        json!({
            "id": user.id.hyphenated().to_string(),
            "email": "owner@example.test",
            "name": "User own",
        })
    );
}

#[tokio::test]
async fn the_instance_of_an_open_deployment_reports_no_sign_in() {
    let open = harness().await;
    let reply = common::send(&open.app, common::bodiless(Method::GET, "/api/v1/instance")).await;
    assert_eq!(reply.json()["auth_mode"], "open");
    assert_eq!(reply.json()["auth"]["sign_in_available"], false);
}

#[tokio::test]
async fn person_only_routes_answer_sign_in_required_to_anonymous_callers_in_mixed_mode() {
    let harness = mixed_harness().await;
    let anonymous = harness.anonymous_endpoint(None).await;

    let members = harness
        .send_as(
            None,
            Method::GET,
            &format!("/api/v1/endpoints/{anonymous}/members"),
            "",
        )
        .await;
    let listing = harness
        .send_as(None, Method::GET, "/api/v1/endpoints", "")
        .await;

    for reply in [members, listing] {
        assert_eq!(reply.status, StatusCode::FORBIDDEN);
        assert_eq!(error_code(&reply), "sign_in_required");
    }
}
