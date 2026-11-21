use std::collections::BTreeSet;
use std::future::Future;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgListener;
use sqlx::types::Uuid;
use tokio::sync::watch;
use whl_core::endpoint_id::EndpointId;
use whl_core::tier::{TierLimits, Tiers};
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
    owner: Option<Uuid>,
    never_expires: bool,
    retention_seconds: Option<i32>,
    capture_ttl_seconds: Option<i32>,
}

impl Seed {
    fn active(active_ago_secs: i64) -> Self {
        Self {
            bytes: 0,
            active_ago_secs,
            deleted_ago_secs: None,
            owner: None,
            never_expires: false,
            retention_seconds: None,
            capture_ttl_seconds: None,
        }
    }

    fn owned_by(mut self, owner: Uuid) -> Self {
        self.owner = Some(owner);
        self
    }

    fn never_expiring(mut self) -> Self {
        self.never_expires = true;
        self
    }

    fn retained_for(mut self, seconds: i32) -> Self {
        self.retention_seconds = Some(seconds);
        self
    }

    fn with_ttl(mut self, seconds: i32) -> Self {
        self.capture_ttl_seconds = Some(seconds);
        self
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
        "INSERT INTO endpoints (id, bytes_used, capture_count, last_active_at, deleted_at, \
         owner_id, never_expires, retention_seconds, capture_ttl_seconds) \
         VALUES ($1, $2, 1, now() - make_interval(secs => $3), \
                 CASE WHEN $4::float8 IS NULL THEN NULL ELSE now() - make_interval(secs => $4) END, \
                 $5, $6, $7, $8)",
    )
    .bind(&id)
    .bind(seed.bytes)
    .bind(seed.active_ago_secs as f64)
    .bind(seed.deleted_ago_secs.map(|secs| secs as f64))
    .bind(seed.owner)
    .bind(seed.never_expires)
    .bind(seed.retention_seconds)
    .bind(seed.capture_ttl_seconds)
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

async fn seed_aged_captures(pool: &PgPool, endpoint: &str, ages_secs: &[i64]) {
    sqlx::query("DELETE FROM captures WHERE endpoint_id = $1")
        .bind(endpoint)
        .execute(pool)
        .await
        .expect("the seeded capture clears");
    for (index, age) in ages_secs.iter().enumerate() {
        sqlx::query(
            "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
             client_ip, headers, query, body_size, stored_bytes, response_status) \
             VALUES ($1, $2, gen_random_uuid(), now() - make_interval(secs => $3), 'GET', \
             'http://x/', '', 'x', '127.0.0.1', '[]', '[]', 0, 10, 200)",
        )
        .bind(endpoint)
        .bind(i64::try_from(index + 1).expect("the sequence fits"))
        .bind(*age as f64)
        .execute(pool)
        .await
        .expect("the capture inserts");
    }
    sqlx::query(
        "INSERT INTO capture_bodies (capture_id, data, search_text) \
         SELECT id, ''::bytea, '' FROM captures WHERE endpoint_id = $1",
    )
    .bind(endpoint)
    .execute(pool)
    .await
    .expect("the bodies insert");
    sqlx::query(
        "UPDATE endpoints SET capture_count = $2, bytes_used = $3, last_seq = $2 WHERE id = $1",
    )
    .bind(endpoint)
    .bind(i32::try_from(ages_secs.len()).expect("the count fits"))
    .bind(i64::try_from(ages_secs.len()).expect("the count fits") * 10)
    .execute(pool)
    .await
    .expect("the counters set");
}

async fn counters(pool: &PgPool, endpoint: &str) -> (i32, i64) {
    sqlx::query_as("SELECT capture_count, bytes_used FROM endpoints WHERE id = $1")
        .bind(endpoint)
        .fetch_one(pool)
        .await
        .expect("the counters read")
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

#[tokio::test]
async fn purge_applies_each_endpoint_its_own_retention_instead_of_the_default() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let longer = seed(db.pool(), Seed::active(5400).retained_for(7200)).await;
    let shorter = seed(db.pool(), Seed::active(4000).retained_for(3600)).await;
    let plain_fresh = seed(db.pool(), Seed::active(4000)).await;
    let plain_stale = seed(db.pool(), Seed::active(100_000)).await;

    let settings = MaintenanceSettings {
        retention: Duration::from_secs(86_400),
        ..settings(u64::MAX)
    };
    let batch = store.purge_batch(&settings).await.expect("the batch runs");
    assert_eq!((batch.expired, batch.deleted), (2, 0));
    let remaining = endpoint_ids(db.pool()).await;
    assert_eq!(remaining, BTreeSet::from([longer, plain_fresh]));
    assert!(!remaining.contains(&shorter));
    assert!(!remaining.contains(&plain_stale));
}

#[tokio::test]
async fn purge_never_expires_a_never_expire_endpoint_but_still_removes_it_once_deleted() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let kept = seed(
        db.pool(),
        Seed::active(10_000_000).owned_by(owner).never_expiring(),
    )
    .await;
    let deleted = seed(
        db.pool(),
        Seed::active(10)
            .owned_by(owner)
            .never_expiring()
            .deleted(120),
    )
    .await;
    let stale = seed(db.pool(), Seed::active(10_000_000).owned_by(owner)).await;

    let batch = store
        .purge_batch(&settings(u64::MAX))
        .await
        .expect("the batch runs");
    assert_eq!((batch.expired, batch.deleted), (1, 1));
    let remaining = endpoint_ids(db.pool()).await;
    assert_eq!(remaining, BTreeSet::from([kept]));
    assert!(!remaining.contains(&stale));
    assert!(!remaining.contains(&deleted));
}

#[tokio::test]
async fn eviction_takes_anonymous_endpoints_before_owned_ones_and_never_a_never_expire_one() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let pinned = seed(
        db.pool(),
        Seed::active(900_000)
            .with_bytes(400)
            .owned_by(owner)
            .never_expiring(),
    )
    .await;
    let owned_old = seed(
        db.pool(),
        Seed::active(5000).with_bytes(100).owned_by(owner),
    )
    .await;
    let anonymous_new = seed(db.pool(), Seed::active(10).with_bytes(200)).await;
    let anonymous_old = seed(db.pool(), Seed::active(2000).with_bytes(300)).await;

    let settings = settings(u64::MAX);
    let mut order = Vec::new();
    loop {
        let before = endpoint_ids(db.pool()).await;
        let batch = store
            .evict_batch(&settings, u64::MAX)
            .await
            .expect("the batch runs");
        if batch.endpoints == 0 {
            break;
        }
        let after = endpoint_ids(db.pool()).await;
        order.extend(before.difference(&after).cloned());
    }
    assert_eq!(order, [anonymous_old, anonymous_new, owned_old]);
    assert_eq!(endpoint_ids(db.pool()).await, BTreeSet::from([pinned]));
}

#[tokio::test]
async fn the_ttl_purge_deletes_expired_captures_in_batches_and_corrects_the_counters() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = seed(db.pool(), Seed::active(10).with_ttl(60)).await;
    seed_aged_captures(db.pool(), &target, &[500, 400, 300, 10, 5]).await;
    let without_ttl = seed(db.pool(), Seed::active(10)).await;
    seed_aged_captures(db.pool(), &without_ttl, &[500, 400]).await;
    let fresh = seed(db.pool(), Seed::active(10).with_ttl(600)).await;
    seed_aged_captures(db.pool(), &fresh, &[500, 400]).await;
    let mut listener = PgListener::connect(db.url())
        .await
        .expect("the listener connects");
    listener
        .listen(EVENT_CHANNEL)
        .await
        .expect("the listener subscribes");

    let settings = MaintenanceSettings {
        ttl_batch: 2,
        ..settings(u64::MAX)
    };
    let mut sizes = Vec::new();
    loop {
        let due = store
            .endpoints_due_for_capture_ttl(&settings)
            .await
            .expect("the scan runs");
        if due.is_empty() {
            break;
        }
        assert_eq!(due, std::slice::from_ref(&target));
        let batch = store
            .purge_expired_captures(&settings, &target)
            .await
            .expect("the batch runs");
        sizes.push((batch.captures, batch.bytes));
    }
    assert_eq!(sizes, [(2, 20), (1, 10)]);
    assert_eq!(counters(db.pool(), &target).await, (2, 20));
    assert_eq!(counters(db.pool(), &without_ttl).await, (2, 20));
    assert_eq!(counters(db.pool(), &fresh).await, (2, 20));
    assert_eq!(count(db.pool(), "captures").await, 6);
    assert_eq!(count(db.pool(), "capture_bodies").await, 6);

    let mut seqs = BTreeSet::new();
    while seqs.len() < 3 {
        let notification = tokio::time::timeout(Duration::from_secs(5), listener.recv())
            .await
            .expect("a notification arrives")
            .expect("the listener stays connected");
        let payload: serde_json::Value =
            serde_json::from_str(notification.payload()).expect("the payload is JSON");
        assert_eq!(payload["t"], "capture_deleted");
        assert_eq!(payload["e"], target.as_str());
        seqs.insert(payload["d"]["seq"].as_i64().expect("the sequence"));
    }
    assert_eq!(seqs, BTreeSet::from([1, 2, 3]));
}

#[tokio::test]
async fn the_ttl_purge_skips_an_endpoint_that_is_locked_by_a_writer() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = seed(db.pool(), Seed::active(10).with_ttl(60)).await;
    seed_aged_captures(db.pool(), &target, &[500, 400]).await;

    let mut writer = db.pool().begin().await.expect("the writer begins");
    sqlx::query("UPDATE endpoints SET last_seq = last_seq + 1 WHERE id = $1")
        .bind(&target)
        .execute(&mut *writer)
        .await
        .expect("the writer locks the row");
    let batch = store
        .purge_expired_captures(&settings(u64::MAX), &target)
        .await
        .expect("the batch runs");
    assert_eq!(batch.captures, 0);
    writer.commit().await.expect("the writer commits");

    let batch = store
        .purge_expired_captures(&settings(u64::MAX), &target)
        .await
        .expect("the batch runs");
    assert_eq!(batch.captures, 2);
    assert_eq!(counters(db.pool(), &target).await, (0, 0));
}

#[tokio::test]
async fn two_replicas_delete_expired_captures_once_and_keep_the_counters_exact() {
    let db = TestDb::new().await;
    let target = seed(db.pool(), Seed::active(10).with_ttl(60)).await;
    seed_aged_captures(db.pool(), &target, &[900, 800, 700, 600, 500, 5, 4]).await;
    let (first, first_stop) = runner(&db, u64::MAX);
    let (second, second_stop) = runner(&db, u64::MAX);

    eventually("the expired captures to be deleted", || async {
        count(db.pool(), "captures").await == 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(count(db.pool(), "captures").await, 2);
    assert_eq!(count(db.pool(), "capture_bodies").await, 2);
    assert_eq!(counters(db.pool(), &target).await, (2, 20));
    assert!(first.is_leader() != second.is_leader());

    let _ = first_stop.send(true);
    let _ = second_stop.send(true);
    first.join().await;
    second.join().await;
}

#[tokio::test]
async fn the_ttl_purge_keeps_starred_captures_and_stops_scanning_once_only_they_remain() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = seed(db.pool(), Seed::active(10).with_ttl(60)).await;
    seed_aged_captures(db.pool(), &target, &[900, 800, 700, 5]).await;
    sqlx::query("UPDATE captures SET starred = true WHERE endpoint_id = $1 AND seq IN (1, 3)")
        .bind(&target)
        .execute(db.pool())
        .await
        .expect("the captures are starred");

    let settings = settings(u64::MAX);
    assert_eq!(
        store
            .endpoints_due_for_capture_ttl(&settings)
            .await
            .expect("the scan runs"),
        std::slice::from_ref(&target)
    );
    let batch = store
        .purge_expired_captures(&settings, &target)
        .await
        .expect("the batch runs");
    assert_eq!((batch.captures, batch.bytes), (1, 10));
    let kept: Vec<i64> =
        sqlx::query_scalar("SELECT seq FROM captures WHERE endpoint_id = $1 ORDER BY seq")
            .bind(&target)
            .fetch_all(db.pool())
            .await
            .expect("the sequences read");
    assert_eq!(kept, [1, 3, 4]);
    assert_eq!(counters(db.pool(), &target).await, (3, 30));
    assert!(
        store
            .endpoints_due_for_capture_ttl(&settings)
            .await
            .expect("the scan runs")
            .is_empty()
    );
}

async fn owner_storage(pool: &PgPool, user: Uuid) -> (i64, bool) {
    sqlx::query_as("SELECT bytes_used, storage_over_quota FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .expect("the user row is readable")
}

#[tokio::test]
async fn refreshing_owner_storage_flags_owners_over_their_tier_quota_and_clears_the_flag() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let heavy = insert_user(db.pool()).await;
    let light = insert_user(db.pool()).await;
    seed(db.pool(), Seed::active(10).owned_by(heavy).with_bytes(3000)).await;
    seed(db.pool(), Seed::active(10).owned_by(light).with_bytes(100)).await;
    let member = TierLimits {
        storage_bytes: Some(2000),
        ..TierLimits::member()
    };
    let settings = MaintenanceSettings {
        tiers: Tiers::default()
            .with_limits("member", member)
            .expect("the tier is valid"),
        ..settings(u64::MAX)
    };

    let first = store
        .refresh_owner_storage(&settings)
        .await
        .expect("the refresh runs");
    assert_eq!(first.over_quota, 1);
    assert_eq!(owner_storage(db.pool(), heavy).await, (3000, true));
    assert_eq!(owner_storage(db.pool(), light).await, (100, false));

    sqlx::query("UPDATE endpoints SET bytes_used = 10 WHERE owner_id = $1")
        .bind(heavy)
        .execute(db.pool())
        .await
        .expect("the usage shrinks");
    let second = store
        .refresh_owner_storage(&settings)
        .await
        .expect("the refresh runs");
    assert_eq!(second.over_quota, 0);
    assert_eq!(owner_storage(db.pool(), heavy).await, (10, false));
}

#[tokio::test]
async fn refreshing_owner_storage_writes_a_quarter_of_the_quota_as_the_endpoint_cap() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let capped = insert_user(db.pool()).await;
    let free = insert_user(db.pool()).await;
    sqlx::query("UPDATE users SET tier = 'unmetered' WHERE id = $1")
        .bind(free)
        .execute(db.pool())
        .await
        .expect("the tier is set");
    let member = TierLimits {
        storage_bytes: Some(2000),
        ..TierLimits::member()
    };
    let unmetered = TierLimits {
        storage_bytes: None,
        ..TierLimits::member()
    };
    let settings = MaintenanceSettings {
        tiers: Tiers::default()
            .with_limits("member", member)
            .and_then(|tiers| tiers.with_limits("unmetered", unmetered))
            .expect("the tiers are valid"),
        ..settings(u64::MAX)
    };

    store
        .refresh_owner_storage(&settings)
        .await
        .expect("the refresh runs");

    let cap = |user: Uuid| {
        sqlx::query_scalar::<_, Option<i64>>("SELECT endpoint_bytes_cap FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(db.pool())
    };
    assert_eq!(cap(capped).await.expect("the cap reads"), Some(500));
    assert_eq!(cap(free).await.expect("the cap reads"), Some(i64::MAX));
}

#[tokio::test]
async fn eviction_takes_an_over_quota_owners_endpoint_before_older_anonymous_ones() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    sqlx::query("UPDATE users SET storage_over_quota = true WHERE id = $1")
        .bind(owner)
        .execute(db.pool())
        .await
        .expect("the flag is set");
    let anonymous = seed(db.pool(), Seed::active(5000).with_bytes(100)).await;
    seed(db.pool(), Seed::active(10).owned_by(owner).with_bytes(100)).await;

    let batch = store
        .evict_batch(&settings(u64::MAX), 1)
        .await
        .expect("the batch runs");

    assert_eq!(batch.endpoints, 1);
    assert_eq!(endpoint_ids(db.pool()).await, BTreeSet::from([anonymous]));
}
