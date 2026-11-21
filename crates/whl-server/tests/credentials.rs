use serde_json::json;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{KeyId, Keyring};
use whl_server::{StaleCredentials, stale_capture_credentials};
use whl_store::Store;
use whl_testkit::TestDb;

const KEY_ONE: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const KEY_TWO: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

fn ring(spec: &str) -> Keyring {
    Keyring::parse(spec).expect("a valid key set")
}

fn key(id: u16) -> Option<KeyId> {
    Some(KeyId::new(id).expect("a valid key id"))
}

async fn endpoint_with(db: &TestDb, document: Option<serde_json::Value>, deleted: bool) {
    let id = EndpointId::generate().expect("entropy");
    sqlx::query(
        "INSERT INTO endpoints (id, capture_auth, deleted_at) \
         VALUES ($1, $2::text::jsonb, CASE WHEN $3 THEN now() END)",
    )
    .bind(id.unpack())
    .bind(document.map(|document| document.to_string()))
    .bind(deleted)
    .execute(db.pool())
    .await
    .expect("the endpoint inserts");
}

fn credential(key_id: i64) -> Option<serde_json::Value> {
    Some(json!({"kind": "basic", "key_id": key_id, "username": "alice", "mac": "AA=="}))
}

#[tokio::test]
async fn capture_credentials_outside_the_active_key_are_counted_and_flagged_by_availability() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    endpoint_with(&db, credential(1), false).await;
    endpoint_with(&db, credential(1), false).await;
    endpoint_with(&db, credential(2), false).await;
    endpoint_with(&db, credential(1), true).await;
    endpoint_with(&db, None, false).await;
    endpoint_with(&db, Some(json!({"kind": "basic"})), false).await;

    let both = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    let outcome = stale_capture_credentials(&store, Some(&both))
        .await
        .expect("the usage reads");
    assert_eq!(
        outcome,
        [
            StaleCredentials {
                key_id: key(1),
                endpoints: 2,
                key_available: true
            },
            StaleCredentials {
                key_id: None,
                endpoints: 1,
                key_available: false
            },
        ]
    );

    let dropped = ring(&format!("2:{KEY_TWO}"));
    let outcome = stale_capture_credentials(&store, Some(&dropped))
        .await
        .expect("the usage reads");
    assert_eq!(
        outcome[0],
        StaleCredentials {
            key_id: key(1),
            endpoints: 2,
            key_available: false
        }
    );

    let none = stale_capture_credentials(&store, None)
        .await
        .expect("the usage reads");
    assert_eq!(none.len(), 3);
    assert!(none.iter().all(|entry| !entry.key_available));
}
