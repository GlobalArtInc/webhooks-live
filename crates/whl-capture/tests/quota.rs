mod common;

use common::Harness;
use sqlx::types::Uuid;
use whl_core::endpoint_id::EndpointId;

async fn owned_endpoint(harness: &Harness, over_quota: bool, overflow: &str) -> EndpointId {
    let owner: Uuid = sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email, storage_over_quota) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, 'owner@example.test', $1) \
         RETURNING id",
    )
    .bind(over_quota)
    .fetch_one(harness.db.pool())
    .await
    .expect("the owner inserts");
    let id = harness.endpoint_with(overflow).await;
    sqlx::query("UPDATE endpoints SET owner_id = $2 WHERE id = $1")
        .bind(id.unpack())
        .bind(owner)
        .execute(harness.db.pool())
        .await
        .expect("the owner is assigned");
    id
}

#[tokio::test]
async fn an_over_quota_owner_answers_507_quota_exceeded_and_nothing_is_stored() {
    let harness = Harness::new().await;
    let id = owned_endpoint(&harness, true, "refuse").await;

    let reply = harness.post(&format!("/{id}"), "payload").await;

    assert_eq!(reply.status, 507);
    assert_eq!(
        reply.header("x-webhooks-live-error"),
        Some("quota_exceeded")
    );
    assert_eq!(reply.header("x-endpoint-id"), Some(id.unpack()));
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn an_owner_within_the_quota_is_captured_as_usual() {
    let harness = Harness::new().await;
    let id = owned_endpoint(&harness, false, "refuse").await;

    let reply = harness.post(&format!("/{id}"), "payload").await;

    assert_eq!(reply.status, 200);
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn a_ring_mode_endpoint_of_an_over_quota_owner_keeps_accepting() {
    let harness = Harness::new().await;
    let id = owned_endpoint(&harness, true, "ring").await;

    let reply = harness.post(&format!("/{id}"), "payload").await;

    assert_eq!(reply.status, 200);
    assert_eq!(harness.rows(&id).await.len(), 1);
}
