use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use sqlx::PgPool;
use sqlx::types::Uuid;
use time::OffsetDateTime;
use tokio::task::JoinSet;
use whl_core::capture_id::CaptureId;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::composer::{QuotaCharge, QuotaOutcome, QuotaWindow};
use whl_store::quota::{
    EXPORTS_METRIC, EndpointByteCaps, SetTierOutcome, UserSelector, user_subject,
};
use whl_store::{
    ClaimOutcome, EndpointChange, EndpointSettings, Lifecycle, LifecycleError, LifecycleLimits,
    NewEndpoint, Overflow, Store,
};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const MAX_CAPTURES: u32 = 500;

fn settings() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        templating: Default::default(),
        cors: false,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
        verifier: None,
    }
}

fn counted(endpoints_max: u32) -> LifecycleLimits {
    LifecycleLimits {
        endpoints_max: Some(endpoints_max),
        never_expire_max: 5,
        ..LifecycleLimits::NONE
    }
}

async fn insert_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email, email_verified) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, $1, true) RETURNING id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("the user inserts")
}

async fn create(
    store: &Store,
    owner: Option<Uuid>,
    network: Option<&CreatorNet>,
    lifecycle: Lifecycle,
    limits: LifecycleLimits,
) -> Result<EndpointId, LifecycleError> {
    let settings = settings();
    store
        .create_endpoint_with_lifecycle(
            NewEndpoint {
                owner,
                creator_net: network,
                settings: &settings,
                lifecycle,
                limits,
            },
            RETENTION,
        )
        .await
        .map(|record| record.id)
}

fn network(octet: u8) -> CreatorNet {
    CreatorNet::from_ip(IpAddr::V4(Ipv4Addr::new(198, 51, octet, 7)))
}

async fn set_bytes(pool: &PgPool, endpoint: &EndpointId, bytes: i64) {
    sqlx::query("UPDATE endpoints SET bytes_used = $2 WHERE id = $1")
        .bind(endpoint.unpack())
        .bind(bytes)
        .execute(pool)
        .await
        .expect("the usage is set");
}

async fn send(store: &Store, endpoint: &EndpointId) -> CaptureOutcome {
    store
        .store_capture(
            NewCapture {
                id: CaptureId::generate().expect("entropy"),
                endpoint_id: endpoint.clone(),
                received_at: OffsetDateTime::now_utc(),
                method: "POST".to_owned(),
                url: "http://h/x".to_owned(),
                path: "/x".to_owned(),
                query_string: String::new(),
                host: "h".to_owned(),
                client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                headers: Vec::new(),
                query: Vec::new(),
                form: None,
                content_type: None,
                body: Bytes::from_static(b"x"),
                status_override: None,
            },
            CapturePolicy {
                max_captures: MAX_CAPTURES,
                retention: RETENTION,
            },
        )
        .await
        .expect("the capture call runs")
}

fn charge(subject: String, limit: u32) -> QuotaCharge {
    QuotaCharge {
        subject,
        window: QuotaWindow::Hour,
        limit,
        name: "outbound",
    }
}

#[tokio::test]
async fn parallel_endpoint_creations_never_pass_the_owner_limit() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;

    let mut tasks = JoinSet::new();
    for _ in 0..12 {
        let store = store.clone();
        tasks.spawn(async move {
            create(&store, Some(owner), None, Lifecycle::default(), counted(3)).await
        });
    }
    let mut created = 0;
    let mut refused = 0;
    while let Some(outcome) = tasks.join_next().await {
        match outcome.expect("the task finishes") {
            Ok(_) => created += 1,
            Err(LifecycleError::EndpointLimit { max: 3 }) => refused += 1,
            Err(other) => panic!("unexpected outcome: {other:?}"),
        }
    }

    assert_eq!((created, refused), (3, 9));
}

#[tokio::test]
async fn anonymous_creations_are_limited_per_creator_network_and_unlimited_without_one() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let home = network(1);
    let office = network(2);

    for _ in 0..2 {
        create(&store, None, Some(&home), Lifecycle::default(), counted(2))
            .await
            .expect("within the limit");
    }
    let refused = create(&store, None, Some(&home), Lifecycle::default(), counted(2)).await;

    assert!(matches!(
        refused,
        Err(LifecycleError::EndpointLimit { max: 2 })
    ));
    create(
        &store,
        None,
        Some(&office),
        Lifecycle::default(),
        counted(2),
    )
    .await
    .expect("another network has its own count");
    create(&store, None, None, Lifecycle::default(), counted(2))
        .await
        .expect("an unknown network is not counted");
}

#[tokio::test]
async fn fifty_parallel_charges_never_pass_a_quota_of_ten() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let subject = user_subject(Uuid::now_v7());

    let mut tasks = JoinSet::new();
    for _ in 0..50 {
        let store = store.clone();
        let charges = [charge(subject.clone(), 10)];
        tasks.spawn(async move { store.consume_quota(EXPORTS_METRIC, &charges).await });
    }
    let mut allowed = 0;
    while let Some(outcome) = tasks.join_next().await {
        if outcome.expect("the task finishes").expect("counted") == QuotaOutcome::Allowed {
            allowed += 1;
        }
    }

    assert_eq!(allowed, 10);
}

#[test]
fn a_window_resets_at_its_next_boundary() {
    let moment = time::macros::datetime!(2026-11-21 10:30:15 UTC);

    assert_eq!(
        QuotaWindow::Hour.resets_at(moment),
        time::macros::datetime!(2026-11-21 11:00:00 UTC)
    );
    assert_eq!(
        QuotaWindow::Day.resets_at(moment),
        time::macros::datetime!(2026-11-22 00:00:00 UTC)
    );
    assert_eq!(QuotaWindow::Hour.as_str(), "hour");
}

#[tokio::test]
async fn owner_usage_counts_objects_and_the_current_window_counters() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    let stranger = insert_user(db.pool(), "stranger@example.test").await;
    let first = create(&store, Some(owner), None, Lifecycle::default(), counted(10))
        .await
        .expect("created");
    create(
        &store,
        Some(owner),
        None,
        Lifecycle {
            never_expires: true,
            ..Lifecycle::default()
        },
        counted(10),
    )
    .await
    .expect("created");
    create(
        &store,
        Some(stranger),
        None,
        Lifecycle::default(),
        counted(10),
    )
    .await
    .expect("created");
    set_bytes(db.pool(), &first, 700).await;
    for metric in ["outbound", "outbound", EXPORTS_METRIC] {
        store
            .consume_quota(
                metric,
                &[QuotaCharge {
                    subject: user_subject(owner),
                    window: QuotaWindow::Hour,
                    limit: 100,
                    name: "outbound",
                }],
            )
            .await
            .expect("counted");
    }

    let usage = store
        .owner_usage(owner, RETENTION)
        .await
        .expect("the usage reads");

    assert_eq!(usage.endpoints, 2);
    assert_eq!(usage.never_expire, 1);
    assert_eq!(usage.storage_bytes, 700);
    assert_eq!(usage.outbound_hour, 2);
    assert_eq!(usage.exports_hour, 1);
    assert_eq!(usage.notifications_hour, 0);
}

#[tokio::test]
async fn captures_to_an_over_quota_owner_are_refused_until_the_flag_clears() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    let endpoint = create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    sqlx::query("UPDATE users SET storage_over_quota = true WHERE id = $1")
        .bind(owner)
        .execute(db.pool())
        .await
        .expect("the flag is set");

    let admission = store
        .endpoint_admission(
            &endpoint,
            CapturePolicy {
                max_captures: MAX_CAPTURES,
                retention: RETENTION,
            },
        )
        .await
        .expect("the admission reads");
    let refused = send(&store, &endpoint).await;
    sqlx::query("UPDATE users SET storage_over_quota = false WHERE id = $1")
        .bind(owner)
        .execute(db.pool())
        .await
        .expect("the flag clears");
    let accepted = send(&store, &endpoint).await;

    assert!(admission.over_quota);
    assert_eq!(admission.owner_tier.as_deref(), Some("member"));
    assert_eq!(refused, CaptureOutcome::OverQuota);
    assert!(matches!(accepted, CaptureOutcome::Stored { .. }));
}

#[tokio::test]
async fn never_expire_is_refused_while_the_owner_is_over_the_storage_quota() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    let holder = create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    set_bytes(db.pool(), &holder, 3000).await;
    let never = Lifecycle {
        never_expires: true,
        ..Lifecycle::default()
    };

    let refused = create(
        &store,
        Some(owner),
        None,
        never,
        LifecycleLimits {
            never_expire_max: 5,
            storage_bytes_max: Some(2000),
            ..LifecycleLimits::NONE
        },
    )
    .await;
    let accepted = create(
        &store,
        Some(owner),
        None,
        never,
        LifecycleLimits {
            never_expire_max: 5,
            storage_bytes_max: Some(5000),
            ..LifecycleLimits::NONE
        },
    )
    .await;

    assert!(matches!(
        refused,
        Err(LifecycleError::StorageQuota { max: 2000 })
    ));
    assert!(accepted.is_ok());
}

#[tokio::test]
async fn a_claim_beyond_the_endpoint_limit_is_refused() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    let home = network(3);
    let anonymous = create(
        &store,
        None,
        Some(&home),
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");

    let outcome = store
        .claim_endpoint_limited(
            &anonymous,
            owner,
            Some(&home),
            RETENTION,
            Duration::from_secs(3600),
            Some(1),
        )
        .await
        .expect("the claim runs");

    assert_eq!(outcome, ClaimOutcome::EndpointLimit { max: 1 });
}

#[tokio::test]
async fn set_tier_updates_one_user_by_email_or_id_and_reports_the_previous_tier() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool(), "Person@Example.test").await;

    let by_email = store
        .set_user_tier(
            &UserSelector::Email("person@example.TEST".to_owned()),
            "pro",
        )
        .await
        .expect("the tier is set");
    let by_id = store
        .set_user_tier(&UserSelector::Id(user), "member")
        .await
        .expect("the tier is set");

    assert_eq!(
        by_email,
        SetTierOutcome::Changed {
            user,
            previous: "member".to_owned()
        }
    );
    assert_eq!(
        by_id,
        SetTierOutcome::Changed {
            user,
            previous: "pro".to_owned()
        }
    );
}

#[tokio::test]
async fn set_tier_refuses_an_ambiguous_email_and_reports_an_unknown_user() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    insert_user(db.pool(), "shared@example.test").await;
    insert_user(db.pool(), "shared@example.test").await;

    let ambiguous = store
        .set_user_tier(
            &UserSelector::Email("shared@example.test".to_owned()),
            "pro",
        )
        .await
        .expect("the lookup runs");
    let missing = store
        .set_user_tier(
            &UserSelector::Email("nobody@example.test".to_owned()),
            "pro",
        )
        .await
        .expect("the lookup runs");

    assert!(matches!(ambiguous, SetTierOutcome::Ambiguous { users } if users.len() == 2));
    assert_eq!(missing, SetTierOutcome::NotFound);
}

async fn set_ring(pool: &PgPool, endpoint: &EndpointId) {
    sqlx::query("UPDATE endpoints SET overflow = 'ring' WHERE id = $1")
        .bind(endpoint.unpack())
        .execute(pool)
        .await
        .expect("the overflow mode is set");
}

async fn counters(pool: &PgPool, endpoint: &EndpointId) -> (i32, i64) {
    sqlx::query_as("SELECT capture_count, bytes_used FROM endpoints WHERE id = $1")
        .bind(endpoint.unpack())
        .fetch_one(pool)
        .await
        .expect("the counters read")
}

#[tokio::test]
async fn a_refuse_mode_endpoint_at_its_byte_cap_is_full() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let endpoint = create(
        &store,
        None,
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    assert!(matches!(
        send(&store, &endpoint).await,
        CaptureOutcome::Stored { .. }
    ));
    let (_, unit) = counters(db.pool(), &endpoint).await;
    let capped = store.clone().with_endpoint_byte_caps(EndpointByteCaps {
        anonymous: Some(3 * unit),
        owner_default: None,
    });

    for _ in 0..2 {
        assert!(matches!(
            send(&capped, &endpoint).await,
            CaptureOutcome::Stored { .. }
        ));
    }
    let refused = send(&capped, &endpoint).await;

    assert_eq!(refused, CaptureOutcome::Full);
    assert_eq!(counters(db.pool(), &endpoint).await, (3, 3 * unit));
}

#[tokio::test]
async fn a_ring_mode_endpoint_at_its_byte_cap_evicts_its_own_oldest_captures() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let endpoint = create(
        &store,
        None,
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    set_ring(db.pool(), &endpoint).await;
    assert!(matches!(
        send(&store, &endpoint).await,
        CaptureOutcome::Stored { .. }
    ));
    let (_, unit) = counters(db.pool(), &endpoint).await;
    let capped = store.clone().with_endpoint_byte_caps(EndpointByteCaps {
        anonymous: Some(3 * unit),
        owner_default: None,
    });

    for _ in 0..6 {
        assert!(matches!(
            send(&capped, &endpoint).await,
            CaptureOutcome::Stored { .. }
        ));
    }

    assert_eq!(counters(db.pool(), &endpoint).await, (3, 3 * unit));
}

#[tokio::test]
async fn the_owner_flag_stops_refuse_mode_endpoints_while_ring_endpoints_keep_accepting() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    let refusing = create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    let ring = create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    set_ring(db.pool(), &ring).await;
    sqlx::query("UPDATE users SET storage_over_quota = true WHERE id = $1")
        .bind(owner)
        .execute(db.pool())
        .await
        .expect("the flag is set");

    let refused = send(&store, &refusing).await;
    let accepted = send(&store, &ring).await;

    assert_eq!(refused, CaptureOutcome::OverQuota);
    assert!(matches!(accepted, CaptureOutcome::Stored { .. }));
}

#[tokio::test]
async fn set_tier_ignores_an_email_address_that_is_not_verified() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    sqlx::query(
        "INSERT INTO users (id, issuer, subject, email, email_verified) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, 'claimed@example.test', false)",
    )
    .execute(db.pool())
    .await
    .expect("the user inserts");

    let outcome = store
        .set_user_tier(
            &UserSelector::Email("claimed@example.test".to_owned()),
            "pro",
        )
        .await
        .expect("the lookup runs");

    assert_eq!(outcome, SetTierOutcome::NotFound);
}

#[tokio::test]
async fn an_endpoint_update_waits_for_the_owner_row_before_it_locks_the_endpoint() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool(), "owner@example.test").await;
    let endpoint = create(
        &store,
        Some(owner),
        None,
        Lifecycle::default(),
        LifecycleLimits::NONE,
    )
    .await
    .expect("created");
    let mut holder = db.pool().begin().await.expect("a transaction opens");
    sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(owner)
        .execute(&mut *holder)
        .await
        .expect("the owner row is locked");
    let update = tokio::spawn({
        let store = store.clone();
        let endpoint = endpoint.clone();
        async move {
            store
                .update_endpoint::<LifecycleError, _>(
                    &endpoint,
                    RETENTION,
                    LifecycleLimits::NONE,
                    |current, _owner| {
                        Ok(EndpointChange {
                            settings: current.settings.clone(),
                            lifecycle: current.lifecycle,
                        })
                    },
                )
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let free = sqlx::query("SELECT id FROM endpoints WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(endpoint.unpack())
        .execute(db.pool())
        .await;
    holder.commit().await.expect("the owner row is released");
    let updated = update.await.expect("the task finishes");

    assert!(free.is_ok(), "the endpoint row stayed free: {free:?}");
    assert!(updated.expect("the update runs").is_some());
}
