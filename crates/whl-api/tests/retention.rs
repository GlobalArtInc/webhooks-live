mod common;

use axum::http::{Method, StatusCode};
use common::{field_codes, harness, mixed_harness, request, send};
use whl_store::GrantedRole;

fn field(path: &str, code: &str) -> (String, String) {
    (path.to_owned(), code.to_owned())
}

#[tokio::test]
async fn an_anonymous_endpoint_takes_its_own_retention_cap_and_capture_ttl() {
    let harness = harness().await;
    let created = common::create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"sliding","seconds":7200},"max_captures":3,"capture_ttl_seconds":120}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(
        body["retention"],
        serde_json::json!({"mode": "sliding", "seconds": 7200})
    );
    assert_eq!(body["max_captures"], 3);
    assert_eq!(body["capture_ttl_seconds"], 120);
    assert!(body["expires_at"].is_string());

    let reset = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"sliding","seconds":null},"max_captures":null,"capture_ttl_seconds":null}"#,
        ),
    )
    .await
    .json();
    assert_eq!(
        reset["retention"],
        serde_json::json!({"mode": "sliding", "seconds": null})
    );
    assert_eq!(reset["max_captures"], 500);
    assert!(reset["capture_ttl_seconds"].is_null());
}

#[tokio::test]
async fn values_outside_the_ceilings_are_refused_with_one_violation_each() {
    let harness = harness().await;
    let created = common::create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"sliding","seconds":60},"max_captures":501,"capture_ttl_seconds":59}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let codes = field_codes(&reply.json());
    assert_eq!(
        codes,
        [
            field("retention.seconds", "out_of_range"),
            field("max_captures", "out_of_range"),
            field("capture_ttl_seconds", "out_of_range"),
        ]
    );

    let too_long = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"sliding","seconds":999999999}}"#,
        ),
    )
    .await;
    assert_eq!(
        field_codes(&too_long.json()),
        [field("retention.seconds", "out_of_range")]
    );
}

#[tokio::test]
async fn never_expire_is_not_allowed_in_open_mode() {
    let harness = harness().await;
    let created = common::create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"never","seconds":null}}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        [field("retention.mode", "not_allowed")]
    );
}

#[tokio::test]
async fn never_expire_asks_an_anonymous_caller_to_sign_in_where_signing_in_exists() {
    let harness = mixed_harness().await;
    let id = harness.anonymous_endpoint(None).await;
    let reply = harness
        .send_as(
            None,
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"retention":{"mode":"never","seconds":null}}"#,
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.json()["error"]["code"], "sign_in_required");
}

#[tokio::test]
async fn an_owner_gets_never_expire_up_to_the_limit_and_an_editor_cannot_change_retention() {
    let harness = mixed_harness().await;
    let owner = harness.actor("owner", "owner@example.test", true).await;
    let editor = harness.actor("editor", "editor@example.test", true).await;
    let mut ids = Vec::new();
    for _ in 0..11 {
        ids.push(harness.endpoint_for(&owner).await);
    }
    for id in &ids[..10] {
        let reply = harness
            .send_as(
                Some(&owner),
                Method::PATCH,
                &format!("/api/v1/endpoints/{id}"),
                r#"{"retention":{"mode":"never","seconds":null}}"#,
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK);
        let body = reply.json();
        assert_eq!(
            body["retention"],
            serde_json::json!({"mode": "never", "seconds": null})
        );
        assert!(body["expires_at"].is_null());
    }
    let eleventh = harness
        .send_as(
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/endpoints/{}", ids[10]),
            r#"{"retention":{"mode":"never","seconds":null}}"#,
        )
        .await;
    assert_eq!(eleventh.status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = eleventh.json();
    assert_eq!(error["error"]["code"], "limit_reached");
    assert_eq!(
        error["error"]["limit"],
        serde_json::json!({"name": "never_expire", "max": 10})
    );

    let shared = &ids[10];
    harness
        .store
        .set_member(
            &shared.parse().expect("the endpoint id parses"),
            common::RETENTION,
            &editor.email,
            GrantedRole::Editor,
        )
        .await
        .expect("the member is added");
    let refused = harness
        .send_as(
            Some(&editor),
            Method::PATCH,
            &format!("/api/v1/endpoints/{shared}"),
            r#"{"max_captures":5}"#,
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "forbidden");
    let allowed = harness
        .send_as(
            Some(&editor),
            Method::PATCH,
            &format!("/api/v1/endpoints/{shared}"),
            r#"{"title":"renamed"}"#,
        )
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
}
