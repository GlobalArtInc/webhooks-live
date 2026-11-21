use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use sqlx::PgPool;
use sqlx::types::Uuid;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::{
    EndpointChange, EndpointSettings, Lifecycle, LifecycleError, LifecycleLimits, NewEndpoint,
    Overflow, Store,
};
use whl_testkit::TestDb;

const DEFAULT_RETENTION: Duration = Duration::from_secs(3600);
const DEFAULT_MAX_CAPTURES: u32 = 500;

fn settings(overflow: Overflow) -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        templating: Default::default(),
        cors: false,
        overflow,
        tags: Vec::new(),
        verifier: None,
    }
}

fn limits(max: u32) -> LifecycleLimits {
    LifecycleLimits {
        never_expire_max: max,
        ..LifecycleLimits::NONE
    }
}

async fn insert_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, 'user@example.test') \
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("the user inserts")
}

async fn create(
    store: &Store,
    owner: Option<Uuid>,
    lifecycle: Lifecycle,
    overflow: Overflow,
    limit: u32,
) -> Result<EndpointId, LifecycleError> {
    let settings = settings(overflow);
    store
        .create_endpoint_with_lifecycle(
            NewEndpoint {
                owner,
                creator_net: None,
                settings: &settings,
                lifecycle,
                limits: limits(limit),
            },
            DEFAULT_RETENTION,
        )
        .await
        .map(|record| record.id)
}

async fn age(pool: &PgPool, endpoint: &EndpointId, seconds: i64) {
    sqlx::query(
        "UPDATE endpoints SET last_active_at = now() - make_interval(secs => $2) WHERE id = $1",
    )
    .bind(endpoint.unpack())
    .bind(seconds as f64)
    .execute(pool)
    .await
    .expect("the activity time moves");
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
                max_captures: DEFAULT_MAX_CAPTURES,
                retention: DEFAULT_RETENTION,
            },
        )
        .await
        .expect("the capture call runs")
}

fn never() -> Lifecycle {
    Lifecycle {
        never_expires: true,
        ..Lifecycle::default()
    }
}

#[tokio::test]
async fn a_capture_honours_the_per_endpoint_cap_in_both_overflow_modes() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let capped = Lifecycle {
        max_captures: Some(2),
        ..Lifecycle::default()
    };
    let refusing = create(&store, None, capped, Overflow::Refuse, 0)
        .await
        .expect("the endpoint is created");
    assert!(matches!(
        send(&store, &refusing).await,
        CaptureOutcome::Stored { .. }
    ));
    assert!(matches!(
        send(&store, &refusing).await,
        CaptureOutcome::Stored { .. }
    ));
    assert_eq!(send(&store, &refusing).await, CaptureOutcome::Full);

    let ring = create(&store, None, capped, Overflow::Ring, 0)
        .await
        .expect("the endpoint is created");
    for _ in 0..4 {
        assert!(matches!(
            send(&store, &ring).await,
            CaptureOutcome::Stored { .. }
        ));
    }
    let kept: Vec<i64> =
        sqlx::query_scalar("SELECT seq FROM captures WHERE endpoint_id = $1 ORDER BY seq")
            .bind(ring.unpack())
            .fetch_all(db.pool())
            .await
            .expect("the sequences read");
    assert_eq!(kept, [3, 4]);
    let count: i32 = sqlx::query_scalar("SELECT capture_count FROM endpoints WHERE id = $1")
        .bind(ring.unpack())
        .fetch_one(db.pool())
        .await
        .expect("the count reads");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn a_capture_honours_the_per_endpoint_retention_and_never_expire() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;

    let longer = create(
        &store,
        None,
        Lifecycle {
            retention_seconds: Some(7200),
            ..Lifecycle::default()
        },
        Overflow::Refuse,
        0,
    )
    .await
    .expect("the endpoint is created");
    age(db.pool(), &longer, 5400).await;
    assert!(matches!(
        send(&store, &longer).await,
        CaptureOutcome::Stored { .. }
    ));

    let plain = create(&store, None, Lifecycle::default(), Overflow::Refuse, 0)
        .await
        .expect("the endpoint is created");
    age(db.pool(), &plain, 5400).await;
    assert_eq!(send(&store, &plain).await, CaptureOutcome::Gone);

    let pinned = create(&store, Some(owner), never(), Overflow::Refuse, 1)
        .await
        .expect("the endpoint is created");
    age(db.pool(), &pinned, 10_000_000).await;
    assert!(matches!(
        send(&store, &pinned).await,
        CaptureOutcome::Stored { .. }
    ));
    let state = store
        .get_endpoint(&pinned, DEFAULT_RETENTION)
        .await
        .expect("the endpoint reads")
        .expect("the endpoint is live");
    assert_eq!(state.expires_at, None);
}

#[tokio::test]
async fn parallel_updates_never_push_an_owner_past_the_never_expire_limit() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let mut ids = Vec::new();
    for _ in 0..8 {
        ids.push(
            create(
                &store,
                Some(owner),
                Lifecycle::default(),
                Overflow::Refuse,
                0,
            )
            .await
            .expect("the endpoint is created"),
        );
    }

    let mut attempts = Vec::new();
    for id in ids {
        let store = store.clone();
        attempts.push(tokio::spawn(async move {
            store
                .update_endpoint::<LifecycleError, _>(
                    &id,
                    DEFAULT_RETENTION,
                    limits(3),
                    |current, _owner| {
                        Ok(EndpointChange {
                            settings: current.settings.clone(),
                            lifecycle: never(),
                        })
                    },
                )
                .await
        }));
    }
    let mut accepted = 0;
    let mut refused = 0;
    for attempt in attempts {
        match attempt.await.expect("the task finishes") {
            Ok(Some(_)) => accepted += 1,
            Err(LifecycleError::NeverExpireLimit { max: 3 }) => refused += 1,
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert_eq!((accepted, refused), (3, 5));
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoints WHERE never_expires")
        .fetch_one(db.pool())
        .await
        .expect("the count reads");
    assert_eq!(held, 3);
}

#[tokio::test]
async fn a_deleted_never_expire_endpoint_frees_its_place_and_an_ownerless_one_is_refused() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let first = create(&store, Some(owner), never(), Overflow::Refuse, 1)
        .await
        .expect("the first endpoint is created");
    assert!(matches!(
        create(&store, Some(owner), never(), Overflow::Refuse, 1).await,
        Err(LifecycleError::NeverExpireLimit { max: 1 })
    ));
    assert!(
        store
            .delete_endpoint(&first, DEFAULT_RETENTION)
            .await
            .expect("the endpoint deletes")
    );
    create(&store, Some(owner), never(), Overflow::Refuse, 1)
        .await
        .expect("the place is free again");
    assert!(matches!(
        create(&store, None, never(), Overflow::Refuse, 5).await,
        Err(LifecycleError::NeverExpireNeedsOwner)
    ));
}

#[tokio::test]
async fn the_database_refuses_a_never_expire_endpoint_without_an_owner() {
    let db = TestDb::new().await;
    let id = EndpointId::generate().expect("entropy");
    let refused = sqlx::query("INSERT INTO endpoints (id, never_expires) VALUES ($1, true)")
        .bind(id.unpack())
        .execute(db.pool())
        .await;
    assert!(refused.is_err());
}

#[tokio::test]
async fn an_update_keeps_an_existing_never_expire_flag_without_counting_it_again() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let id = create(&store, Some(owner), never(), Overflow::Refuse, 1)
        .await
        .expect("the endpoint is created");
    let record = store
        .update_endpoint::<LifecycleError, _>(
            &id,
            DEFAULT_RETENTION,
            limits(1),
            |current, _owner| {
                Ok(EndpointChange {
                    settings: EndpointSettings {
                        title: Some("renamed".to_owned()),
                        ..current.settings.clone()
                    },
                    lifecycle: current.lifecycle,
                })
            },
        )
        .await
        .expect("the update runs")
        .expect("the endpoint is live");
    assert!(record.lifecycle.never_expires);
    assert_eq!(record.settings.title.as_deref(), Some("renamed"));
}
