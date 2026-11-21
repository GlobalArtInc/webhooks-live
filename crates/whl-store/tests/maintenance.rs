use std::collections::BTreeSet;
use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::watch;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::captures::EVENT_CHANNEL;
use whl_store::maintenance::{
    MaintenanceHandle, MaintenanceSettings, below_cap, exceeds_cap, exceeds_hard_limit,
    spawn_maintenance,
};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);

fn settings(cap_bytes: u64) -> MaintenanceSettings {
    MaintenanceSettings {
        deleted_grace: Duration::from_secs(10),
        purge_batch: 2,
        evict_batch: 1,
        cycle_interval: Duration::from_millis(50),
        retry_interval: Duration::from_millis(200),
        lock_check_interval: Duration::from_millis(50),
        ..MaintenanceSettings::new(RETENTION, cap_bytes)
    }
}

struct Seed {
    bytes: i64,
    active_ago_secs: i64,
    deleted_ago_secs: Option<i64>,
}

impl Seed {
    fn active(active_ago_secs: i64) -> Self {
        Self {
            bytes: 0,
            active_ago_secs,
            deleted_ago_secs: None,
        }
    }

    fn with_bytes(mut self, bytes: i64) -> Self {
        self.bytes = bytes;
        self
    }

    fn deleted(mut self, deleted_ago_secs: i64) -> Self {
        self.deleted_ago_secs = Some(deleted_ago_secs);
        self
    }
}

async fn seed(pool: &PgPool, seed: Seed) -> String {
    let id = EndpointId::generate()
        .expect("entropy is available")
        .unpack()
        .to_owned();
    sqlx::query(
        "INSERT INTO endpoints (id, bytes_used, capture_count, last_active_at, deleted_at) \
         VALUES ($1, $2, 1, now() - make_interval(secs => $3), \
                 CASE WHEN $4::float8 IS NULL THEN NULL ELSE now() - make_interval(secs => $4) END)",
    )
    .bind(&id)
    .bind(seed.bytes)
    .bind(seed.active_ago_secs as f64)
    .bind(seed.deleted_ago_secs.map(|secs| secs as f64))
    .execute(pool)
    .await
    .expect("the endpoint inserts");
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, body_size, stored_bytes, response_status) \
         VALUES ($1, 1, gen_random_uuid(), now(), 'GET', 'http://x/', '', 'x', '127.0.0.1', \
         '[]', '[]', 0, 0, 200)",
    )
    .bind(&id)
    .execute(pool)
    .await
    .expect("the capture inserts");
    sqlx::query(
        "INSERT INTO capture_bodies (capture_id, data, search_text) \
         SELECT id, ''::bytea, '' FROM captures WHERE endpoint_id = $1",
    )
    .bind(&id)
    .execute(pool)
    .await
    .expect("the body inserts");
    id
}

async fn endpoint_ids(pool: &PgPool) -> BTreeSet<String> {
    sqlx::query_scalar::<_, String>("SELECT id FROM endpoints")
        .fetch_all(pool)
        .await
        .expect("endpoints read")
        .into_iter()
        .collect()
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count reads")
}

async fn advisory_holders(pool: &PgPool) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT pid FROM pg_locks WHERE locktype = 'advisory' AND granted \
         AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
    )
    .fetch_all(pool)
    .await
    .expect("locks read")
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..400 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

fn stop_signal() -> (
    watch::Sender<bool>,
    impl Future<Output = ()> + Send + 'static,
) {
    let (sender, mut receiver) = watch::channel(false);
    (sender, async move {
        let _ = receiver.wait_for(|stopped| *stopped).await;
    })
}

fn runner(db: &TestDb, cap_bytes: u64) -> (MaintenanceHandle, watch::Sender<bool>) {
    let (sender, signal) = stop_signal();
    let handle = spawn_maintenance(
        Store::from_pool(db.pool().clone()),
        settings(cap_bytes),
        signal,
    );
    (handle, sender)
}

#[test]
fn thresholds_are_strict_at_the_boundaries() {
    assert!(!exceeds_hard_limit(1100, 1000));
    assert!(exceeds_hard_limit(1101, 1000));
    assert!(!exceeds_cap(1000, 1000));
    assert!(exceeds_cap(1001, 1000));
    assert!(!below_cap(1000, 1000));
    assert!(below_cap(999, 1000));
    assert!(exceeds_hard_limit(u64::MAX, 1));
}

#[tokio::test]
async fn storage_bytes_sums_every_endpoint() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    assert_eq!(store.storage_bytes().await.expect("sum reads"), 0);
    seed(db.pool(), Seed::active(0).with_bytes(300)).await;
    seed(db.pool(), Seed::active(0).with_bytes(200).deleted(0)).await;
    assert_eq!(store.storage_bytes().await.expect("sum reads"), 500);
}

#[tokio::test]
async fn purge_removes_expired_and_deleted_endpoints_in_batches_and_notifies() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let live = seed(db.pool(), Seed::active(10)).await;
    let recently_deleted = seed(db.pool(), Seed::active(10).deleted(1)).await;
    let mut victims = BTreeSet::new();
    for _ in 0..4 {
        victims.insert(seed(db.pool(), Seed::active(7200)).await);
    }
    victims.insert(seed(db.pool(), Seed::active(10).deleted(120)).await);
    let mut listener = PgListener::connect(db.url())
        .await
        .expect("the listener connects");
    listener
        .listen(EVENT_CHANNEL)
        .await
        .expect("the listener subscribes");

    let settings = settings(u64::MAX);
    let mut sizes = Vec::new();
    let (mut expired, mut deleted) = (0, 0);
    loop {
        let batch = store.purge_batch(&settings).await.expect("the batch runs");
        if batch.total() == 0 {
            break;
        }
        sizes.push(batch.total());
        expired += batch.expired;
        deleted += batch.deleted;
    }
    assert_eq!(sizes, [2, 2, 1]);
    assert_eq!((expired, deleted), (4, 1));

    let remaining: BTreeSet<String> = [live, recently_deleted].into_iter().collect();
    assert_eq!(endpoint_ids(db.pool()).await, remaining);
    assert_eq!(count(db.pool(), "captures").await, 2);
    assert_eq!(count(db.pool(), "capture_bodies").await, 2);

    let mut notified = BTreeSet::new();
    while notified.len() < victims.len() {
        let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
            .await
            .expect("a notification arrives")
            .expect("the listener stays connected");
        let payload: serde_json::Value =
            serde_json::from_str(notification.payload()).expect("the payload is JSON");
        let id = payload["e"].as_str().expect("the endpoint id").to_owned();
        assert_eq!(
            payload,
            serde_json::json!({ "t": "endpoint_deleted", "e": id })
        );
        notified.insert(id);
    }
    assert_eq!(notified, victims);
}

#[tokio::test]
async fn eviction_takes_the_least_recently_active_endpoint_first_and_skips_empty_ones() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let empty_oldest = seed(db.pool(), Seed::active(3000)).await;
    let oldest = seed(db.pool(), Seed::active(2000).with_bytes(100)).await;
    let middle = seed(db.pool(), Seed::active(1000).with_bytes(200)).await;
    let newest = seed(db.pool(), Seed::active(10).with_bytes(300)).await;
    let mut listener = PgListener::connect(db.url())
        .await
        .expect("the listener connects");
    listener
        .listen(EVENT_CHANNEL)
        .await
        .expect("the listener subscribes");

    let settings = settings(u64::MAX);
    for (victim, bytes) in [(&oldest, 100), (&middle, 200), (&newest, 300)] {
        let batch = store
            .evict_batch(&settings, u64::MAX)
            .await
            .expect("the batch runs");
        assert_eq!((batch.endpoints, batch.bytes), (1, bytes));
        let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
            .await
            .expect("a notification arrives")
            .expect("the listener stays connected");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(notification.payload())
                .expect("the payload is JSON"),
            serde_json::json!({ "t": "endpoint_deleted", "e": victim })
        );
    }
    let batch = store
        .evict_batch(&settings, u64::MAX)
        .await
        .expect("the batch runs");
    assert_eq!(batch.endpoints, 0);
    assert_eq!(
        endpoint_ids(db.pool()).await,
        BTreeSet::from([empty_oldest])
    );
}

#[tokio::test]
async fn a_batch_frees_only_as_many_endpoints_as_the_shortfall_requires() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    seed(db.pool(), Seed::active(3000).with_bytes(100)).await;
    seed(db.pool(), Seed::active(2000).with_bytes(200)).await;
    let newest = seed(db.pool(), Seed::active(10).with_bytes(300)).await;

    let settings = MaintenanceSettings {
        evict_batch: 5,
        ..settings(u64::MAX)
    };
    let batch = store
        .evict_batch(&settings, 101)
        .await
        .expect("the batch runs");
    assert_eq!((batch.endpoints, batch.bytes), (2, 300));
    assert_eq!(endpoint_ids(db.pool()).await, BTreeSet::from([newest]));
}

#[tokio::test]
async fn the_leader_evicts_until_usage_is_below_the_cap_and_purges_expired_endpoints() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let expired = seed(db.pool(), Seed::active(7200)).await;
    seed(db.pool(), Seed::active(3000).with_bytes(100)).await;
    seed(db.pool(), Seed::active(2000).with_bytes(200)).await;
    let newest = seed(db.pool(), Seed::active(10).with_bytes(300)).await;

    let (handle, stop) = runner(&db, 450);
    eventually("the leader to bring usage under the cap", || async {
        store.storage_bytes().await.is_ok_and(|used| used < 450)
    })
    .await;
    eventually("the expired endpoint to go", || async {
        !endpoint_ids(db.pool()).await.contains(&expired)
    })
    .await;
    assert_eq!(
        endpoint_ids(db.pool()).await,
        BTreeSet::from([newest.clone()])
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(store.storage_bytes().await.expect("sum reads"), 300);
    assert_eq!(count(db.pool(), "captures").await, 1);

    let _ = stop.send(true);
    handle.join().await;
}

#[tokio::test]
async fn nothing_is_evicted_while_usage_is_within_the_cap() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let oldest = seed(db.pool(), Seed::active(3000).with_bytes(100)).await;
    let middle = seed(db.pool(), Seed::active(2000).with_bytes(200)).await;
    let newest = seed(db.pool(), Seed::active(10).with_bytes(300)).await;

    let (handle, stop) = runner(&db, 700);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        endpoint_ids(db.pool()).await,
        BTreeSet::from([oldest, middle, newest])
    );
    assert_eq!(store.storage_bytes().await.expect("sum reads"), 600);

    let _ = stop.send(true);
    handle.join().await;
}

#[tokio::test]
async fn the_leader_deletes_expired_sessions_and_login_states() {
    let db = TestDb::new().await;
    sqlx::query(
        "INSERT INTO users (id, issuer, subject, email) \
         VALUES (gen_random_uuid(), 'https://issuer.test', 'subject', 'user@example.test')",
    )
    .execute(db.pool())
    .await
    .expect("the user inserts");
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, last_seen_at, expires_at) \
         SELECT decode('01', 'hex'), id, now(), now() - interval '1 minute' FROM users \
         UNION ALL SELECT decode('02', 'hex'), id, now(), now() + interval '1 hour' FROM users \
         UNION ALL SELECT decode('03', 'hex'), id, now() - interval '13 hours', now() + interval '1 hour' FROM users",
    )
    .execute(db.pool())
    .await
    .expect("the sessions insert");
    sqlx::query(
        "INSERT INTO login_states (state, pkce_verifier, nonce, return_to, expires_at) VALUES \
         ('old', 'v', 'n', '/', now() - interval '1 minute'), \
         ('fresh', 'v', 'n', '/', now() + interval '1 hour')",
    )
    .execute(db.pool())
    .await
    .expect("the login states insert");

    let (handle, stop) = runner(&db, u64::MAX);
    eventually("the expired rows to go", || async {
        count(db.pool(), "sessions").await == 1 && count(db.pool(), "login_states").await == 1
    })
    .await;
    let kept: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_all(db.pool())
        .await
        .expect("sessions read");
    assert_eq!(kept, [vec![2_u8]]);

    let _ = stop.send(true);
    handle.join().await;
}

#[tokio::test]
async fn exactly_one_runner_leads_and_the_other_takes_over_when_the_leader_connection_dies() {
    let db = TestDb::new().await;
    let (first, first_stop) = runner(&db, u64::MAX);
    let (second, second_stop) = runner(&db, u64::MAX);

    eventually("a leader to emerge", || async {
        first.is_leader() || second.is_leader()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(first.is_leader() != second.is_leader());
    let holders = advisory_holders(db.pool()).await;
    assert_eq!(holders.len(), 1);

    let (leader, follower) = if first.is_leader() {
        (&first, &second)
    } else {
        (&second, &first)
    };
    let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(holders[0])
        .fetch_one(db.pool())
        .await
        .expect("the backend terminates");
    assert!(terminated);

    tokio::time::timeout(Duration::from_secs(5), follower.wait_for_leadership(true))
        .await
        .expect("the follower takes over within the retry interval")
        .expect("the follower task runs");
    tokio::time::timeout(Duration::from_secs(5), leader.wait_for_leadership(false))
        .await
        .expect("the old leader notices the loss")
        .expect("the old leader task runs");
    eventually("a single new holder", || async {
        let now = advisory_holders(db.pool()).await;
        now.len() == 1 && now[0] != holders[0]
    })
    .await;
    assert!(follower.is_leader() && !leader.is_leader());

    let _ = first_stop.send(true);
    let _ = second_stop.send(true);
    first.join().await;
    second.join().await;
}

#[tokio::test]
async fn shutdown_releases_the_lock_so_another_replica_can_lead() {
    let db = TestDb::new().await;
    let (leader, stop) = runner(&db, u64::MAX);
    leader
        .wait_for_leadership(true)
        .await
        .expect("the runner leads");
    assert_eq!(advisory_holders(db.pool()).await.len(), 1);

    let _ = stop.send(true);
    leader.join().await;
    assert!(advisory_holders(db.pool()).await.is_empty());

    let (successor, successor_stop) = runner(&db, u64::MAX);
    tokio::time::timeout(Duration::from_secs(5), successor.wait_for_leadership(true))
        .await
        .expect("the successor leads")
        .expect("the successor task runs");
    let _ = successor_stop.send(true);
    successor.join().await;
}
