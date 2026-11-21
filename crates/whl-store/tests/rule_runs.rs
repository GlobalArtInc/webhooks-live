use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_rules::{Action, Condition, Rule, Step};
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::rule_runs::{
    FinishOutcome, PlanWrite, RuleRunSeed, RunStart, SyncResult, SyncState,
};
use whl_store::rules::{RuleChange, WriteOutcome};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const DEADLINE: Duration = Duration::from_secs(5);

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
                templating: Default::default(),
                cors: false,
                overflow: Overflow::Refuse,
                tags: Vec::new(),
                verifier: None,
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

fn rule(name: &str, action: Action) -> Rule {
    Rule {
        name: name.to_owned(),
        enabled: true,
        condition: Condition::Always,
        steps: vec![Step {
            name: None,
            continue_on_error: false,
            action,
        }],
    }
}

fn respond() -> Action {
    Action::Respond {
        status: "202".to_owned(),
        content_type: None,
        headers: Vec::new(),
        body: String::new(),
    }
}

fn increment() -> Action {
    Action::Increment {
        counter: "hits".to_owned(),
        by: 1,
    }
}

impl Fixture {
    async fn add_rule(&self, version: i32, rule: Rule) {
        let outcome = self
            .store
            .write_rules(
                &self.endpoint,
                RETENTION,
                version,
                &RuleChange::Create(rule),
            )
            .await
            .expect("the rule is written");
        assert!(matches!(outcome, WriteOutcome::Written(_)), "{outcome:?}");
    }

    async fn capture(&self) -> (CaptureId, Option<RuleRunSeed>) {
        self.capture_via(&self.store).await
    }

    async fn capture_via(&self, store: &Store) -> (CaptureId, Option<RuleRunSeed>) {
        let id = CaptureId::generate().expect("entropy is available");
        let outcome = store
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
                    headers: Vec::new(),
                    query: Vec::new(),
                    form: None,
                    content_type: Some("text/plain".to_owned()),
                    body: Bytes::from_static(b"hello"),
                    status_override: None,
                },
                CapturePolicy {
                    max_captures: 500,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is stored");
        match outcome {
            CaptureOutcome::Stored { capture, .. } => (id, capture.rule_run),
            other => panic!("the capture was not stored: {other:?}"),
        }
    }

    async fn run_row(&self, capture: &CaptureId) -> sqlx::postgres::PgRow {
        sqlx::query(
            "SELECT id, state, trigger, rules_version, plan::text AS plan, \
             locked_until > now() AS leased, locked_until IS NULL AS unleased, \
             error, response_status FROM rule_runs WHERE capture_id = $1",
        )
        .bind(capture.unpack())
        .fetch_one(self.db.pool())
        .await
        .expect("the run row is readable")
    }

    async fn capture_state(&self, capture: &CaptureId) -> (Option<String>, i16) {
        sqlx::query_as("SELECT rule_state, response_status FROM captures WHERE id = $1")
            .bind(capture.unpack())
            .fetch_one(self.db.pool())
            .await
            .expect("the capture row is readable")
    }
}

fn sync_result(state: SyncState, plan: PlanWrite) -> SyncResult {
    SyncResult {
        state,
        plan,
        vars: json!({"token": "abc"}),
        log: json!([{"rule_id": "r", "rule_name": "n", "matched": true}]),
        steps: json!({"fetch": {"status": 200}}),
        response_status: Some(202),
        capture_status: Some(202),
        error: None,
        event: None,
    }
}

#[tokio::test]
async fn a_capture_on_an_endpoint_with_a_response_rule_starts_a_leased_sync_run_with_the_rule_snapshot()
 {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("answer", respond())).await;

    let (id, seed) = fixture.capture().await;

    assert_eq!(
        seed,
        Some(RuleRunSeed {
            rules_version: 1,
            start: RunStart::Sync
        })
    );
    let row = fixture.run_row(&id).await;
    assert_eq!(row.get::<String, _>("state"), "sync");
    assert_eq!(row.get::<String, _>("trigger"), "capture");
    assert_eq!(row.get::<uuid::Uuid, _>("id"), id.unpack());
    assert!(row.get::<bool, _>("leased"));
    let plan: Value =
        serde_json::from_str(&row.get::<String, _>("plan")).expect("the plan is JSON");
    assert_eq!(plan["version"], 2);
    assert_eq!(plan["rules"][0]["name"], "answer");
    assert_eq!(plan["snapshots"], json!([]));
    assert_eq!(plan["resume"], json!({"rule": 0, "step": 0}));
    assert_eq!(
        fixture.capture_state(&id).await.0.as_deref(),
        Some("running")
    );
    let accounted: bool = sqlx::query_scalar(
        "SELECT c.stored_bytes > octet_length(r.plan::text) AND e.bytes_used >= c.stored_bytes \
         FROM captures c JOIN rule_runs r ON r.capture_id = c.id \
         JOIN endpoints e ON e.id = c.endpoint_id WHERE c.id = $1",
    )
    .bind(id.unpack())
    .fetch_one(fixture.db.pool())
    .await
    .expect("the byte accounting is readable");
    assert!(
        accounted,
        "the stored plan is not counted against the endpoint"
    );
}

#[tokio::test]
async fn a_rule_set_without_a_response_step_is_queued_without_a_lease() {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("count", increment())).await;

    let (id, seed) = fixture.capture().await;

    assert_eq!(
        seed,
        Some(RuleRunSeed {
            rules_version: 1,
            start: RunStart::Queued
        })
    );
    let row = fixture.run_row(&id).await;
    assert_eq!(row.get::<String, _>("state"), "queued");
    assert!(row.get::<bool, _>("unleased"));
}

#[tokio::test]
async fn an_endpoint_without_rules_stores_no_run_and_no_rule_state() {
    let fixture = fixture().await;

    let (id, seed) = fixture.capture().await;

    assert_eq!(seed, None);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs")
        .fetch_one(fixture.db.pool())
        .await
        .expect("the runs are countable");
    assert_eq!(runs, 0);
    assert_eq!(fixture.capture_state(&id).await.0, None);
}

#[tokio::test]
async fn a_second_capture_run_for_the_same_capture_is_refused_by_the_database() {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("answer", respond())).await;
    let (id, _) = fixture.capture().await;

    let duplicate = sqlx::query(
        "INSERT INTO rule_runs (id, capture_id, endpoint_id, trigger, rules_version, state) \
         VALUES (gen_random_uuid(), $1, $2, 'capture', 1, 'queued')",
    )
    .bind(id.unpack())
    .bind(fixture.endpoint.unpack())
    .execute(fixture.db.pool())
    .await;

    assert!(duplicate.is_err());
}

#[tokio::test]
async fn finishing_a_sync_run_records_the_result_on_the_run_and_the_capture_once() {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("answer", respond())).await;
    let (id, _) = fixture.capture().await;
    let plan = json!({"version": 1, "rules": [], "resume": null, "deferred": [], "payload": null});

    let first = fixture
        .store
        .finish_sync_run(
            &id,
            &sync_result(SyncState::Queued, PlanWrite::Set(plan.clone())),
            DEADLINE,
        )
        .await
        .expect("the write runs");
    let second = fixture
        .store
        .finish_sync_run(
            &id,
            &sync_result(SyncState::Failed, PlanWrite::Clear),
            DEADLINE,
        )
        .await
        .expect("the write runs");

    assert_eq!(first, FinishOutcome::Written);
    assert_eq!(second, FinishOutcome::LeaseLost);
    let row = fixture.run_row(&id).await;
    assert_eq!(row.get::<String, _>("state"), "queued");
    assert!(row.get::<bool, _>("unleased"));
    assert_eq!(row.get::<Option<i16>, _>("response_status"), Some(202));
    let stored: Value =
        serde_json::from_str(&row.get::<String, _>("plan")).expect("the plan is JSON");
    assert_eq!(stored, plan);
    let steps: Value = sqlx::query_scalar("SELECT step_results FROM rule_runs")
        .fetch_one(fixture.db.pool())
        .await
        .expect("the step results are readable");
    assert_eq!(steps, json!({"fetch": {"status": 200}}));
    let (rule_state, status) = fixture.capture_state(&id).await;
    assert_eq!(rule_state.as_deref(), Some("running"));
    assert_eq!(status, 202);
}

#[tokio::test]
async fn a_store_with_the_rules_engine_off_inserts_no_run_and_keeps_no_plan_bytes() {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("answer", respond())).await;
    let off = fixture.store.clone().with_rules_enabled(false);

    let (id, seed) = fixture.capture_via(&off).await;

    assert_eq!(seed, None);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM rule_runs")
        .fetch_one(fixture.db.pool())
        .await
        .expect("the runs are countable");
    assert_eq!(runs, 0);
    assert_eq!(fixture.capture_state(&id).await.0, None);
    let plain: bool = sqlx::query_scalar(
        "SELECT c.stored_bytes < 100 AND e.bytes_used = c.stored_bytes \
         FROM captures c JOIN endpoints e ON e.id = c.endpoint_id WHERE c.id = $1",
    )
    .bind(id.unpack())
    .fetch_one(fixture.db.pool())
    .await
    .expect("the byte accounting is readable");
    assert!(plain, "plan bytes were counted for a capture without a run");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_captures_and_result_writes_neither_block_each_other_nor_drift_the_byte_count() {
    let fixture = fixture().await;
    fixture.add_rule(0, rule("answer", respond())).await;
    let mut waiting = Vec::new();
    for _ in 0..8 {
        waiting.push(fixture.capture().await.0);
    }
    let store = fixture.store.clone();
    let endpoint = fixture.endpoint.clone();

    let mut tasks = tokio::task::JoinSet::new();
    for id in waiting {
        let store = store.clone();
        tasks.spawn(async move {
            let plan = json!({"version": 1, "rules": [], "resume": null, "snapshots": [], "deferred": [], "payload": null});
            store
                .finish_sync_run(&id, &sync_result(SyncState::Queued, PlanWrite::Set(plan)), DEADLINE)
                .await
                .map(|outcome| outcome == FinishOutcome::Written)
        });
    }
    for _ in 0..8 {
        let store = store.clone();
        let endpoint = endpoint.clone();
        tasks.spawn(async move {
            let outcome = store
                .store_capture(
                    NewCapture {
                        id: CaptureId::generate().expect("entropy is available"),
                        endpoint_id: endpoint,
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
                        body: Bytes::new(),
                        status_override: None,
                    },
                    CapturePolicy {
                        max_captures: 500,
                        retention: RETENTION,
                    },
                )
                .await;
            outcome.map(|outcome| matches!(outcome, CaptureOutcome::Stored { .. }))
        });
    }
    let finished = tokio::time::timeout(Duration::from_secs(20), async {
        let mut results = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            results.push(joined.expect("the task finishes"));
        }
        results
    })
    .await
    .expect("the writes did not block each other");

    assert!(finished.iter().all(|result| matches!(result, Ok(true))));
    let drift: i64 = sqlx::query_scalar(
        "SELECT e.bytes_used - COALESCE(sum(c.stored_bytes), 0) \
         FROM endpoints e JOIN captures c ON c.endpoint_id = e.id \
         WHERE e.id = $1 GROUP BY e.bytes_used",
    )
    .bind(fixture.endpoint.unpack())
    .fetch_one(fixture.db.pool())
    .await
    .expect("the byte accounting is readable");
    assert_eq!(drift, 0);
}
