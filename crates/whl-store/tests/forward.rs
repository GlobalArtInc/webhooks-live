use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::forward::{
    AttemptRecord, ClaimPolicy, CreateRuleOutcome, NewForwardRule, RecordOutcome, Settlement,
};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const LEASE: Duration = Duration::from_secs(60);

struct Fixture {
    db: TestDb,
    store: Store,
    endpoint: EndpointId,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let record = store
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
        .expect("the endpoint is created");
    Fixture {
        db,
        store,
        endpoint: record.id,
    }
}

impl Fixture {
    async fn rule(&self, target: &str, enabled: bool, method: Option<&str>) {
        let outcome = self
            .store
            .create_forward_rule(
                &self.endpoint,
                RETENTION,
                &NewForwardRule {
                    enabled,
                    target_url: target.to_owned(),
                    method: method.map(str::to_owned),
                    content_type: None,
                    header_allowlist: vec!["x-signature".to_owned()],
                    append_path: true,
                },
            )
            .await
            .expect("the rule is stored");
        assert!(matches!(outcome, CreateRuleOutcome::Created(_)));
    }

    async fn capture(&self) -> CaptureId {
        self.capture_of(whl_store::CaptureKind::Http).await
    }

    async fn capture_of(&self, kind: whl_store::CaptureKind) -> CaptureId {
        let id = CaptureId::generate().expect("an id is generated");
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: self.endpoint.clone(),
                    received_at: OffsetDateTime::now_utc(),
                    method: "POST".to_owned(),
                    url: "http://h/x".to_owned(),
                    path: "/x".to_owned(),
                    query_string: String::new(),
                    host: "h".to_owned(),
                    client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                    headers: vec![NameValue {
                        name: "x-signature".to_owned(),
                        value: "s".to_owned(),
                    }],
                    query: Vec::new(),
                    form: None,
                    content_type: Some("text/plain".to_owned()),
                    body: Bytes::from_static(b"hello"),
                    status_override: None,
                    kind,
                    meta: serde_json::json!({}),
                    fixed_status: None,
                },
                CapturePolicy {
                    max_captures: 500,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is stored");
        assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
        id
    }

    fn pool(&self) -> &PgPool {
        self.db.pool()
    }
}

fn policy(limit: i64) -> ClaimPolicy {
    ClaimPolicy {
        limit,
        max_attempts: 3,
        lease: LEASE,
    }
}

fn record(job_id: whl_store::Uuid, attempt: i16) -> AttemptRecord {
    AttemptRecord {
        job_id,
        attempt,
        duration_ms: 12,
        status: Some(200),
        error: None,
        final_url: Some("http://t/".to_owned()),
        response_content_type: Some("text/plain".to_owned()),
        response_body: Some(b"ok".to_vec()),
        response_truncated: false,
    }
}

#[tokio::test]
async fn an_email_capture_queues_no_forward_job() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;

    let capture = fixture.capture_of(whl_store::CaptureKind::Email).await;

    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_jobs WHERE capture_id = $1")
        .bind(capture.unpack())
        .fetch_one(fixture.pool())
        .await
        .expect("the jobs are counted");
    assert_eq!(jobs, 0);
}

#[tokio::test]
async fn a_capture_gets_one_job_per_enabled_rule_and_none_for_disabled_ones() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, Some("PUT")).await;
    fixture.rule("http://two.test/b", true, None).await;
    fixture.rule("http://off.test/c", false, None).await;
    let capture = fixture.capture().await;

    let rows: Vec<(String, String, String, i16, bool)> = sqlx::query_as(
        "SELECT target_url, method, state, attempts, append_path FROM forward_jobs \
         WHERE capture_id = $1 ORDER BY target_url",
    )
    .bind(capture.unpack())
    .fetch_all(fixture.pool())
    .await
    .expect("the jobs are readable");

    assert_eq!(
        rows,
        vec![
            (
                "http://one.test/a".to_owned(),
                "PUT".to_owned(),
                "pending".to_owned(),
                0,
                true
            ),
            (
                "http://two.test/b".to_owned(),
                "POST".to_owned(),
                "pending".to_owned(),
                0,
                true
            ),
        ]
    );
}

#[tokio::test]
async fn a_capture_without_rules_creates_no_jobs_and_no_wake_notification() {
    let fixture = fixture().await;
    let mut listener = PgListener::connect_with(fixture.pool())
        .await
        .expect("the listener connects");
    listener
        .listen(whl_store::forward::WAKE_CHANNEL)
        .await
        .expect("the listener subscribes");

    fixture.capture().await;
    let none = tokio::time::timeout(Duration::from_millis(300), listener.recv()).await;
    assert!(none.is_err(), "no wake notification without jobs");
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_jobs")
        .fetch_one(fixture.pool())
        .await
        .expect("the count is readable");
    assert_eq!(jobs, 0);

    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;
    let woken = tokio::time::timeout(Duration::from_secs(5), listener.recv()).await;
    assert!(
        woken.is_ok(),
        "a wake notification follows a capture with jobs"
    );
}

#[tokio::test]
async fn rules_of_several_endpoints_load_in_one_query_grouped_by_endpoint() {
    let fixture = fixture().await;
    let other = fixture
        .store
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
        .expect("the second endpoint is created")
        .id;
    let bare = EndpointId::generate().expect("an id is generated");
    fixture.rule("http://8.8.8.8/a", true, None).await;
    fixture.rule("http://8.8.8.8/b", false, Some("PUT")).await;
    let created = fixture
        .store
        .create_forward_rule(
            &other,
            RETENTION,
            &NewForwardRule {
                enabled: true,
                target_url: "http://8.8.8.8/c".to_owned(),
                method: None,
                content_type: None,
                header_allowlist: Vec::new(),
                append_path: false,
            },
        )
        .await
        .expect("the rule is stored");
    assert!(matches!(created, CreateRuleOutcome::Created(_)));

    let ids = [fixture.endpoint.clone(), other.clone(), bare.clone()];
    let grouped = fixture
        .store
        .list_forward_rules_for(&ids)
        .await
        .expect("the rules load");
    for id in [&fixture.endpoint, &other] {
        let single = fixture
            .store
            .list_forward_rules(id)
            .await
            .expect("the rules load");
        assert_eq!(grouped.get(id), Some(&single));
    }
    assert_eq!(grouped[&fixture.endpoint].len(), 2);
    assert!(!grouped.contains_key(&bare));
    assert!(
        fixture
            .store
            .list_forward_rules_for(&[])
            .await
            .expect("an empty list loads")
            .is_empty()
    );
}

#[tokio::test]
async fn concurrent_claims_never_return_the_same_job() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    for _ in 0..40 {
        fixture.capture().await;
    }

    let claims: Vec<_> = (0..8)
        .map(|_| {
            let store = fixture.store.clone();
            tokio::spawn(async move { store.claim_forward_jobs(policy(10)).await })
        })
        .collect();
    let mut seen = Vec::new();
    for claim in claims {
        let jobs = claim
            .await
            .expect("the claim task finishes")
            .expect("the claim succeeds");
        seen.extend(jobs.into_iter().map(|job| job.id));
    }

    assert_eq!(seen.len(), 40);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 40);
}

#[tokio::test]
async fn a_live_lease_blocks_a_claim_and_an_expired_one_is_reclaimed() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;

    let first = fixture
        .store
        .claim_forward_jobs(policy(5))
        .await
        .expect("the claim succeeds");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].attempt, 1);
    let blocked = fixture
        .store
        .claim_forward_jobs(policy(5))
        .await
        .expect("the claim succeeds");
    assert!(blocked.is_empty());

    sqlx::query("UPDATE forward_jobs SET locked_until = now() - interval '1 second'")
        .execute(fixture.pool())
        .await
        .expect("the lease is expired");
    let second = fixture
        .store
        .claim_forward_jobs(policy(5))
        .await
        .expect("the claim succeeds");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].id, first[0].id);
    assert_eq!(second[0].attempt, 2);
}

#[tokio::test]
async fn a_result_from_a_superseded_claim_is_discarded() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;
    let stale = fixture
        .store
        .claim_forward_jobs(policy(1))
        .await
        .expect("the claim succeeds")
        .remove(0);
    sqlx::query("UPDATE forward_jobs SET locked_until = now() - interval '1 second'")
        .execute(fixture.pool())
        .await
        .expect("the lease is expired");
    let fresh = fixture
        .store
        .claim_forward_jobs(policy(1))
        .await
        .expect("the claim succeeds")
        .remove(0);

    let late = fixture
        .store
        .record_forward_attempt(&record(stale.id, stale.attempt), Settlement::Succeeded)
        .await
        .expect("the record call succeeds");
    assert_eq!(late, RecordOutcome::Superseded);
    let current = fixture
        .store
        .record_forward_attempt(&record(fresh.id, fresh.attempt), Settlement::Succeeded)
        .await
        .expect("the record call succeeds");
    assert_eq!(current, RecordOutcome::Recorded);

    let attempts: Vec<i16> = sqlx::query_scalar("SELECT attempt FROM forward_attempts")
        .fetch_all(fixture.pool())
        .await
        .expect("the attempts are readable");
    assert_eq!(attempts, vec![2]);
}

#[tokio::test]
async fn a_retry_settlement_schedules_the_next_attempt_and_releases_the_lease() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;
    let job = fixture
        .store
        .claim_forward_jobs(policy(1))
        .await
        .expect("the claim succeeds")
        .remove(0);

    fixture
        .store
        .record_forward_attempt(
            &record(job.id, job.attempt),
            Settlement::RetryIn(Duration::from_secs(300)),
        )
        .await
        .expect("the record call succeeds");

    let (state, locked, wait): (String, bool, f64) = sqlx::query_as(
        "SELECT state, locked_until IS NOT NULL, \
         extract(epoch FROM next_attempt_at - now())::float8 FROM forward_jobs",
    )
    .fetch_one(fixture.pool())
    .await
    .expect("the job is readable");
    assert_eq!(state, "pending");
    assert!(!locked);
    assert!(wait > 290.0 && wait <= 300.0);
    let none = fixture
        .store
        .claim_forward_jobs(policy(1))
        .await
        .expect("the claim succeeds");
    assert!(none.is_empty());
}

#[tokio::test]
async fn jobs_of_a_deleted_endpoint_are_not_claimed() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;
    assert!(
        fixture
            .store
            .delete_endpoint(&fixture.endpoint, RETENTION)
            .await
            .expect("the endpoint is deleted")
    );

    let claimed = fixture
        .store
        .claim_forward_jobs(policy(5))
        .await
        .expect("the claim succeeds");
    assert!(claimed.is_empty());
}

#[tokio::test]
async fn exhausted_jobs_with_a_lost_lease_fail_with_a_recorded_attempt() {
    let fixture = fixture().await;
    fixture.rule("http://one.test/a", true, None).await;
    fixture.capture().await;
    sqlx::query(
        "UPDATE forward_jobs SET state = 'running', attempts = 3, \
         locked_until = now() - interval '1 second'",
    )
    .execute(fixture.pool())
    .await
    .expect("the crashed state is simulated");

    assert_eq!(
        fixture
            .store
            .fail_exhausted_forward_jobs(3)
            .await
            .expect("the reap succeeds"),
        1
    );

    let (state, error): (String, String) = sqlx::query_as(
        "SELECT j.state, a.error FROM forward_jobs j JOIN forward_attempts a ON a.job_id = j.id",
    )
    .fetch_one(fixture.pool())
    .await
    .expect("the job and attempt are readable");
    assert_eq!(state, "failed");
    assert!(error.starts_with("timeout: "));
}
