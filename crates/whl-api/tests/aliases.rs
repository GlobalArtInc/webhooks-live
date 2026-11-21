mod common;

use axum::http::{Method, StatusCode};
use common::{Actor, Reply, SsoHarness, field_codes, mixed_harness};
use serde_json::json;

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn put_alias(harness: &SsoHarness, actor: &Actor, endpoint: &str, alias: &str) -> Reply {
    harness
        .send_as(
            Some(actor),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/aliases/{alias}"),
            "",
        )
        .await
}

async fn delete_alias(harness: &SsoHarness, actor: &Actor, endpoint: &str, alias: &str) -> Reply {
    harness
        .send_as(
            Some(actor),
            Method::DELETE,
            &format!("/api/v1/endpoints/{endpoint}/aliases/{alias}"),
            "",
        )
        .await
}

async fn list_aliases(harness: &SsoHarness, actor: &Actor, endpoint: &str) -> Reply {
    harness
        .send_as(
            Some(actor),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/aliases"),
            "",
        )
        .await
}

async fn check_availability(harness: &SsoHarness, actor: Option<&Actor>, name: &str) -> Reply {
    harness
        .send_as(
            actor,
            Method::GET,
            &format!("/api/v1/aliases/{name}/availability"),
            "",
        )
        .await
}

#[tokio::test]
async fn the_owner_assigns_lists_and_releases_an_alias() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    let created = put_alias(&harness, &owner, &endpoint, "my-hook").await;
    assert_eq!(created.status, StatusCode::OK);
    let body = created.json();
    assert_eq!(body["alias"], "my-hook");
    assert_eq!(body["url"], "https://whl.test/a/my-hook");
    assert!(
        body["created_at"]
            .as_str()
            .is_some_and(|text| text.ends_with('Z'))
    );

    let again = put_alias(&harness, &owner, &endpoint, "my-hook").await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json()["created_at"], body["created_at"]);

    let listed = list_aliases(&harness, &owner, &endpoint).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["items"].as_array().map(Vec::len), Some(1));

    let removed = delete_alias(&harness, &owner, &endpoint, "my-hook").await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    assert_eq!(
        list_aliases(&harness, &owner, &endpoint).await.json()["items"],
        json!([])
    );
    let repeated = delete_alias(&harness, &owner, &endpoint, "my-hook").await;
    assert_eq!(repeated.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&repeated), "alias_not_found");
}

#[tokio::test]
async fn a_malformed_or_reserved_alias_is_refused_with_its_field_code() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    for (alias, expected) in [
        ("Bad_Name", "invalid_format"),
        ("ab", "invalid_format"),
        ("ends-with-", "invalid_format"),
        ("admin", "reserved"),
        ("a", "reserved"),
    ] {
        let reply = put_alias(&harness, &owner, &endpoint, alias).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{alias}");
        assert_eq!(code(&reply), "validation_failed", "{alias}");
        assert_eq!(
            field_codes(&reply.json()),
            [("alias".to_owned(), expected.to_owned())],
            "{alias}"
        );
    }
}

#[tokio::test]
async fn an_alias_in_use_or_in_quarantine_answers_conflict_for_another_user() {
    let harness = mixed_harness().await;
    let first = harness.actor("one", "one@example.test", true).await;
    let second = harness.actor("two", "two@example.test", true).await;
    let first_endpoint = harness.endpoint_for(&first).await;
    let second_endpoint = harness.endpoint_for(&second).await;

    assert_eq!(
        put_alias(&harness, &first, &first_endpoint, "contested")
            .await
            .status,
        StatusCode::OK
    );
    let in_use = put_alias(&harness, &second, &second_endpoint, "contested").await;
    assert_eq!(in_use.status, StatusCode::CONFLICT);
    assert_eq!(code(&in_use), "alias_taken");

    delete_alias(&harness, &first, &first_endpoint, "contested").await;
    let quarantined = put_alias(&harness, &second, &second_endpoint, "contested").await;
    assert_eq!(quarantined.status, StatusCode::CONFLICT);
    assert_eq!(code(&quarantined), "alias_taken");

    let back = put_alias(&harness, &first, &first_endpoint, "contested").await;
    assert_eq!(back.status, StatusCode::OK);
}

#[tokio::test]
async fn the_sixth_alias_of_a_user_is_refused_with_the_limit() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    for index in 0..5 {
        let reply = put_alias(&harness, &owner, &endpoint, &format!("alias-{index}")).await;
        assert_eq!(reply.status, StatusCode::OK, "{index}");
    }
    let sixth = put_alias(&harness, &owner, &endpoint, "alias-5").await;
    assert_eq!(sixth.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&sixth), "limit_reached");
    assert_eq!(
        sixth.json()["error"]["limit"],
        json!({"name": "aliases", "max": 5})
    );
}

#[tokio::test]
async fn a_member_lists_aliases_but_only_the_owner_changes_them() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let member = harness.actor("mem", "member@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    put_alias(&harness, &owner, &endpoint, "shared-hook").await;
    let added = harness
        .send_as(
            Some(&owner),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            &json!({"email": "member@example.test", "role": "editor"}).to_string(),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK);

    let listed = list_aliases(&harness, &member, &endpoint).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["items"][0]["alias"], "shared-hook");
    let put = put_alias(&harness, &member, &endpoint, "member-hook").await;
    assert_eq!(put.status, StatusCode::FORBIDDEN);
    let delete = delete_alias(&harness, &member, &endpoint, "shared-hook").await;
    assert_eq!(delete.status, StatusCode::FORBIDDEN);
    assert_eq!(
        list_aliases(&harness, &owner, &endpoint).await.json()["items"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[tokio::test]
async fn a_stranger_learns_nothing_about_the_aliases_of_an_owned_endpoint() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let stranger = harness.actor("str", "stranger@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    put_alias(&harness, &owner, &endpoint, "private-hook").await;

    for reply in [
        list_aliases(&harness, &stranger, &endpoint).await,
        put_alias(&harness, &stranger, &endpoint, "stolen-hook").await,
        delete_alias(&harness, &stranger, &endpoint, "private-hook").await,
    ] {
        assert_eq!(reply.status, StatusCode::GONE);
        assert_eq!(code(&reply), "endpoint_gone");
    }
}

#[tokio::test]
async fn anonymous_callers_and_endpoints_without_an_owner_cannot_take_aliases() {
    let harness = mixed_harness().await;
    let user = harness.actor("usr", "user@example.test", true).await;
    let ownerless = harness.anonymous_endpoint(None).await;
    let owned = harness.endpoint_for(&user).await;

    let anonymous = harness
        .send_as(
            None,
            Method::PUT,
            &format!("/api/v1/endpoints/{owned}/aliases/anon-hook"),
            "",
        )
        .await;
    assert_eq!(anonymous.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&anonymous), "sign_in_required");

    let unclaimed = put_alias(&harness, &user, &ownerless, "unclaimed-hook").await;
    assert_eq!(unclaimed.status, StatusCode::CONFLICT);
    assert_eq!(code(&unclaimed), "claim_required");
}

#[tokio::test]
async fn the_availability_check_names_free_taken_reserved_and_malformed_names() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let other = harness.actor("oth", "other@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    put_alias(&harness, &owner, &endpoint, "taken-name").await;

    for (name, available, reason) in [
        ("free-name", true, json!(null)),
        ("taken-name", false, json!("taken")),
        ("admin", false, json!("reserved")),
        ("Bad_Name", false, json!("invalid")),
    ] {
        let reply = check_availability(&harness, Some(&other), name).await;
        assert_eq!(reply.status, StatusCode::OK, "{name}");
        assert_eq!(
            reply.json(),
            json!({"alias": name, "available": available, "reason": reason}),
            "{name}"
        );
    }
    let anonymous = check_availability(&harness, None, "free-name").await;
    assert_eq!(anonymous.status, StatusCode::FORBIDDEN);
    assert_eq!(code(&anonymous), "sign_in_required");
}
