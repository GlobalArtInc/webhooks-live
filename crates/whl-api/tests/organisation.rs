mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{Method, StatusCode};
use common::matrix::error_code;
use common::{Actor, Reply, SsoHarness, field_codes, mixed_harness};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::captures::{CapturePolicy, NewCapture};

const RETENTION: Duration = Duration::from_secs(3600);

async fn store_capture(harness: &SsoHarness, endpoint: &str, method: &str) {
    let endpoint_id = EndpointId::parse(endpoint).expect("a valid endpoint id");
    let capture = NewCapture {
        id: CaptureId::generate().expect("an id is generated"),
        endpoint_id,
        received_at: OffsetDateTime::now_utc(),
        method: method.to_owned(),
        url: "https://whl.test/x".to_owned(),
        path: "/x".to_owned(),
        query_string: String::new(),
        host: "whl.test".to_owned(),
        client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
        headers: Vec::new(),
        query: Vec::new(),
        form: None,
        content_type: Some("text/plain".to_owned()),
        body: Bytes::from_static(b"hello"),
        status_override: None,
    };
    let policy = CapturePolicy {
        max_captures: 500,
        retention: RETENTION,
    };
    harness
        .store
        .store_capture(capture, policy)
        .await
        .expect("the capture is stored");
}

async fn share(harness: &SsoHarness, owner: &Actor, endpoint: &str, guest: &Actor, role: &str) {
    let body = format!(r#"{{"email":"{}","role":"{role}"}}"#, guest.email);
    let reply = harness
        .send_as(
            Some(owner),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            &body,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
}

async fn create_folder(harness: &SsoHarness, user: &Actor, name: &str) -> Reply {
    harness
        .send_as(
            Some(user),
            Method::POST,
            "/api/v1/folders",
            &format!(r#"{{"name":"{name}"}}"#),
        )
        .await
}

#[tokio::test]
async fn tags_are_stored_on_the_endpoint_and_echoed() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    let reply = harness
        .send_as(
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}"),
            r#"{"tags":["prod","eu-1"]}"#,
        )
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["tags"], serde_json::json!(["prod", "eu-1"]));
}

#[tokio::test]
async fn an_invalid_tag_is_refused_with_its_field() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;

    let reply = harness
        .send_as(
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/endpoints/{endpoint}"),
            r#"{"tags":["Prod"]}"#,
        )
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        field_codes(&reply.json())
            .iter()
            .any(|(field, _)| field == "tags")
    );
}

#[tokio::test]
async fn the_endpoint_list_filters_by_tag() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let tagged = harness.endpoint_for(&owner).await;
    let _plain = harness.endpoint_for(&owner).await;
    harness
        .send_as(
            Some(&owner),
            Method::PATCH,
            &format!("/api/v1/endpoints/{tagged}"),
            r#"{"tags":["prod"]}"#,
        )
        .await;

    let reply = harness
        .send_as(Some(&owner), Method::GET, "/api/v1/endpoints?tag=prod", "")
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    let items = reply.json()["items"].clone();
    assert_eq!(items.as_array().map(Vec::len), Some(1));
    assert_eq!(items[0]["id"], tagged.as_str());
}

#[tokio::test]
async fn folder_names_are_unique_per_user() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;

    let first = create_folder(&harness, &user, "work").await;
    let second = create_folder(&harness, &user, "work").await;

    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&second), "name_taken");
}

#[tokio::test]
async fn a_folder_collects_endpoints_the_user_can_see() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let folder = create_folder(&harness, &owner, "work").await.json()["id"]
        .as_str()
        .expect("the folder has an id")
        .to_owned();

    let added = harness
        .send_as(
            Some(&owner),
            Method::PUT,
            &format!("/api/v1/folders/{folder}/endpoints/{endpoint}"),
            "",
        )
        .await;
    let listed = harness
        .send_as(Some(&owner), Method::GET, "/api/v1/folders", "")
        .await;
    let filtered = harness
        .send_as(
            Some(&owner),
            Method::GET,
            &format!("/api/v1/endpoints?folder={folder}"),
            "",
        )
        .await;
    let removed = harness
        .send_as(
            Some(&owner),
            Method::DELETE,
            &format!("/api/v1/folders/{folder}/endpoints/{endpoint}"),
            "",
        )
        .await;

    assert_eq!(added.status, StatusCode::NO_CONTENT);
    assert_eq!(
        listed.json()["items"][0]["endpoint_ids"],
        serde_json::json!([endpoint])
    );
    assert_eq!(filtered.json()["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_stranger_cannot_add_an_endpoint_to_a_folder() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let stranger = harness.actor("str", "stranger@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let folder = create_folder(&harness, &stranger, "mine").await.json()["id"]
        .as_str()
        .expect("the folder has an id")
        .to_owned();

    let reply = harness
        .send_as(
            Some(&stranger),
            Method::PUT,
            &format!("/api/v1/folders/{folder}/endpoints/{endpoint}"),
            "",
        )
        .await;

    assert_eq!(reply.status, StatusCode::GONE);
}

#[tokio::test]
async fn folders_need_a_signed_in_caller() {
    let harness = mixed_harness().await;

    let reply = harness
        .send_as(None, Method::GET, "/api/v1/folders", "")
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&reply), "sign_in_required");
}

#[tokio::test]
async fn a_viewer_cannot_annotate_a_capture_but_an_editor_can() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let guest = harness.actor("gue", "guest@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    store_capture(&harness, &endpoint, "POST").await;
    let capture = harness
        .store
        .list_captures(
            &EndpointId::parse(&endpoint).expect("a valid endpoint id"),
            &whl_store::CaptureQuery {
                filter: whl_store::CaptureFilter::default(),
                order: whl_store::Order::Desc,
                limit: 10,
                position: None,
            },
        )
        .await
        .expect("the captures are listed")
        .items[0]
        .id
        .clone();
    share(&harness, &owner, &endpoint, &guest, "viewer").await;
    let uri = format!("/api/v1/endpoints/{endpoint}/captures/{capture}");

    let refused = harness
        .send_as(Some(&guest), Method::PATCH, &uri, r#"{"note":"check"}"#)
        .await;
    share(&harness, &owner, &endpoint, &guest, "editor").await;
    let accepted = harness
        .send_as(
            Some(&guest),
            Method::PATCH,
            &uri,
            r#"{"note":"check","starred":true}"#,
        )
        .await;
    let starred = harness
        .send_as(
            Some(&guest),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/captures?starred=true"),
            "",
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(accepted.json()["has_note"], true);
    assert_eq!(accepted.json()["starred"], true);
    assert_eq!(starred.json()["items"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn stats_count_captures_by_method_and_status() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    store_capture(&harness, &endpoint, "POST").await;
    store_capture(&harness, &endpoint, "POST").await;
    store_capture(&harness, &endpoint, "GET").await;

    let reply = harness
        .send_as(
            Some(&owner),
            Method::GET,
            &format!("/api/v1/endpoints/{endpoint}/stats?bucket=day"),
            "",
        )
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    let point = reply.json()["series"][0].clone();
    assert_eq!(point["total"], 3);
    assert_eq!(point["by_method"], serde_json::json!({"GET": 1, "POST": 2}));
    assert_eq!(point["by_status"], serde_json::json!({"2xx": 3}));
}
