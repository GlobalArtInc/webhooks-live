use std::time::Duration;

use whl_core::capture_auth::{AuthHeaderName, AuthUsername};
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_core::sender_allow::{SenderAllowList, SenderRange};
use whl_secrets::KeyId;
use whl_store::captures::{CapturePolicy, CaptureStoreError, EndpointState};
use whl_store::{
    AccessSettings, CaptureAuthRecord, EndpointChange, EndpointError, EndpointSettings, Lifecycle,
    LifecycleError, LifecycleLimits, NewEndpoint, Overflow, Store, StoredMac,
};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const POLICY: CapturePolicy = CapturePolicy {
    max_captures: 500,
    retention: RETENTION,
};

fn settings(cors: bool) -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
    }
}

fn allow(texts: &[&str]) -> SenderAllowList {
    SenderAllowList::new(
        texts
            .iter()
            .map(|text| SenderRange::parse(text).expect("the range parses"))
            .collect(),
    )
    .expect("the list fits")
}

fn basic(mac: u8) -> CaptureAuthRecord {
    CaptureAuthRecord::Basic {
        key_id: KeyId::new(3).expect("a valid key id"),
        username: AuthUsername::parse("alice").expect("a valid user name"),
        password_mac: StoredMac::new([mac; 32]),
    }
}

fn header(mac: u8) -> CaptureAuthRecord {
    CaptureAuthRecord::Header {
        key_id: KeyId::new(1).expect("a valid key id"),
        name: AuthHeaderName::parse("X-Hook-Token").expect("a valid header name"),
        value_mac: StoredMac::new([mac; 32]),
    }
}

async fn create(store: &Store, access: &AccessSettings, cors: bool) -> EndpointId {
    let id = EndpointId::generate().expect("entropy");
    let settings = settings(cors);
    store
        .create_endpoint_with_lifecycle(
            NewEndpoint {
                id: &id,
                owner: None,
                creator_net: None,
                settings: &settings,
                lifecycle: Lifecycle::default(),
                access,
                limits: LifecycleLimits::NONE,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    id
}

async fn replace_access(store: &Store, id: &EndpointId, access: AccessSettings) {
    store
        .update_endpoint::<LifecycleError, _>(
            id,
            RETENTION,
            LifecycleLimits::NONE,
            |current, _owner| {
                Ok(EndpointChange {
                    settings: current.settings.clone(),
                    lifecycle: current.lifecycle,
                    access,
                })
            },
        )
        .await
        .expect("the update runs")
        .expect("the endpoint is live");
}

#[tokio::test]
async fn access_settings_survive_create_update_read_and_the_capture_admission() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let created = AccessSettings {
        sender_allow: allow(&["203.0.113.0/24", "2001:db8::1"]),
        capture_auth: Some(basic(7)),
    };
    let id = create(&store, &created, true).await;

    let record = store
        .get_endpoint(&id, RETENTION)
        .await
        .expect("the read runs")
        .expect("the endpoint is live");
    assert_eq!(record.access, created);
    let admission = store
        .endpoint_admission(&id, POLICY)
        .await
        .expect("the admission reads");
    assert_eq!(admission.state, EndpointState::Live);
    assert_eq!(admission.access, created);
    assert!(admission.cors);

    let changed = AccessSettings {
        sender_allow: allow(&["198.51.100.9"]),
        capture_auth: Some(header(9)),
    };
    replace_access(&store, &id, changed.clone()).await;
    let admission = store
        .endpoint_admission(&id, POLICY)
        .await
        .expect("the admission reads");
    assert_eq!(admission.access, changed);

    replace_access(&store, &id, AccessSettings::default()).await;
    let admission = store
        .endpoint_admission(&id, POLICY)
        .await
        .expect("the admission reads");
    assert_eq!(admission.access, AccessSettings::default());
}

#[tokio::test]
async fn an_unknown_endpoint_is_gone_with_open_access() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let unknown = EndpointId::generate().expect("entropy");

    let admission = store
        .endpoint_admission(&unknown, POLICY)
        .await
        .expect("the admission reads");

    assert_eq!(admission.state, EndpointState::Gone);
    assert_eq!(admission.access, AccessSettings::default());
}

#[tokio::test]
async fn a_malformed_stored_auth_document_fails_the_admission_instead_of_opening_the_endpoint() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = create(&store, &AccessSettings::default(), false).await;
    sqlx::query("UPDATE endpoints SET capture_auth = '{\"kind\":\"basic\"}'::jsonb WHERE id = $1")
        .bind(id.unpack())
        .execute(db.pool())
        .await
        .expect("the document is written");

    let refused = store.endpoint_admission(&id, POLICY).await;
    assert!(matches!(refused, Err(CaptureStoreError::Corrupt(_))));
    let read = store.get_endpoint(&id, RETENTION).await;
    assert!(matches!(read, Err(EndpointError::Invalid { .. })));
}

#[tokio::test]
async fn the_database_refuses_more_than_twenty_sender_ranges() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = create(&store, &AccessSettings::default(), false).await;

    let refused = sqlx::query(
        "UPDATE endpoints SET sender_allow_cidrs = \
         ARRAY(SELECT ('10.0.' || n || '.0/24')::cidr FROM generate_series(1, 21) AS n) \
         WHERE id = $1",
    )
    .bind(id.unpack())
    .execute(db.pool())
    .await;

    assert!(refused.is_err());
}
