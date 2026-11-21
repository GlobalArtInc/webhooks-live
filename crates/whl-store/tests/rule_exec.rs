use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_rules::{Action, Condition, Rule, Step};
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::rule_exec::{
    ClaimedFrom, ClaimedRun, Disposition, EnqueueOutcome, FinalState, ForwardStepJob,
    MAX_LISTED_RUNS, RerunOutcome, RunClaimPolicy, RunFilter, RunSave, SaveOutcome, StepDelivery,
};
use whl_store::rules::{RuleChange, WriteOutcome};
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

fn after_rule() -> Rule {
    Rule {
        name: "after".to_owned(),
        enabled: true,
        condition: Condition::Always,
        steps: vec![Step {
            name: None,
            continue_on_error: false,
            action: Action::SetVar {
                into: "a".to_owned(),
                value: "1".to_owned(),
            },
        }],
    }
}

fn respond_rule() -> Rule {
    Rule {
        name: "answer".to_owned(),
        enabled: true,
        condition: Condition::Always,
        steps: vec![Step {
            name: None,
            continue_on_error: false,
            action: Action::Respond {
                status: "202".to_owned(),
                content_type: None,
                headers: Vec::new(),
                body: String::new(),
            },
        }],
    }
}

fn header(name: &str, value: &str) -> NameValue {
    NameValue {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn policy(per_endpoint: i32) -> RunClaimPolicy {
    RunClaimPolicy {
        limit: 10,
        per_endpoint,
        max_claims: 100,
        lease: LEASE,
    }
}

impl Fixture {
    async fn add_rule(&self, rule: Rule) {
        let version: i32 = sqlx::query_scalar("SELECT rules_version FROM endpoints WHERE id = $1")
            .bind(self.endpoint.unpack())
            .fetch_one(self.db.pool())
            .await
            .expect("the version is readable");
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

    async fn capture_with(&self, headers: Vec<NameValue>) -> CaptureId {
        let id = CaptureId::generate().expect("entropy is available");
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
                    headers,
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
        assert!(
            matches!(outcome, CaptureOutcome::Stored { .. }),
            "the capture was not stored: {outcome:?}"
        );
        id
    }

    async fn capture(&self) -> CaptureId {
        self.capture_with(Vec::new()).await
    }

    async fn claim(&self, per_endpoint: i32) -> Vec<ClaimedRun> {
        self.store
            .claim_rule_runs(policy(per_endpoint))
            .await
            .expect("the claim runs")
    }

    async fn execute(&self, sql: &str, id: Uuid) {
        sqlx::query(sql)
            .bind(id)
            .execute(self.db.pool())
            .await
            .expect("the statement runs");
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .expect("the count is readable")
    }

    async fn state_of(&self, run: Uuid) -> (String, i16, bool) {
        sqlx::query_as(
            "SELECT state, step_attempts, locked_until IS NULL FROM rule_runs WHERE id = $1",
        )
        .bind(run)
        .fetch_one(self.db.pool())
        .await
        .expect("the run is readable")
    }
}

fn save(run: &ClaimedRun, position: i32, disposition: Disposition) -> RunSave {
    RunSave {
        run: run.id,
        claim: run.claim,
        position,
        vars: json!({"token": "abc"}),
        log: json!([]),
        steps: json!({}),
        disposition,
    }
}

#[tokio::test]
async fn a_due_queued_run_is_claimed_once_and_leased() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;

    let claimed = fixture.claim(2).await;

    let [run] = claimed.as_slice() else {
        panic!("one run is due");
    };
    assert_eq!(run.capture_id, capture.unpack());
    assert_eq!(run.previous, ClaimedFrom::Queued);
    assert_eq!(run.claim, 1);
    assert!(run.plan.get("rules").is_some_and(Value::is_array));
    assert_eq!(fixture.state_of(run.id).await.0, "running");
    assert!(fixture.claim(2).await.is_empty());
}

#[tokio::test]
async fn an_expired_sync_lease_is_claimed_and_a_live_one_is_left_alone() {
    let fixture = fixture().await;
    fixture.add_rule(respond_rule()).await;
    let capture = fixture.capture().await;
    assert!(fixture.claim(2).await.is_empty());

    fixture
        .execute(
            "UPDATE rule_runs SET locked_until = now() - interval '1 second' WHERE id = $1",
            capture.unpack(),
        )
        .await;
    let claimed = fixture.claim(2).await;

    let [run] = claimed.as_slice() else {
        panic!("the expired sync run is claimed");
    };
    assert_eq!(run.previous, ClaimedFrom::Sync);
}

#[tokio::test]
async fn an_expired_running_lease_is_reclaimed_at_its_saved_position() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    fixture.capture().await;
    let first = fixture.claim(2).await.remove(0);
    let saved = fixture
        .store
        .save_rule_run(&save(&first, 2, Disposition::Continue { lease: LEASE }))
        .await
        .expect("the progress is saved");
    assert_eq!(saved, SaveOutcome::Saved);
    fixture
        .execute(
            "UPDATE rule_runs SET locked_until = now() - interval '1 second' WHERE id = $1",
            first.id,
        )
        .await;

    let again = fixture.claim(2).await.remove(0);

    assert_eq!(again.previous, ClaimedFrom::Running);
    assert_eq!(again.next_step, 2);
    assert_eq!(again.claim, 2);
    assert_eq!(again.vars, json!({"token": "abc"}));
}

#[tokio::test]
async fn at_most_the_configured_runs_of_one_endpoint_are_claimed_at_once() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    for _ in 0..3 {
        fixture.capture().await;
    }

    let first = fixture.claim(2).await;
    let blocked = fixture.claim(2).await;
    fixture
        .store
        .save_rule_run(&save(
            &first[0],
            1,
            Disposition::Finish {
                state: FinalState::Succeeded,
                error: None,
                event: None,
            },
        ))
        .await
        .expect("the run is finished");
    let freed = fixture.claim(2).await;

    assert_eq!(first.len(), 2);
    assert!(blocked.is_empty());
    assert_eq!(freed.len(), 1);
}

#[tokio::test]
async fn saving_progress_renews_the_lease_and_a_stale_claim_loses() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    fixture.capture().await;
    let run = fixture.claim(2).await.remove(0);

    let saved = fixture
        .store
        .save_rule_run(&save(&run, 1, Disposition::Continue { lease: LEASE }))
        .await
        .expect("the progress is saved");
    let mut stale = save(&run, 2, Disposition::Continue { lease: LEASE });
    stale.claim += 1;
    let lost = fixture
        .store
        .save_rule_run(&stale)
        .await
        .expect("the stale save is answered");

    assert_eq!(saved, SaveOutcome::Saved);
    assert_eq!(lost, SaveOutcome::LeaseLost);
    let next: i16 = sqlx::query_scalar("SELECT next_step FROM rule_runs WHERE id = $1")
        .bind(run.id)
        .fetch_one(fixture.db.pool())
        .await
        .expect("the position is readable");
    assert_eq!(next, 1);
}

#[tokio::test]
async fn a_retry_parks_the_run_until_it_is_due_and_keeps_the_attempt_count() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    fixture.capture().await;
    let run = fixture.claim(2).await.remove(0);

    fixture
        .store
        .save_rule_run(&save(
            &run,
            0,
            Disposition::Retry {
                after: Duration::from_secs(60),
                step_attempts: 1,
            },
        ))
        .await
        .expect("the retry is saved");

    assert_eq!(
        fixture.state_of(run.id).await,
        ("queued".to_owned(), 1, true)
    );
    assert!(fixture.claim(2).await.is_empty());
    fixture
        .execute(
            "UPDATE rule_runs SET next_attempt_at = now() - interval '1 second' WHERE id = $1",
            run.id,
        )
        .await;
    let due = fixture.claim(2).await.remove(0);
    assert_eq!(due.step_attempts, 1);
}

#[tokio::test]
async fn finishing_marks_the_capture_and_keeps_the_byte_accounts_exact() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    let run = fixture.claim(2).await.remove(0);
    let mut finish = save(
        &run,
        1,
        Disposition::Finish {
            state: FinalState::Failed,
            error: Some("http: refused".to_owned()),
            event: None,
        },
    );
    finish.log = json!([{"rule_id": run.id, "rule_name": "after", "matched": true}]);
    finish.steps = json!({"fetch": {"status": 200, "headers": [], "body": "x".repeat(2000), "duration_ms": 1, "error": null}});

    fixture
        .store
        .save_rule_run(&finish)
        .await
        .expect("the run is finished");

    let state: Option<String> = sqlx::query_scalar("SELECT rule_state FROM captures WHERE id = $1")
        .bind(capture.unpack())
        .fetch_one(fixture.db.pool())
        .await
        .expect("the capture is readable");
    assert_eq!(state.as_deref(), Some("failed"));
    let (state, _, unleased) = fixture.state_of(run.id).await;
    assert_eq!((state.as_str(), unleased), ("failed", true));
    let drift = fixture
        .count(
            "SELECT (SELECT bytes_used FROM endpoints LIMIT 1) \
             - (SELECT coalesce(sum(stored_bytes), 0) FROM captures)",
        )
        .await;
    assert_eq!(drift, 0);
}

#[tokio::test]
async fn a_run_that_spent_its_claims_is_failed_and_its_capture_marked() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    let run = fixture.claim(2).await.remove(0);
    fixture
        .execute(
            "UPDATE rule_runs SET attempts = 100, locked_until = now() - interval '1 second' WHERE id = $1",
            run.id,
        )
        .await;

    let failed = fixture
        .store
        .fail_exhausted_rule_runs(100)
        .await
        .expect("the reaper runs");

    assert_eq!(failed, 1);
    assert_eq!(fixture.state_of(run.id).await.0, "failed");
    let state: Option<String> = sqlx::query_scalar("SELECT rule_state FROM captures WHERE id = $1")
        .bind(capture.unpack())
        .fetch_one(fixture.db.pool())
        .await
        .expect("the capture is readable");
    assert_eq!(state.as_deref(), Some("failed"));
}

#[tokio::test]
async fn a_forward_step_creates_one_job_per_step_id_and_refuses_a_foreign_rule() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    let job = ForwardStepJob {
        id: Uuid::now_v7(),
        endpoint: fixture.endpoint.clone(),
        capture: capture.unpack(),
        rule: None,
        target_url: Some("https://hooks.example.com/in".to_owned()),
        method: Some("PUT".to_owned()),
        content_type: None,
        header_allowlist: vec!["x-token".to_owned()],
        payload: Some(b"shaped".to_vec()),
    };

    let first = fixture.store.enqueue_step_forward(&job).await;
    let second = fixture.store.enqueue_step_forward(&job).await;
    let foreign = fixture
        .store
        .enqueue_step_forward(&ForwardStepJob {
            id: Uuid::now_v7(),
            rule: Some(Uuid::now_v7()),
            target_url: None,
            ..job.clone()
        })
        .await;

    assert_eq!(first.expect("the first enqueue"), EnqueueOutcome::Queued);
    assert_eq!(second.expect("the repeat"), EnqueueOutcome::Queued);
    assert_eq!(
        foreign.expect("the foreign rule"),
        EnqueueOutcome::RuleNotFound
    );
    assert_eq!(fixture.count("SELECT count(*) FROM forward_jobs").await, 1);
    let (method, payload): (String, Vec<u8>) =
        sqlx::query_as("SELECT method, payload FROM forward_jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(fixture.db.pool())
            .await
            .expect("the job is readable");
    assert_eq!(
        (method.as_str(), payload.as_slice()),
        ("PUT", b"shaped".as_slice())
    );
}

#[tokio::test]
async fn a_notification_step_needs_an_enabled_channel_of_the_same_endpoint() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    let channel = Uuid::now_v7();
    let insert = |id: Uuid, endpoint: String, enabled: bool| {
        let pool = fixture.db.pool().clone();
        async move {
            sqlx::query(
                "INSERT INTO notification_channels \
                 (id, endpoint_id, kind, name, enabled, trigger, config, per_hour) \
                 VALUES ($1, $2, 'webhook', 'hook', $3, 'rule', '{}'::jsonb, 30)",
            )
            .bind(id)
            .bind(endpoint)
            .bind(enabled)
            .execute(&pool)
            .await
            .expect("the channel is inserted");
        }
    };
    insert(channel, fixture.endpoint.unpack().to_owned(), true).await;
    let disabled = Uuid::now_v7();
    insert(disabled, fixture.endpoint.unpack().to_owned(), false).await;
    let delivery = |channel: Uuid| StepDelivery {
        id: Uuid::now_v7(),
        endpoint: fixture.endpoint.clone(),
        capture: capture.unpack(),
        channel,
        message: Some("Order 42".to_owned()),
    };
    let queued = delivery(channel);

    let first = fixture.store.enqueue_step_delivery(&queued).await;
    let repeat = fixture.store.enqueue_step_delivery(&queued).await;
    let unknown = fixture
        .store
        .enqueue_step_delivery(&delivery(Uuid::now_v7()))
        .await;
    let off = fixture
        .store
        .enqueue_step_delivery(&delivery(disabled))
        .await;

    assert_eq!(first.expect("the first delivery"), EnqueueOutcome::Queued);
    assert_eq!(repeat.expect("the repeat"), EnqueueOutcome::Queued);
    assert_eq!(
        unknown.expect("the unknown channel"),
        EnqueueOutcome::ChannelNotFound
    );
    assert_eq!(
        off.expect("the disabled channel"),
        EnqueueOutcome::ChannelDisabled
    );
    assert_eq!(
        fixture
            .count("SELECT count(*) FROM notification_deliveries WHERE message = 'Order 42'")
            .await,
        1
    );
}

#[tokio::test]
async fn a_rerun_needs_enabled_rules_and_is_refused_while_another_is_active() {
    let fixture = fixture().await;
    let capture = fixture.capture().await;
    let create = |id: Uuid| {
        fixture
            .store
            .create_rule_rerun(&fixture.endpoint, RETENTION, capture.unpack(), id)
    };
    assert_eq!(
        create(Uuid::now_v7()).await.expect("no rules"),
        RerunOutcome::NoRules
    );
    fixture.add_rule(after_rule()).await;

    let first = create(Uuid::now_v7()).await.expect("the first rerun");
    let second = create(Uuid::now_v7()).await.expect("the second rerun");

    let RerunOutcome::Created(record) = first else {
        panic!("the first rerun is created");
    };
    assert_eq!(
        (record.trigger.as_str(), record.state.as_str()),
        ("rerun", "queued")
    );
    assert_eq!(second, RerunOutcome::Active);
    fixture
        .execute(
            "UPDATE rule_runs SET state = 'succeeded' WHERE id = $1",
            record.id,
        )
        .await;
    assert!(matches!(
        create(Uuid::now_v7())
            .await
            .expect("a rerun after the first"),
        RerunOutcome::Created(_)
    ));
}

#[tokio::test]
async fn a_capture_at_the_hop_limit_starts_no_run_and_no_forward_job() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    sqlx::query(
        "INSERT INTO forward_rules (id, endpoint_id, position, target_url) \
         VALUES ($1, $2, 0, 'https://hooks.example.com/in')",
    )
    .bind(Uuid::now_v7())
    .bind(fixture.endpoint.unpack())
    .execute(fixture.db.pool())
    .await
    .expect("the forward rule is inserted");

    let blocked = fixture
        .capture_with(vec![header("X-Webhooks-Live-Hop", "3")])
        .await;

    assert_eq!(fixture.count("SELECT count(*) FROM rule_runs").await, 0);
    assert_eq!(fixture.count("SELECT count(*) FROM forward_jobs").await, 0);
    let state: Option<String> = sqlx::query_scalar("SELECT rule_state FROM captures WHERE id = $1")
        .bind(blocked.unpack())
        .fetch_one(fixture.db.pool())
        .await
        .expect("the capture is readable");
    assert_eq!(state, None);

    fixture
        .capture_with(vec![header("x-webhooks-live-hop", "2")])
        .await;

    assert_eq!(fixture.count("SELECT count(*) FROM rule_runs").await, 1);
    assert_eq!(fixture.count("SELECT count(*) FROM forward_jobs").await, 1);
}

#[tokio::test]
async fn runs_list_newest_first_with_a_cursor_and_a_capture_filter() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let first = fixture.capture().await;
    fixture.capture().await;
    let newest = fixture.capture().await;

    let page = fixture
        .store
        .list_rule_runs(&fixture.endpoint, &RunFilter::default(), 2)
        .await
        .expect("the page is listed");
    let rest = fixture
        .store
        .list_rule_runs(
            &fixture.endpoint,
            &RunFilter {
                after: page.last().map(|record| record.id),
                ..RunFilter::default()
            },
            2,
        )
        .await
        .expect("the rest is listed");
    let one = fixture
        .store
        .list_rule_runs(
            &fixture.endpoint,
            &RunFilter {
                capture: Some(first.unpack()),
                ..RunFilter::default()
            },
            10,
        )
        .await
        .expect("the filtered runs are listed");

    assert_eq!(page.len(), 2);
    assert_eq!(page[0].capture_id, newest.unpack());
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].capture_id, first.unpack());
    assert_eq!(one.len(), 1);
}

#[tokio::test]
async fn a_disabled_forward_rule_is_refused_like_a_missing_one() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    let forward_rule = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO forward_rules (id, endpoint_id, position, enabled, target_url) \
         VALUES ($1, $2, 0, false, 'https://hooks.example.com/in')",
    )
    .bind(forward_rule)
    .bind(fixture.endpoint.unpack())
    .execute(fixture.db.pool())
    .await
    .expect("the forward rule is inserted");

    let outcome = fixture
        .store
        .enqueue_step_forward(&ForwardStepJob {
            id: Uuid::now_v7(),
            endpoint: fixture.endpoint.clone(),
            capture: capture.unpack(),
            rule: Some(forward_rule),
            target_url: None,
            method: None,
            content_type: None,
            header_allowlist: Vec::new(),
            payload: None,
        })
        .await
        .expect("the enqueue runs");

    assert_eq!(outcome, EnqueueOutcome::RuleNotFound);
    assert_eq!(fixture.count("SELECT count(*) FROM forward_jobs").await, 0);
}

#[tokio::test]
async fn the_run_list_reports_one_row_beyond_the_page_cap_and_knows_its_own_cursors() {
    let fixture = fixture().await;
    fixture.add_rule(after_rule()).await;
    let capture = fixture.capture().await;
    sqlx::query(
        "INSERT INTO rule_runs (id, capture_id, endpoint_id, trigger, rules_version, state) \
         SELECT gen_random_uuid(), $1, $2, 'rerun', 1, 'succeeded' FROM generate_series(1, 201)",
    )
    .bind(capture.unpack())
    .bind(fixture.endpoint.unpack())
    .execute(fixture.db.pool())
    .await
    .expect("the runs are inserted");

    let page = fixture
        .store
        .list_rule_runs(
            &fixture.endpoint,
            &RunFilter::default(),
            MAX_LISTED_RUNS + 1,
        )
        .await
        .expect("the page is listed");
    let known = fixture
        .store
        .rule_run_in_endpoint(&fixture.endpoint, page[0].id)
        .await
        .expect("the cursor is checked");
    let unknown = fixture
        .store
        .rule_run_in_endpoint(&fixture.endpoint, Uuid::now_v7())
        .await
        .expect("the cursor is checked");

    assert_eq!(
        page.len(),
        usize::try_from(MAX_LISTED_RUNS + 1).unwrap_or(0)
    );
    assert!(known);
    assert!(!unknown);
}
