mod support;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::postgres::{PgListener, PgPoolOptions};
use support::queue::{
    HoldingServer, fixture, run, unrestricted_worker, wait_until, worker_config, worker_for,
};
use support::{Reply, TestServer};
use whl_forward::StaticLookup;
use whl_store::forward::{ClaimPolicy, JobState};

#[tokio::test]
async fn a_successful_attempt_is_recorded_and_the_job_succeeds() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("received")).await;
    fixture.rule(&server.url("/hook"), &["X-Signature"]).await;
    let capture = fixture
        .capture(
            &[
                ("X-Signature", "sig-1"),
                ("Authorization", "Bearer secret"),
                ("Cookie", "session=1"),
            ],
            b"hello",
        )
        .await;
    let running = run(worker_for(&fixture.store, worker_config(2, 3)));

    wait_until("the job to succeed", || async {
        fixture.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].target, "/hook");
    assert_eq!(seen[0].body, b"hello");
    assert_eq!(seen[0].header("x-signature"), Some("sig-1"));
    assert_eq!(seen[0].header("authorization"), None);
    assert_eq!(seen[0].header("cookie"), None);
    assert_eq!(seen[0].header("content-type"), Some("text/plain"));
    assert_eq!(
        seen[0].header("x-webhooks-live-capture-id"),
        Some(capture.to_string().as_str())
    );

    let jobs = fixture
        .store
        .forward_jobs_for_capture(capture.unpack())
        .await
        .expect("the jobs are readable");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JobState::Succeeded);
    assert!(jobs[0].finished_at.is_some());
    let attempt = &jobs[0].attempts[0];
    assert_eq!(attempt.attempt, 1);
    assert_eq!(attempt.status, Some(200));
    assert_eq!(attempt.error, None);
    assert_eq!(attempt.response_content_type.as_deref(), Some("text/plain"));
    assert_eq!(attempt.response_body.as_deref(), Some(&b"received"[..]));
    assert!(!attempt.response_truncated);
    assert_eq!(
        attempt.final_url.as_deref(),
        Some(server.url("/hook").as_str())
    );
}

#[tokio::test]
async fn server_errors_are_retried_with_backoff_until_the_attempts_run_out() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Respond {
        status: 500,
        headers: Vec::new(),
        body: b"boom".to_vec(),
        delay: Duration::ZERO,
    })
    .await;
    fixture.rule(&server.url("/"), &[]).await;
    let capture = fixture.capture(&[], b"x").await;
    let started = Instant::now();
    let running = run(worker_for(&fixture.store, worker_config(2, 3)));

    wait_until("the job to fail", || async {
        fixture.count_in_state("failed").await == 1
    })
    .await;
    let elapsed = started.elapsed();
    running.stop().await;

    assert!(
        elapsed >= Duration::from_millis(140),
        "backoff was skipped: {elapsed:?}"
    );
    assert_eq!(server.seen().len(), 3);
    let jobs = fixture
        .store
        .forward_jobs_for_capture(capture.unpack())
        .await
        .expect("the jobs are readable");
    let attempts: Vec<(i16, Option<i16>)> = jobs[0]
        .attempts
        .iter()
        .map(|attempt| (attempt.attempt, attempt.status))
        .collect();
    assert_eq!(
        attempts,
        vec![(1, Some(500)), (2, Some(500)), (3, Some(500))]
    );
    assert_eq!(
        jobs[0].attempts[0].response_body.as_deref(),
        Some(&b"boom"[..])
    );
}

#[tokio::test]
async fn a_client_error_is_final_and_not_retried() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Respond {
        status: 404,
        headers: Vec::new(),
        body: Vec::new(),
        delay: Duration::ZERO,
    })
    .await;
    fixture.rule(&server.url("/"), &[]).await;
    fixture.capture(&[], b"x").await;
    let running = run(worker_for(&fixture.store, worker_config(2, 3)));

    wait_until("the job to fail", || async {
        fixture.count_in_state("failed").await == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    running.stop().await;

    assert_eq!(server.seen().len(), 1);
}

#[tokio::test]
async fn a_target_that_resolves_to_a_refused_address_fails_at_once_without_a_connection() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("never")).await;
    let loopback: IpAddr = "127.0.0.1".parse().expect("a valid address");
    let lookup = StaticLookup::new().with_answer("rebound.test", &[loopback]);
    let port = server.addr.port();
    fixture
        .rule(&format!("http://rebound.test:{port}/"), &[])
        .await;
    fixture.capture(&[], b"x").await;
    let running = run(unrestricted_worker(
        &fixture.store,
        worker_config(2, 3),
        lookup,
    ));

    wait_until("the job to fail", || async {
        fixture.count_in_state("failed").await == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    running.stop().await;

    assert_eq!(server.connections(), 0);
    let job = fixture.job_ids().await[0];
    assert_eq!(fixture.attempt_count(job).await, 1);
    let (status, error): (Option<i16>, Option<String>) =
        sqlx::query_as("SELECT status, error FROM forward_attempts WHERE job_id = $1")
            .bind(job)
            .fetch_one(fixture.pool())
            .await
            .expect("the attempt is readable");
    assert_eq!(status, None);
    assert_eq!(error.as_deref(), Some("blocked_address: refused_address"));
}

#[tokio::test]
async fn a_literal_metadata_address_is_refused_without_a_connection() {
    let fixture = fixture().await;
    fixture
        .rule("http://169.254.169.254/latest/meta-data", &[])
        .await;
    fixture.capture(&[], b"x").await;
    let running = run(unrestricted_worker(
        &fixture.store,
        worker_config(2, 3),
        StaticLookup::new(),
    ));

    wait_until("the job to fail", || async {
        fixture.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    let job = fixture.job_ids().await[0];
    let error: Option<String> =
        sqlx::query_scalar("SELECT error FROM forward_attempts WHERE job_id = $1")
            .bind(job)
            .fetch_one(fixture.pool())
            .await
            .expect("the attempt is readable");
    assert_eq!(error.as_deref(), Some("blocked_address: refused_address"));
}

#[tokio::test]
async fn a_job_whose_lease_expired_after_a_crash_is_picked_up_exactly_once() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("ok")).await;
    fixture.rule(&server.url("/"), &[]).await;
    fixture.capture(&[], b"x").await;
    let crashed = fixture
        .store
        .claim_forward_jobs(ClaimPolicy {
            limit: 5,
            max_attempts: 3,
            lease: Duration::from_secs(60),
        })
        .await
        .expect("the simulated crashed claim succeeds");
    assert_eq!(crashed.len(), 1);

    let running = run(worker_for(&fixture.store, worker_config(2, 3)));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.seen().len(),
        0,
        "a live lease must not be taken over"
    );

    sqlx::query("UPDATE forward_jobs SET locked_until = now() - interval '1 second'")
        .execute(fixture.pool())
        .await
        .expect("the lease is expired");
    wait_until("the job to succeed", || async {
        fixture.count_in_state("succeeded").await == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    running.stop().await;

    assert_eq!(server.seen().len(), 1);
    let attempts: Vec<i16> = sqlx::query_scalar("SELECT attempt FROM forward_attempts")
        .fetch_all(fixture.pool())
        .await
        .expect("the attempts are readable");
    assert_eq!(attempts, vec![2]);
}

#[tokio::test]
async fn a_job_with_no_attempts_left_and_a_lost_lease_is_failed_by_the_worker() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("ok")).await;
    fixture.rule(&server.url("/"), &[]).await;
    fixture.capture(&[], b"x").await;
    sqlx::query(
        "UPDATE forward_jobs SET state = 'running', attempts = 3, \
         locked_until = now() - interval '1 second'",
    )
    .execute(fixture.pool())
    .await
    .expect("the crashed state is simulated");
    let running = run(worker_for(&fixture.store, worker_config(2, 3)));

    wait_until("the job to fail", || async {
        fixture.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(server.seen().len(), 0);
}

#[tokio::test]
async fn two_workers_on_one_database_never_send_the_same_job_twice() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Respond {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
        delay: Duration::from_millis(15),
    })
    .await;
    fixture.rule(&server.url("/"), &[]).await;
    let total = 40;
    for _ in 0..total {
        fixture.capture(&[], b"x").await;
    }
    let other_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(fixture.db.url())
        .await
        .expect("the second pool connects");
    let other_store = whl_store::Store::from_pool(other_pool);
    let first = run(worker_for(&fixture.store, worker_config(4, 3)));
    let second = run(worker_for(&other_store, worker_config(4, 3)));

    wait_until("every job to succeed", || async {
        fixture.count_in_state("succeeded").await == total
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    first.stop().await;
    second.stop().await;

    let mut per_capture: HashMap<String, usize> = HashMap::new();
    for request in server.seen() {
        let id = request
            .header("x-webhooks-live-capture-id")
            .expect("every forward names its capture")
            .to_owned();
        *per_capture.entry(id).or_default() += 1;
    }
    assert_eq!(per_capture.len(), 40);
    assert!(per_capture.values().all(|count| *count == 1));
    assert_eq!(server.seen().len(), 40);
}

#[tokio::test]
async fn the_number_of_attempts_in_flight_never_exceeds_the_concurrency_limit() {
    let fixture = fixture().await;
    let server = HoldingServer::start(Duration::from_millis(250)).await;
    fixture.rule(&server.url(), &[]).await;
    for _ in 0..9 {
        fixture.capture(&[], b"x").await;
    }
    let running = run(worker_for(&fixture.store, worker_config(3, 3)));

    wait_until("every job to succeed", || async {
        fixture.count_in_state("succeeded").await == 9
    })
    .await;
    running.stop().await;

    assert_eq!(server.total(), 9);
    assert_eq!(server.peak(), 3);
}

#[tokio::test]
async fn a_manual_job_runs_like_a_rule_job() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::ok("again")).await;
    let capture = fixture.capture(&[("X-Signature", "s")], b"payload").await;
    let outcome = fixture
        .store
        .create_manual_job(
            &fixture.endpoint,
            support::queue::RETENTION,
            capture.unpack(),
            &whl_store::forward::ManualTarget::Custom(whl_store::forward::CustomTarget {
                target_url: server.url("/resend"),
                method: Some("PUT".to_owned()),
                content_type: Some("application/x-test".to_owned()),
                header_allowlist: Vec::new(),
            }),
        )
        .await
        .expect("the manual job is created");
    let whl_store::forward::ManualJobOutcome::Created(job) = outcome else {
        panic!("the manual job was not created: {outcome:?}");
    };
    assert_eq!(job.state, JobState::Pending);
    let running = run(worker_for(&fixture.store, worker_config(2, 3)));

    wait_until("the manual job to succeed", || async {
        fixture.state_of(job.id).await == "succeeded"
    })
    .await;
    running.stop().await;

    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "PUT");
    assert_eq!(seen[0].target, "/resend");
    assert_eq!(seen[0].header("content-type"), Some("application/x-test"));
    assert_eq!(seen[0].header("x-signature"), None);
}

#[tokio::test]
async fn every_finished_attempt_announces_a_thin_forward_result() {
    let fixture = fixture().await;
    let server = TestServer::start("127.0.0.1:0", |_| Reply::Respond {
        status: 503,
        headers: Vec::new(),
        body: Vec::new(),
        delay: Duration::ZERO,
    })
    .await;
    fixture.rule(&server.url("/"), &[]).await;
    let capture = fixture.capture(&[], b"x").await;
    let job = fixture.job_ids().await[0];
    let mut listener = PgListener::connect(fixture.db.url())
        .await
        .expect("the listener connects");
    listener
        .listen("whl_events")
        .await
        .expect("the listener subscribes");
    while tokio::time::timeout(Duration::from_millis(200), listener.recv())
        .await
        .is_ok()
    {}
    let running = run(worker_for(&fixture.store, worker_config(2, 2)));

    let mut payloads = Vec::new();
    while payloads.len() < 2 {
        let notification = tokio::time::timeout(Duration::from_secs(10), listener.recv())
            .await
            .expect("a notification arrives")
            .expect("the listener stays connected");
        payloads.push(notification.payload().to_owned());
    }
    running.stop().await;

    let expected = serde_json::json!({
        "t": "forward_result",
        "e": fixture.endpoint.unpack(),
        "d": {
            "capture_id": capture.to_string(),
            "job_id": job.hyphenated().to_string(),
        },
    });
    for payload in payloads {
        assert!(payload.len() < 400);
        let parsed: serde_json::Value =
            serde_json::from_str(&payload).expect("the payload is JSON");
        assert_eq!(parsed, expected);
    }
}

#[tokio::test]
async fn shutdown_stops_claiming_and_lets_the_attempt_in_flight_finish() {
    let fixture = fixture().await;
    let server = HoldingServer::start(Duration::from_millis(500)).await;
    fixture.rule(&server.url(), &[]).await;
    fixture.capture(&[], b"first").await;
    fixture.capture(&[], b"second").await;
    let mut running = run(worker_for(&fixture.store, worker_config(1, 3)));

    wait_until("the first attempt to be in flight", || async {
        server.in_flight() == 1
    })
    .await;
    running.trigger_stop();
    running.join().await;

    assert_eq!(fixture.count_in_state("succeeded").await, 1);
    assert_eq!(fixture.count_in_state("running").await, 0);
    assert_eq!(fixture.count_in_state("pending").await, 1);
    assert_eq!(server.total(), 1);
}

#[tokio::test]
async fn a_worker_refuses_a_lease_shorter_than_the_attempt_timeout() {
    let fixture = fixture().await;
    let mut config = worker_config(2, 3);
    config.lease = Duration::from_secs(5);
    let client = whl_forward::GuardedClient::with_lookup(
        support::loopback_config(),
        Arc::new(StaticLookup::new()),
    )
    .expect("the client is configured");
    let error = whl_forward::ForwardWorker::new(fixture.store.clone(), client, config)
        .expect_err("the lease is too short");
    assert!(matches!(error, whl_forward::WorkerError::Lease { .. }));
}
