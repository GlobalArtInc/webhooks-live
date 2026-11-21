mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{
    Scene, Target, closed_port, fake_value, header, keyring, loopback_guard, new_endpoint, scene,
    settings, start,
};
use serde_json::{Value, json};
use uuid::Uuid;
use whl_forward::GuardConfig;
use whl_rules_worker::step_id;
use whl_store::rule_exec::RerunOutcome;

fn rule(steps: Vec<Value>) -> Value {
    json!({"name": "after", "steps": steps})
}

fn post(name: &str, url: &str) -> Value {
    json!({"name": name, "kind": "http", "method": "POST", "url": url, "body": "{{ request.body }}"})
}

async fn channel(scene: &Scene, endpoint: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO notification_channels \
         (id, endpoint_id, kind, name, enabled, trigger, config, per_hour) \
         VALUES ($1, $2, 'webhook', 'hook', true, 'rule', '{}'::jsonb, 30)",
    )
    .bind(id)
    .bind(endpoint)
    .execute(scene.db.pool())
    .await
    .expect("the channel is inserted");
    id
}

#[tokio::test]
async fn an_http_step_runs_after_the_response_with_the_next_hop_and_its_answer_reaches_later_steps()
{
    let scene = scene().await;
    let target = Target::start(|_| (201, "created".to_owned())).await;
    scene
        .add_rule(
            rule(vec![
                json!({
                    "name": "fetch", "kind": "http", "method": "POST", "url": target.url("/in"),
                    "headers": [{"name": "X-Token", "value": "{{ request.header.x-token }}"}],
                    "body": "{{ request.body }}",
                }),
                json!({"kind": "set_var", "into": "seen", "value": "{{ step.fetch.status }}:{{ step.fetch.body }}"}),
            ]),
            &[],
        )
        .await;
    let capture = scene
        .capture(vec![
            header("X-Token", "abc"),
            header("X-Webhooks-Live-Hop", "1"),
        ])
        .await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let seen = target.seen();
    let [request] = seen.as_slice() else {
        panic!("one request is expected");
    };
    assert_eq!(request.request_line(), "POST /in HTTP/1.1");
    assert_eq!(request.header("x-webhooks-live-hop").as_deref(), Some("2"));
    assert_eq!(
        request.header("x-webhooks-live-delivery"),
        Some(format!("{}:0", capture.unpack().hyphenated()))
    );
    assert_eq!(request.header("x-token").as_deref(), Some("abc"));
    assert!(
        request
            .header("user-agent")
            .is_some_and(|agent| agent.starts_with("webhooks.live/"))
    );
    assert_eq!(request.body, b"hello");
    assert_eq!(run.vars["seen"], "201:created");
    let steps = run.step_entries();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["kind"], "http");
    assert_eq!(steps[0]["output"]["status"], 201);
}

#[tokio::test]
async fn a_target_in_a_refused_range_fails_the_run_and_is_never_contacted() {
    let scene = scene().await;
    let target = Target::status(200).await;
    scene
        .add_rule(rule(vec![post("fetch", &target.url("/in"))]), &[])
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, GuardConfig::default(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    let error = run.error.expect("the failure is recorded");
    assert!(error.starts_with("http: "), "{error}");
    assert!(error.contains("refused"), "{error}");
    assert_eq!(target.hits(), 0);
}

#[tokio::test]
async fn a_secret_reaches_the_target_but_is_never_stored_in_clear() {
    let scene = scene().await;
    let value = fake_value(&["tok", "en-demo-", "1234"]);
    scene.put_secret("API_KEY", &value).await;
    let target =
        Target::start(|seen| (200, seen.header("authorization").unwrap_or_default())).await;
    scene
        .add_rule(
            rule(vec![
                json!({
                    "name": "fetch", "kind": "http", "method": "GET", "url": target.url("/s"),
                    "headers": [{"name": "Authorization", "value": "Bearer {{ secret.API_KEY }}"}],
                }),
                json!({"kind": "set_var", "into": "echo", "value": "{{ step.fetch.body }}"}),
            ]),
            &["API_KEY"],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), Some(keyring()));

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let seen = target.seen();
    assert_eq!(
        seen[0].header("authorization"),
        Some(format!("Bearer {value}"))
    );
    let stored = run.everything();
    assert!(!stored.contains(&value), "{stored}");
    assert!(stored.contains("[secret:API_KEY]"), "{stored}");
}

#[tokio::test]
async fn a_failing_status_is_retried_with_backoff_and_the_late_answer_is_the_result() {
    let scene = scene().await;
    let counter = Arc::new(AtomicUsize::new(0));
    let target = Target::start({
        let counter = Arc::clone(&counter);
        move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                (503, "busy".to_owned())
            } else {
                (200, "fine".to_owned())
            }
        }
    })
    .await;
    scene
        .add_rule(rule(vec![post("fetch", &target.url("/in"))]), &[])
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    assert_eq!(target.hits(), 2);
    assert_eq!(run.step_attempts, 0);
    let steps = run.step_entries();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["output"]["status"], 200);
    assert_eq!(steps[0]["output"]["attempts"], 2);
    let entered = run
        .log
        .as_array()
        .expect("the log is an array")
        .iter()
        .filter(|entry| entry.get("rule_name").is_some())
        .count();
    assert_eq!(entered, 1);
}

#[tokio::test]
async fn a_transport_failure_fails_the_run_after_the_last_attempt_and_later_steps_do_not_run() {
    let scene = scene().await;
    let port = closed_port().await;
    scene
        .add_rule(
            rule(vec![
                post("fetch", &format!("http://127.0.0.1:{port}/in")),
                json!({"kind": "set_var", "into": "later", "value": "ran"}),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    let error = run.error.clone().expect("the failure is recorded");
    assert!(error.starts_with("http: "), "{error}");
    assert!(error.contains("attempt 3"), "{error}");
    assert_eq!(run.step_entries().len(), 1);
    assert!(run.vars.get("later").is_none());
}

#[tokio::test]
async fn continue_on_error_lets_the_run_go_on_and_exposes_the_error_to_later_steps() {
    let scene = scene().await;
    let port = closed_port().await;
    let mut failing = post("fetch", &format!("http://127.0.0.1:{port}/in"));
    failing["continue_on_error"] = json!(true);
    scene
        .add_rule(
            rule(vec![
                failing,
                json!({"kind": "set_var", "into": "failure", "value": "{{ step.fetch.error }}"}),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    assert_eq!(run.vars["failure"], "http");
    assert_eq!(run.step_entries()[0]["outcome"], "failed");
}

#[tokio::test]
async fn a_replayed_run_enqueues_the_same_forward_job_only_once() {
    let scene = scene().await;
    scene
        .add_rule(
            rule(vec![
                json!({"kind": "forward", "target_url": "https://hooks.example.com/in", "method": "PUT"}),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);
    let first = scene.wait_for_final(&capture).await;
    worker.stop().await;
    assert_eq!(first.state, "succeeded", "{:?}", first.error);
    assert_eq!(scene.count("SELECT count(*) FROM forward_jobs").await, 1);

    scene
        .execute(
            "UPDATE rule_runs SET state = 'running', locked_until = now() - interval '1 second', \
             next_step = 0, log = '[]'::jsonb",
        )
        .await;
    let worker = start(&scene, loopback_guard(), settings(), None);
    let replayed = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(replayed.state, "succeeded", "{:?}", replayed.error);
    assert_eq!(replayed.attempts, 2);
    assert_eq!(scene.count("SELECT count(*) FROM forward_jobs").await, 1);
    let job: Uuid = sqlx::query_scalar("SELECT id FROM forward_jobs")
        .fetch_one(scene.db.pool())
        .await
        .expect("the job is readable");
    assert_eq!(job, step_id(capture.unpack(), 0));
}

#[tokio::test]
async fn a_notify_step_queues_a_delivery_for_its_own_channel_and_fails_for_a_foreign_one() {
    let scene = scene().await;
    let own = channel(&scene, scene.endpoint.unpack()).await;
    let other = new_endpoint(&scene.store).await;
    let foreign = channel(&scene, other.unpack()).await;
    scene
        .add_rule(
            rule(vec![
                json!({"kind": "notify", "channel_id": own.to_string(), "message": "got {{ request.body }}"}),
                json!({"kind": "notify", "channel_id": foreign.to_string()}),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    let error = run.error.expect("the failure is recorded");
    assert!(
        error.contains("does not belong to this endpoint"),
        "{error}"
    );
    let messages: Vec<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT channel_id, message FROM notification_deliveries")
            .fetch_all(scene.db.pool())
            .await
            .expect("the deliveries are readable");
    assert_eq!(messages, [(own, Some("got hello".to_owned()))]);
}

#[tokio::test]
async fn a_store_s3_step_puts_the_body_with_a_signed_path_style_request() {
    let scene = scene().await;
    let access = fake_value(&["acc", "ess-", "demo-id"]);
    let secret = fake_value(&["sec", "ret-", "demo-key"]);
    scene.put_secret("S3_ID", &access).await;
    scene.put_secret("S3_KEY", &secret).await;
    let target = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![json!({
                "kind": "store_s3", "bucket_url": target.url("/bucket"), "prefix": "in/",
                "region": "eu-1", "access_key_secret": "S3_ID", "secret_key_secret": "S3_KEY",
                "content": "body",
            })]),
            &["S3_ID", "S3_KEY"],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), Some(keyring()));

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let seq: i64 = sqlx::query_scalar("SELECT seq FROM captures WHERE id = $1")
        .bind(capture.unpack())
        .fetch_one(scene.db.pool())
        .await
        .expect("the sequence is readable");
    let seen = target.seen();
    let [request] = seen.as_slice() else {
        panic!("one request is expected");
    };
    assert_eq!(
        request.request_line(),
        format!(
            "PUT /bucket/in/{}/{seq}-{}.bin HTTP/1.1",
            scene.endpoint.unpack(),
            capture.unpack().hyphenated()
        )
    );
    assert_eq!(
        request.header("x-amz-content-sha256").as_deref(),
        Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
    );
    let authorization = request
        .header("authorization")
        .expect("the request is signed");
    assert!(
        authorization.starts_with(&format!("AWS4-HMAC-SHA256 Credential={access}/")),
        "{authorization}"
    );
    assert!(
        authorization.contains(
            "/eu-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature="
        ),
        "{authorization}"
    );
    assert_eq!(request.header("x-webhooks-live-hop").as_deref(), Some("1"));
    assert_eq!(request.body, b"hello");
    assert!(!run.everything().contains(&secret));
}

#[tokio::test]
async fn a_store_s3_step_can_put_the_capture_as_json() {
    let scene = scene().await;
    scene
        .put_secret("S3_ID", &fake_value(&["acc", "ess-", "demo-id"]))
        .await;
    scene
        .put_secret("S3_KEY", &fake_value(&["sec", "ret-", "demo-key"]))
        .await;
    let target = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![json!({
                "kind": "store_s3", "bucket_url": target.url("/"), "prefix": "",
                "region": "eu-1", "access_key_secret": "S3_ID", "secret_key_secret": "S3_KEY",
                "content": "capture_json",
            })]),
            &["S3_ID", "S3_KEY"],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), Some(keyring()));

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let seen = target.seen();
    let request = &seen[0];
    assert!(request.request_line().ends_with(".json HTTP/1.1"));
    assert_eq!(
        request.header("content-type").as_deref(),
        Some("application/json")
    );
    let document: Value = serde_json::from_slice(&request.body).expect("the body is JSON");
    assert_eq!(
        document["capture_id"],
        capture.unpack().hyphenated().to_string()
    );
    assert_eq!(document["body"]["text"], "hello");
}

#[tokio::test]
async fn a_store_s3_prefix_with_dot_segments_is_refused_before_any_request() {
    let scene = scene().await;
    scene
        .put_secret("S3_ID", &fake_value(&["acc", "ess-", "demo-id"]))
        .await;
    scene
        .put_secret("S3_KEY", &fake_value(&["sec", "ret-", "demo-key"]))
        .await;
    let target = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![json!({
                "kind": "store_s3", "bucket_url": target.url("/bucket"), "prefix": "../other/",
                "region": "eu-1", "access_key_secret": "S3_ID", "secret_key_secret": "S3_KEY",
                "content": "body",
            })]),
            &["S3_ID", "S3_KEY"],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), Some(keyring()));

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    assert!(
        run.error
            .expect("the failure is recorded")
            .contains("dot segments")
    );
    assert_eq!(target.hits(), 0);
}

#[tokio::test]
async fn a_capture_at_the_hop_limit_never_runs_and_a_rerun_of_it_fails_with_hop_limit() {
    let scene = scene().await;
    let target = Target::status(200).await;
    scene
        .add_rule(rule(vec![post("fetch", &target.url("/in"))]), &[])
        .await;
    let capture = scene
        .capture(vec![header("X-Webhooks-Live-Hop", "3")])
        .await;
    assert_eq!(scene.count("SELECT count(*) FROM rule_runs").await, 0);
    let created = scene
        .store
        .create_rule_rerun(
            &scene.endpoint,
            common::RETENTION,
            capture.unpack(),
            Uuid::now_v7(),
        )
        .await
        .expect("the rerun is requested");
    assert!(matches!(created, RerunOutcome::Created(_)));
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_rerun(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    assert!(
        run.error
            .expect("the failure is recorded")
            .starts_with("hop_limit")
    );
    assert_eq!(target.hits(), 0);
}

#[tokio::test]
async fn a_run_resumes_exactly_at_its_saved_step_with_its_saved_step_results() {
    let scene = scene().await;
    let first = Target::status(200).await;
    let second = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![
                post("first", &first.url("/a")),
                json!({
                    "name": "second", "kind": "http", "method": "POST", "url": second.url("/b"),
                    "body": "{{ step.first.status }}",
                }),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let rule_id: String = sqlx::query_scalar("SELECT id::text FROM rules")
        .fetch_one(scene.db.pool())
        .await
        .expect("the rule is readable");
    let log = json!([
        {"rule_id": rule_id, "rule_name": "after", "matched": true},
        {"rule_id": rule_id, "step": 0, "name": "first", "kind": "http", "outcome": "ok",
         "duration_ms": 1, "detail": null, "output": null},
    ]);
    let steps = json!({"first": {"status": 200, "headers": [], "body": "", "duration_ms": 1, "error": null}});
    sqlx::query(
        "UPDATE rule_runs SET state = 'running', attempts = 1, next_step = 1, \
         locked_until = now() - interval '1 second', log = $1::text::jsonb, \
         step_results = $2::text::jsonb WHERE capture_id = $3",
    )
    .bind(log.to_string())
    .bind(steps.to_string())
    .bind(capture.unpack())
    .execute(scene.db.pool())
    .await
    .expect("the crash is simulated");
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    assert_eq!(first.hits(), 0);
    assert_eq!(second.hits(), 1);
    assert_eq!(second.seen()[0].body, b"200");
    assert_eq!(run.step_entries().len(), 2);
}

#[tokio::test]
async fn an_expired_sync_lease_replays_the_whole_plan_with_the_response_as_a_no_op() {
    let scene = scene().await;
    let target = Target::status(200).await;
    scene
        .add_rule(
            json!({"name": "answer", "steps": [
                {"kind": "respond", "status": "202", "body": "late"},
                post("fetch", &target.url("/in")),
            ]}),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    assert_eq!(scene.run(&capture).await.state, "sync");
    scene
        .execute("UPDATE rule_runs SET locked_until = now() - interval '1 second'")
        .await;
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let steps = run.step_entries();
    assert_eq!(steps[0]["kind"], "respond");
    assert_eq!(steps[0]["outcome"], "no_op");
    assert_eq!(target.hits(), 1);
}

#[tokio::test]
async fn the_outbound_budget_stops_a_run_before_the_request_that_would_exceed_it() {
    let scene = scene().await;
    let target = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![
                post("one", &target.url("/a")),
                post("two", &target.url("/b")),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let mut limited = settings();
    limited.limits.endpoint_per_hour = 1;
    let worker = start(&scene, loopback_guard(), limited, None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "failed");
    assert!(
        run.error
            .expect("the failure is recorded")
            .starts_with("quota_exceeded")
    );
    assert_eq!(target.hits(), 1);
}

#[tokio::test]
async fn a_deferred_side_effect_runs_first_with_the_variables_of_its_snapshot() {
    let scene = scene().await;
    scene
        .add_rule(
            rule(vec![json!({"kind": "set_var", "into": "a", "value": "1"})]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let own = channel(&scene, scene.endpoint.unpack()).await;
    let plan = json!({
        "version": 1,
        "rules": [],
        "resume": null,
        "snapshots": [{"vars": {"who": "snapshot"}, "payload": null}],
        "deferred": [{
            "rule_id": Uuid::now_v7().to_string(),
            "rule_name": "before",
            "step": 1,
            "action": {
                "kind": "notify", "continue_on_error": false,
                "channel_id": own.to_string(), "message": "hi {{ var.who }}",
            },
            "snapshot": 0,
        }],
        "payload": null,
    });
    sqlx::query("UPDATE rule_runs SET plan = $1::text::jsonb WHERE capture_id = $2")
        .bind(plan.to_string())
        .bind(capture.unpack())
        .execute(scene.db.pool())
        .await
        .expect("the plan is replaced");
    let worker = start(&scene, loopback_guard(), settings(), None);

    let run = scene.wait_for_final(&capture).await;
    worker.stop().await;

    assert_eq!(run.state, "succeeded", "{:?}", run.error);
    let message: Option<String> = sqlx::query_scalar("SELECT message FROM notification_deliveries")
        .fetch_one(scene.db.pool())
        .await
        .expect("the delivery is readable");
    assert_eq!(message.as_deref(), Some("hi snapshot"));
    assert_eq!(run.step_entries()[0]["outcome"], "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shutdown_releases_the_run_at_its_next_checkpoint_and_a_peer_finishes_it() {
    let scene = scene().await;
    let slow = Target::start(|_| {
        std::thread::sleep(Duration::from_millis(400));
        (200, "slow".to_owned())
    })
    .await;
    let next = Target::status(200).await;
    scene
        .add_rule(
            rule(vec![
                post("first", &slow.url("/a")),
                post("second", &next.url("/b")),
            ]),
            &[],
        )
        .await;
    let capture = scene.capture(Vec::new()).await;
    let worker = start(&scene, loopback_guard(), settings(), None);
    let started = Instant::now();
    while slow.hits() == 0 {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the first request never arrived"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    worker.stop().await;

    let parked = scene.run(&capture).await;
    assert_eq!(parked.state, "queued", "{:?}", parked.error);
    assert_eq!(parked.next_step, 1);
    assert_eq!(next.hits(), 0);
    let unleased: bool = sqlx::query_scalar("SELECT locked_until IS NULL FROM rule_runs")
        .fetch_one(scene.db.pool())
        .await
        .expect("the lease is readable");
    assert!(unleased);

    let peer = start(&scene, loopback_guard(), settings(), None);
    let done = scene.wait_for_final(&capture).await;
    peer.stop().await;

    assert_eq!(done.state, "succeeded", "{:?}", done.error);
    assert_eq!(slow.hits(), 1);
    assert_eq!(next.hits(), 1);
}
