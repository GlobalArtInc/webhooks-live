mod common;

use axum::http::{Method, StatusCode};
use common::matrix::error_code;
use common::{Actor, Reply, SsoHarness, UNKNOWN_ID, harness, mixed_harness, sso_harness};
use whl_core::creator_net::CreatorNet;
use whl_store::{GrantedRole, Uuid};

const HOME: &str = "203.0.113.7";
const HOME_NEIGHBOUR: &str = "203.0.113.200";
const ELSEWHERE: &str = "198.51.100.5";

async fn anonymous_endpoint_from(harness: &SsoHarness, peer: &str) -> String {
    let net = CreatorNet::from_ip(peer.parse().expect("a valid address"));
    harness.anonymous_endpoint(Some(&net)).await
}

async fn claim(
    harness: &SsoHarness,
    actor: Option<&Actor>,
    peer: Option<&str>,
    endpoint: &str,
) -> Reply {
    harness
        .send_from(
            actor,
            peer,
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/claim"),
            "",
        )
        .await
}

async fn stored(harness: &SsoHarness, endpoint: &str) -> (Option<Uuid>, Option<String>) {
    sqlx::query_as("SELECT owner_id, creator_net::text FROM endpoints WHERE id = $1")
        .bind(endpoint)
        .fetch_one(harness.db.pool())
        .await
        .expect("the endpoint row exists")
}

async fn age_creation(harness: &SsoHarness, endpoint: &str, hours: i32) {
    sqlx::query(
        "UPDATE endpoints SET created_at = now() - make_interval(hours => $2) WHERE id = $1",
    )
    .bind(endpoint)
    .bind(hours)
    .execute(harness.db.pool())
    .await
    .expect("the endpoint can be aged");
}

#[tokio::test]
async fn a_signed_in_user_on_the_creating_network_claims_an_ownerless_endpoint() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;

    let reply = claim(&harness, Some(&user), Some(HOME_NEIGHBOUR), &endpoint).await;

    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["id"], endpoint.as_str());
    assert_eq!(body["my_role"], "owner");
    assert_eq!(body["owner"]["email"], "owner@example.test");
    assert_eq!(stored(&harness, &endpoint).await, (Some(user.id), None));
}

#[tokio::test]
async fn once_claimed_the_endpoint_follows_the_sso_rules() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let other = harness.actor("oth", "other@example.test", true).await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;
    assert_eq!(
        claim(&harness, Some(&user), Some(HOME), &endpoint)
            .await
            .status,
        StatusCode::OK
    );
    let target = format!("/api/v1/endpoints/{endpoint}");

    let anonymous = harness.send_as(None, Method::GET, &target, "").await;
    let stranger = harness
        .send_as(Some(&other), Method::GET, &target, "")
        .await;
    let owner = harness.send_as(Some(&user), Method::GET, &target, "").await;

    assert_eq!(anonymous.status, StatusCode::GONE);
    assert_eq!(stranger.status, StatusCode::GONE);
    assert_eq!(owner.status, StatusCode::OK);
    assert_eq!(owner.json()["my_role"], "owner");
}

#[tokio::test]
async fn a_claim_from_another_network_is_refused_and_changes_nothing() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;
    let before = stored(&harness, &endpoint).await;

    let reply = claim(&harness, Some(&user), Some(ELSEWHERE), &endpoint).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&reply), "claim_refused");
    assert_eq!(stored(&harness, &endpoint).await, before);
}

#[tokio::test]
async fn a_claim_without_a_known_client_address_is_refused() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;
    let unrecorded = harness.anonymous_endpoint(None).await;

    let unknown_caller = claim(&harness, Some(&user), None, &endpoint).await;
    let unknown_creator = claim(&harness, Some(&user), Some(HOME), &unrecorded).await;

    for reply in [unknown_caller, unknown_creator] {
        assert_eq!(reply.status, StatusCode::FORBIDDEN);
        assert_eq!(error_code(&reply), "claim_refused");
    }
    assert_eq!(stored(&harness, &unrecorded).await, (None, None));
}

#[tokio::test]
async fn a_claim_after_the_window_is_refused_and_one_inside_it_succeeds() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let late = anonymous_endpoint_from(&harness, HOME).await;
    let timely = anonymous_endpoint_from(&harness, HOME).await;
    age_creation(&harness, &late, 25).await;
    age_creation(&harness, &timely, 23).await;

    let refused = claim(&harness, Some(&user), Some(HOME), &late).await;
    let accepted = claim(&harness, Some(&user), Some(HOME), &timely).await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&refused), "claim_refused");
    assert_eq!(stored(&harness, &late).await.0, None);
    assert_eq!(accepted.status, StatusCode::OK);
}

#[tokio::test]
async fn claiming_an_owned_endpoint_is_refused_without_leaking_it_to_strangers() {
    let harness = mixed_harness().await;
    let owner = harness.actor("own", "owner@example.test", true).await;
    let member = harness.actor("mem", "member@example.test", true).await;
    let stranger = harness.actor("str", "stranger@example.test", true).await;
    let endpoint = harness.endpoint_for(&owner).await;
    let id = whl_core::endpoint_id::EndpointId::parse(&endpoint).expect("a valid id");
    harness
        .store
        .set_member(&id, common::RETENTION, &member.email, GrantedRole::Viewer)
        .await
        .expect("the member is stored");

    for actor in [&owner, &member] {
        let reply = claim(&harness, Some(actor), Some(HOME), &endpoint).await;
        assert_eq!(reply.status, StatusCode::CONFLICT);
        assert_eq!(error_code(&reply), "already_owned");
    }
    let hidden = claim(&harness, Some(&stranger), Some(HOME), &endpoint).await;
    let unknown = claim(&harness, Some(&stranger), Some(HOME), UNKNOWN_ID).await;
    assert_eq!(hidden.status, StatusCode::GONE);
    assert_eq!(hidden.body, unknown.body);
    assert_eq!(stored(&harness, &endpoint).await.0, Some(owner.id));
}

#[tokio::test]
async fn an_anonymous_caller_cannot_claim() {
    let harness = mixed_harness().await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;

    let reply = claim(&harness, None, Some(HOME), &endpoint).await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&reply), "sign_in_required");
    assert_eq!(stored(&harness, &endpoint).await.0, None);
}

#[tokio::test]
async fn deleted_unknown_and_malformed_ids_cannot_be_claimed() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let deleted = anonymous_endpoint_from(&harness, HOME).await;
    let removed = harness
        .send_as(
            None,
            Method::DELETE,
            &format!("/api/v1/endpoints/{deleted}"),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    for id in [deleted.as_str(), UNKNOWN_ID] {
        let reply = claim(&harness, Some(&user), Some(HOME), id).await;
        assert_eq!(reply.status, StatusCode::GONE, "{id}");
    }
    let malformed = claim(&harness, Some(&user), Some(HOME), "not-an-id").await;
    assert_eq!(malformed.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn parallel_claims_of_one_endpoint_have_exactly_one_winner() {
    let harness = mixed_harness().await;
    let mut users = Vec::new();
    for tag in ["aaa", "bbb", "ccc", "ddd", "eee", "fff"] {
        users.push(
            harness
                .actor(tag, &format!("{tag}@example.test"), true)
                .await,
        );
    }
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;

    let replies = futures_util::future::join_all(
        users
            .iter()
            .map(|user| claim(&harness, Some(user), Some(HOME), &endpoint)),
    )
    .await;

    let winners: Vec<usize> = replies
        .iter()
        .enumerate()
        .filter(|(_, reply)| reply.status == StatusCode::OK)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        winners.len(),
        1,
        "{:?}",
        replies.iter().map(|r| r.status).collect::<Vec<_>>()
    );
    for (index, reply) in replies.iter().enumerate() {
        if index != winners[0] {
            assert_eq!(reply.status, StatusCode::GONE);
        }
    }
    assert_eq!(
        stored(&harness, &endpoint).await.0,
        Some(users[winners[0]].id)
    );
}

#[tokio::test]
async fn a_claim_removes_member_rows_that_were_left_on_an_ownerless_endpoint() {
    let harness = mixed_harness().await;
    let user = harness.actor("own", "owner@example.test", true).await;
    let planted = harness.actor("pla", "planted@example.test", true).await;
    let endpoint = anonymous_endpoint_from(&harness, HOME).await;
    sqlx::query(
        "INSERT INTO endpoint_members (endpoint_id, user_id, role) VALUES ($1, $2, 'editor')",
    )
    .bind(&endpoint)
    .bind(planted.id)
    .execute(harness.db.pool())
    .await
    .expect("the member row is planted");

    let reply = claim(&harness, Some(&user), Some(HOME), &endpoint).await;

    assert_eq!(reply.status, StatusCode::OK);
    let planted_target = format!("/api/v1/endpoints/{endpoint}");
    let after = harness
        .send_as(Some(&planted), Method::GET, &planted_target, "")
        .await;
    assert_eq!(after.status, StatusCode::GONE);
}

#[tokio::test]
async fn the_claim_route_is_unavailable_in_open_mode_and_needs_mixed_in_sso() {
    let open = harness().await;
    let in_open = common::send(
        &open.app,
        common::bodiless(
            Method::POST,
            &format!("/api/v1/endpoints/{UNKNOWN_ID}/claim"),
        ),
    )
    .await;
    assert_eq!(in_open.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&in_open), "not_available_in_mode");

    let sso = sso_harness().await;
    let user = sso.actor("own", "owner@example.test", true).await;
    let anonymous = claim(&sso, None, Some(HOME), UNKNOWN_ID).await;
    let signed_in = claim(&sso, Some(&user), Some(HOME), UNKNOWN_ID).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(signed_in.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&signed_in), "not_available_in_mode");
}
