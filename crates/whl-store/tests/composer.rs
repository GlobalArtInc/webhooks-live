use std::time::Duration;

use serde_json::json;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::composer::{
    NewSent, QuotaCharge, QuotaOutcome, QuotaWindow, SentOutcome, SentOwner,
};
use whl_store::{EndpointSettings, Overflow, Store, Uuid, VerifiedIdentity};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);

fn charge(subject: &str, window: QuotaWindow, limit: u32) -> QuotaCharge {
    QuotaCharge {
        subject: subject.to_owned(),
        window,
        limit,
        name: "test_limit",
    }
}

async fn counter(store: &Store, subject: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT coalesce(sum(count), 0)::bigint FROM usage_counters WHERE subject = $1",
    )
    .bind(subject)
    .fetch_one(store.pool())
    .await
    .expect("the counter is read")
}

async fn user(store: &Store, tag: &str) -> Uuid {
    store
        .upsert_user(&VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: tag.to_owned(),
            email: format!("{tag}@example.test"),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored")
        .id
}

async fn endpoint(store: &Store) -> EndpointId {
    store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                cors: false,
                overflow: Overflow::Refuse,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created")
        .id
}

fn sent(owner: SentOwner) -> NewSent {
    NewSent {
        id: whl_core::capture_id::CaptureId::generate()
            .expect("an id")
            .unpack(),
        owner,
        source_capture_id: None,
        request: json!({"method": "GET"}),
        result: json!({"status": 200}),
        response_body: Some(b"body".to_vec()),
    }
}

#[tokio::test]
async fn a_quota_counts_up_to_its_limit_and_then_refuses_with_a_retry_hint() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let charges = [charge("u:1", QuotaWindow::Hour, 3)];

    for _ in 0..3 {
        assert_eq!(
            store
                .consume_quota("outbound", &charges)
                .await
                .expect("counted"),
            QuotaOutcome::Allowed
        );
    }
    let refused = store
        .consume_quota("outbound", &charges)
        .await
        .expect("counted");

    match refused {
        QuotaOutcome::Exceeded {
            name,
            limit,
            retry_after,
        } => {
            assert_eq!((name, limit), ("test_limit", 3));
            assert!(
                retry_after >= Duration::from_secs(1) && retry_after <= Duration::from_secs(3600)
            );
        }
        QuotaOutcome::Allowed => panic!("the fourth use must be refused"),
    }
    assert_eq!(counter(&store, "u:1").await, 3);
}

#[tokio::test]
async fn a_refused_charge_leaves_the_other_charges_of_the_same_action_uncounted() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    store
        .consume_quota("outbound", &[charge("e:x", QuotaWindow::Hour, 1)])
        .await
        .expect("counted");

    let outcome = store
        .consume_quota(
            "outbound",
            &[
                charge("n:a", QuotaWindow::Day, 10),
                charge("e:x", QuotaWindow::Hour, 1),
            ],
        )
        .await
        .expect("counted");

    assert!(matches!(outcome, QuotaOutcome::Exceeded { .. }));
    assert_eq!(counter(&store, "n:a").await, 0);
}

#[tokio::test]
async fn fifty_parallel_charges_never_pass_a_limit_of_ten() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..50 {
        let store = store.clone();
        tasks.spawn(async move {
            store
                .consume_quota("outbound", &[charge("n:b", QuotaWindow::Day, 10)])
                .await
                .expect("counted")
        });
    }
    let mut allowed = 0;
    while let Some(outcome) = tasks.join_next().await {
        if outcome.expect("the task finishes") == QuotaOutcome::Allowed {
            allowed += 1;
        }
    }

    assert_eq!(allowed, 10);
    assert_eq!(counter(&store, "n:b").await, 10);
}

#[tokio::test]
async fn a_limit_of_zero_refuses_without_touching_the_counter() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());

    let outcome = store
        .consume_quota("outbound", &[charge("n:c", QuotaWindow::Day, 0)])
        .await
        .expect("counted");

    assert!(matches!(outcome, QuotaOutcome::Exceeded { .. }));
    assert_eq!(counter(&store, "n:c").await, 0);
}

#[tokio::test]
async fn old_usage_windows_are_purged_and_current_ones_stay() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    store
        .consume_quota("outbound", &[charge("n:d", QuotaWindow::Hour, 5)])
        .await
        .expect("counted");
    sqlx::query(
        "INSERT INTO usage_counters (subject, metric, window_start, count) \
         VALUES ('n:old', 'outbound', now() - interval '3 days', 4)",
    )
    .execute(store.pool())
    .await
    .expect("an old window is inserted");

    let deleted = store.delete_old_usage_windows().await.expect("purged");

    assert_eq!(deleted, 1);
    assert_eq!(counter(&store, "n:d").await, 1);
}

#[tokio::test]
async fn history_keeps_the_newest_fifty_per_owner_and_leaves_other_owners_alone() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let alice = user(&store, "alice").await;
    let bob = user(&store, "bob").await;
    store
        .record_sent(&sent(SentOwner::User(bob)))
        .await
        .expect("recorded");

    for _ in 0..53 {
        assert_eq!(
            store
                .record_sent(&sent(SentOwner::User(alice)))
                .await
                .expect("recorded"),
            SentOutcome::Recorded
        );
    }

    let alices = store
        .list_sent(&SentOwner::User(alice), 50, None)
        .await
        .expect("listed");
    let bobs = store
        .list_sent(&SentOwner::User(bob), 50, None)
        .await
        .expect("listed");
    assert_eq!(alices.items.len(), 50);
    assert_eq!(alices.next_after, None);
    assert_eq!(bobs.items.len(), 1);
}

#[tokio::test]
async fn history_pages_newest_first_and_a_delete_needs_the_owner() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let alice = user(&store, "alice").await;
    let bob = user(&store, "bob").await;
    let mut ids = Vec::new();
    for _ in 0..5 {
        let entry = sent(SentOwner::User(alice));
        ids.push(entry.id);
        store.record_sent(&entry).await.expect("recorded");
    }
    ids.reverse();

    let first = store
        .list_sent(&SentOwner::User(alice), 3, None)
        .await
        .expect("listed");
    let second = store
        .list_sent(&SentOwner::User(alice), 3, first.next_after)
        .await
        .expect("listed");

    let listed: Vec<Uuid> = first
        .items
        .iter()
        .chain(&second.items)
        .map(|item| item.id)
        .collect();
    assert_eq!(listed, ids);
    assert_eq!(second.next_after, None);
    assert!(
        !store
            .delete_sent(ids[0], &SentOwner::User(bob))
            .await
            .expect("deleted")
    );
    assert!(
        store
            .delete_sent(ids[0], &SentOwner::User(alice))
            .await
            .expect("deleted")
    );
    assert_eq!(store.sent_owner(ids[0]).await.expect("read"), None);
}

#[tokio::test]
async fn an_entry_for_a_vanished_endpoint_is_reported_not_stored() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let gone = EndpointId::parse("7k2m9q4xw8c1d5f3h6j0n2p4").expect("an id");
    let live = endpoint(&store).await;

    let outcome = store
        .record_sent(&sent(SentOwner::Endpoint(gone)))
        .await
        .expect("handled");
    let kept = store
        .record_sent(&sent(SentOwner::Endpoint(live.clone())))
        .await
        .expect("handled");

    assert_eq!(outcome, SentOutcome::OwnerGone);
    assert_eq!(kept, SentOutcome::Recorded);
    assert_eq!(
        store
            .list_sent(&SentOwner::Endpoint(live), 50, None)
            .await
            .expect("listed")
            .items
            .len(),
        1
    );
}
