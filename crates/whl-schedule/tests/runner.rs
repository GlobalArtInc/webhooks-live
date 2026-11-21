mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    KEYRING, RETENTION, Target, add_schedule, captures, config, get, loopback_guard, reload,
    runner, runner_full, runner_with, scene,
};
use time::OffsetDateTime;
use whl_capture::StorageFull;
use whl_forward::GuardConfig;
use whl_schedule::{
    CronSchedule, PAUSE_AFTER_FAILURES, QUOTA_EXCEEDED, SECRET_TOO_SHORT, STORAGE_FULL,
};
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::Uuid;
use whl_store::forward::NewForwardRule;
use whl_store::schedules::{
    NewScheduleRun, PatchScheduleOutcome, QueueRunOutcome, RunOutcome, ScheduleClaimPolicy,
    ScheduleHeader, ScheduleSettlement, ScheduleUpdate, SettleOutcome,
};

const EVERY_FIVE: &str = "*/5 * * * *";

fn next_after(moment: OffsetDateTime) -> OffsetDateTime {
    CronSchedule::parse(EVERY_FIVE, "UTC")
        .expect("a valid expression")
        .next_after(moment)
        .expect("a next occurrence")
}

#[tokio::test]
async fn a_due_schedule_runs_once_and_stores_a_schedule_capture() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(
        &scene,
        EVERY_FIVE,
        get(&target.url("/health?x=1")),
        Some(start),
    )
    .await;
    let runner = runner(&scene, loopback_guard(), None);

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, RunOutcome::Succeeded);
    assert_eq!(reports[0].settled, SettleOutcome::Settled);
    assert_eq!(target.hits(), 1);
    assert!(
        target.heads()[0]
            .to_lowercase()
            .contains("x-webhooks-live-hop: 1")
    );

    let stored = captures(&scene).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].kind, "schedule");
    assert_eq!(stored[0].status, 200);
    assert_eq!(stored[0].method, "GET");
    assert_eq!(stored[0].body, b"target body");
    assert_eq!(
        stored[0].meta["schedule_id"],
        schedule.id.hyphenated().to_string()
    );
    assert_eq!(stored[0].meta["schedule_name"], "probe");
    assert_eq!(stored[0].meta["error"], serde_json::Value::Null);
    assert_eq!(stored[0].meta["truncated"], false);

    let after = reload(&scene, &schedule).await;
    assert_eq!(after.next_run_at, Some(next_after(start)));
    assert_eq!(after.last_run_at, Some(start));
    assert_eq!(after.consecutive_failures, 0);

    assert!(runner.tick().await.expect("the tick works").is_empty());
    assert_eq!(target.hits(), 1);
    scene.clock.set(after.next_run_at.expect("a next run"));
    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert_eq!(target.hits(), 2);
}

#[tokio::test]
async fn runs_missed_during_downtime_are_not_backfilled() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    scene.clock.advance(Duration::from_secs(3 * 24 * 3600));
    let runner = runner(&scene, loopback_guard(), None);

    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert!(runner.tick().await.expect("the tick works").is_empty());
    assert_eq!(target.hits(), 1);
    let after = reload(&scene, &schedule).await;
    let next = after.next_run_at.expect("a next run");
    assert!(next > scene.clock.now_value());
    assert!(next <= scene.clock.now_value() + Duration::from_secs(300));
}

#[tokio::test]
async fn ten_consecutive_failures_pause_the_schedule_and_say_why() {
    let scene = scene().await;
    let target = Target::status(503).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let runner = runner(&scene, loopback_guard(), None);

    for attempt in 1..=PAUSE_AFTER_FAILURES {
        let reports = runner.tick().await.expect("the tick works");
        assert_eq!(reports.len(), 1, "attempt {attempt}");
        assert_eq!(reports[0].outcome, RunOutcome::Failed);
        assert_eq!(reports[0].paused, attempt == PAUSE_AFTER_FAILURES);
        let state = reload(&scene, &schedule).await;
        if let Some(next) = state.next_run_at {
            scene.clock.set(next);
        }
    }
    let paused = reload(&scene, &schedule).await;
    assert!(!paused.enabled);
    assert_eq!(paused.next_run_at, None);
    assert_eq!(paused.consecutive_failures, PAUSE_AFTER_FAILURES);
    let reason = paused.paused_reason.expect("a reason");
    assert!(reason.contains("10 consecutive failures"), "{reason}");
    assert!(reason.contains("HTTP 503"), "{reason}");

    scene.clock.advance(Duration::from_secs(3600));
    assert!(runner.tick().await.expect("the tick works").is_empty());
    assert_eq!(target.hits(), 10);
}

#[tokio::test]
async fn parallel_replicas_never_run_the_same_schedule_twice() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    for _ in 0..4 {
        add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    }
    let first = runner(&scene, loopback_guard(), None);
    let second = runner(&scene, loopback_guard(), None);
    let third = runner(&scene, loopback_guard(), None);

    let (a, b, c) = tokio::join!(first.tick(), second.tick(), third.tick());
    let total = a.expect("a tick").len() + b.expect("a tick").len() + c.expect("a tick").len();
    assert_eq!(total, 4);
    assert_eq!(target.hits(), 4);
    assert_eq!(captures(&scene).await.len(), 4);
}

#[tokio::test]
async fn a_claimed_schedule_is_reclaimed_only_after_its_lease_expires() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let policy = ScheduleClaimPolicy {
        now: start,
        limit: 8,
        lease: Duration::from_secs(60),
        retention: RETENTION,
    };
    let claimed = scene
        .store
        .claim_due_schedules(policy)
        .await
        .expect("the claim works");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].lease, start + Duration::from_secs(60));

    let runner = runner(&scene, loopback_guard(), None);
    scene.clock.advance(Duration::from_secs(59));
    assert!(runner.tick().await.expect("the tick works").is_empty());
    scene.clock.advance(Duration::from_secs(2));
    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert_eq!(target.hits(), 1);

    let stale = ScheduleSettlement {
        id: claimed[0].id,
        lease: claimed[0].lease,
        scheduled_for: claimed[0].scheduled_for,
        last_run_at: None,
        consecutive_failures: 0,
        paused_reason: None,
        next_run_at: Some(start),
        run: NewScheduleRun {
            id: Uuid::now_v7(),
            started_at: start,
            duration_ms: 0,
            outcome: RunOutcome::Succeeded,
            status: None,
            error: None,
            capture_id: None,
        },
    };
    assert_eq!(
        scene.store.settle_schedule(&stale).await.expect("a settle"),
        SettleOutcome::LeaseLost
    );
}

#[tokio::test]
async fn editing_the_timing_during_a_run_keeps_the_new_next_run() {
    let scene = scene().await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get("http://127.0.0.1:9/"), Some(start)).await;
    let claimed = scene
        .store
        .claim_due_schedules(ScheduleClaimPolicy {
            now: start,
            limit: 8,
            lease: Duration::from_secs(60),
            retention: RETENTION,
        })
        .await
        .expect("the claim works")
        .remove(0);
    let moved = start + Duration::from_secs(7200);
    scene
        .store
        .update_schedule(
            &scene.endpoint,
            RETENTION,
            schedule.id,
            &ScheduleUpdate {
                name: schedule.name.clone(),
                cron: "0 * * * *".to_owned(),
                timezone: "UTC".to_owned(),
                request: schedule.request.clone(),
                enabled: true,
                next_run_at: Some(moved),
                reset_failures: false,
                release_lease: true,
            },
        )
        .await
        .expect("the update works");
    let settlement = ScheduleSettlement {
        id: claimed.id,
        lease: claimed.lease,
        scheduled_for: claimed.scheduled_for,
        last_run_at: Some(start),
        consecutive_failures: 0,
        paused_reason: None,
        next_run_at: Some(next_after(start)),
        run: NewScheduleRun {
            id: Uuid::now_v7(),
            started_at: start,
            duration_ms: 1,
            outcome: RunOutcome::Succeeded,
            status: Some(200),
            error: None,
            capture_id: None,
        },
    };
    assert_eq!(
        scene
            .store
            .settle_schedule(&settlement)
            .await
            .expect("a settle"),
        SettleOutcome::LeaseLost
    );
    assert_eq!(reload(&scene, &schedule).await.next_run_at, Some(moved));
}

#[tokio::test]
async fn a_target_in_a_refused_range_is_not_called_and_fails_the_run() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let runner = runner(&scene, GuardConfig::default(), None);

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].outcome, RunOutcome::Failed);
    let error = reports[0].error.clone().expect("an error");
    assert!(error.contains("refused"), "{error}");
    assert_eq!(target.hits(), 0);
    let stored = captures(&scene).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].status, 0);
    assert_eq!(stored[0].meta["error"], error);
    assert_eq!(reload(&scene, &schedule).await.consecutive_failures, 1);
}

#[tokio::test]
async fn secret_values_reach_the_target_and_never_reach_storage() {
    let scene = scene().await;
    let keyring = Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"));
    let name = SecretName::parse("API_TOKEN").expect("a valid name");
    let value = "s3cr3t-token-value";
    let sealed = keyring
        .seal(
            &scene.endpoint,
            &name,
            &Secret::new(value.to_owned()).expect("a valid secret"),
        )
        .expect("the secret seals");
    scene
        .store
        .put_secret(&scene.endpoint, RETENTION, &name, &sealed, 20)
        .await
        .expect("the secret is stored");
    let target = Target::start(|seen| (200, seen.head.clone())).await;
    let mut request = get(&target.url("/echo?token={{ secret.API_TOKEN }}"));
    request.headers.push(ScheduleHeader {
        name: "Authorization".to_owned(),
        value: "Bearer {{ secret.API_TOKEN }}".to_owned(),
    });
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, request, Some(start)).await;
    let runner = runner(&scene, loopback_guard(), Some(keyring));

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].outcome, RunOutcome::Succeeded);
    let head = target.heads().remove(0);
    assert!(head.contains(&format!("token={value}")), "{head}");
    assert!(head.contains(&format!("Bearer {value}")), "{head}");

    let stored = captures(&scene).await;
    let body = String::from_utf8_lossy(&stored[0].body).into_owned();
    assert!(body.contains("[secret:API_TOKEN]"), "{body}");
    let everything = format!(
        "{body} {} {} {} {} {:?}",
        stored[0].url, stored[0].meta, stored[0].headers, stored[0].host, reports[0].error
    );
    assert!(!everything.contains(value), "{everything}");
    let history: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT error FROM schedule_runs WHERE schedule_id = $1")
            .bind(schedule.id)
            .fetch_all(scene.db.pool())
            .await
            .expect("the history is read");
    assert!(
        history
            .iter()
            .all(|(error,)| error.as_deref().is_none_or(|text| !text.contains(value)))
    );
    let stored_request = reload(&scene, &schedule).await.request;
    assert!(!format!("{stored_request:?}").contains(value));
}

#[tokio::test]
async fn a_missing_secret_fails_the_run_without_calling_the_target() {
    let scene = scene().await;
    let keyring = Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"));
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(
        &scene,
        EVERY_FIVE,
        get(&target.url("/?k={{ secret.NOT_THERE }}")),
        Some(start),
    )
    .await;
    let runner = runner(&scene, loopback_guard(), Some(keyring));

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].outcome, RunOutcome::Failed);
    assert!(
        reports[0]
            .error
            .clone()
            .expect("an error")
            .contains("NOT_THERE")
    );
    assert_eq!(target.hits(), 0);
}

#[tokio::test]
async fn runs_beyond_the_hourly_quota_are_skipped_without_a_capture() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, "* * * * *", get(&target.url("/")), Some(start)).await;
    let runner = runner_with(&scene, loopback_guard(), None, config(500, 2));

    let mut outcomes = Vec::new();
    for _ in 0..3 {
        let reports = runner.tick().await.expect("the tick works");
        assert_eq!(reports.len(), 1);
        outcomes.push(reports[0].outcome);
        if outcomes.len() == 3 {
            assert_eq!(reports[0].error.as_deref(), Some(QUOTA_EXCEEDED));
        }
        let next = reload(&scene, &schedule)
            .await
            .next_run_at
            .expect("a next run");
        scene.clock.set(next);
    }
    assert_eq!(
        outcomes,
        [
            RunOutcome::Succeeded,
            RunOutcome::Succeeded,
            RunOutcome::Skipped
        ]
    );
    assert_eq!(target.hits(), 2);
    assert_eq!(captures(&scene).await.len(), 2);
    assert_eq!(reload(&scene, &schedule).await.consecutive_failures, 0);
}

#[tokio::test]
async fn a_full_endpoint_counts_as_a_failure() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, "* * * * *", get(&target.url("/")), Some(start)).await;
    let runner = runner_with(&scene, loopback_guard(), None, config(1, 1000));

    assert_eq!(
        runner.tick().await.expect("the tick works")[0].outcome,
        RunOutcome::Succeeded
    );
    scene.clock.set(
        reload(&scene, &schedule)
            .await
            .next_run_at
            .expect("a next run"),
    );
    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].outcome, RunOutcome::Failed);
    assert!(
        reports[0]
            .error
            .clone()
            .expect("an error")
            .starts_with("endpoint_full")
    );
    assert_eq!(captures(&scene).await.len(), 1);
}

#[tokio::test]
async fn run_now_makes_a_future_schedule_due_and_refuses_a_disabled_one() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let later = start + Duration::from_secs(3600);
    let schedule = add_schedule(&scene, "0 * * * *", get(&target.url("/")), Some(later)).await;
    let runner = runner(&scene, loopback_guard(), None);
    assert!(runner.tick().await.expect("the tick works").is_empty());

    let queued = scene
        .store
        .queue_schedule_run(&scene.endpoint, RETENTION, schedule.id, start)
        .await
        .expect("the run is queued");
    assert_eq!(queued, QueueRunOutcome::Queued(start));
    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert_eq!(target.hits(), 1);

    scene
        .store
        .update_schedule(
            &scene.endpoint,
            RETENTION,
            schedule.id,
            &ScheduleUpdate {
                name: schedule.name.clone(),
                cron: schedule.cron.clone(),
                timezone: schedule.timezone.clone(),
                request: schedule.request.clone(),
                enabled: false,
                next_run_at: None,
                reset_failures: false,
                release_lease: true,
            },
        )
        .await
        .expect("the update works");
    let refused = scene
        .store
        .queue_schedule_run(&scene.endpoint, RETENTION, schedule.id, start)
        .await
        .expect("the run is refused");
    assert_eq!(refused, QueueRunOutcome::Disabled);
}

#[tokio::test]
async fn the_run_history_keeps_the_newest_fifty() {
    let scene = scene().await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get("http://127.0.0.1:9/"), Some(start)).await;
    for index in 0..55_u32 {
        let started_at = start + Duration::from_secs(u64::from(index));
        let settlement = ScheduleSettlement {
            id: schedule.id,
            lease: start,
            scheduled_for: start,
            last_run_at: None,
            consecutive_failures: 0,
            paused_reason: None,
            next_run_at: None,
            run: NewScheduleRun {
                id: Uuid::now_v7(),
                started_at,
                duration_ms: 1,
                outcome: RunOutcome::Succeeded,
                status: Some(200),
                error: None,
                capture_id: None,
            },
        };
        scene
            .store
            .settle_schedule(&settlement)
            .await
            .expect("a settle");
    }
    let runs = scene
        .store
        .list_schedule_runs(&scene.endpoint, schedule.id)
        .await
        .expect("the history is read")
        .expect("the schedule exists");
    assert_eq!(runs.len(), 50);
    assert_eq!(runs[0].started_at, start + Duration::from_secs(54));
    assert_eq!(runs[49].started_at, start + Duration::from_secs(5));
}

async fn store_secret(scene: &common::Scene, keyring: &Keyring, name: &str, value: &str) {
    let name = SecretName::parse(name).expect("a valid name");
    let sealed = keyring
        .seal(
            &scene.endpoint,
            &name,
            &Secret::new(value.to_owned()).expect("a valid secret"),
        )
        .expect("the secret seals");
    scene
        .store
        .put_secret(&scene.endpoint, RETENTION, &name, &sealed, 20)
        .await
        .expect("the secret is stored");
}

#[tokio::test]
async fn the_next_run_is_counted_from_the_scheduled_time_not_from_the_poll_lag() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, "* * * * *", get(&target.url("/")), Some(start)).await;
    let mut settings = config(500, 1000);
    settings.min_interval = Duration::from_secs(600);
    let runner = runner_with(&scene, loopback_guard(), None, settings);
    scene.clock.advance(Duration::from_secs(30));

    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert_eq!(
        reload(&scene, &schedule).await.next_run_at,
        Some(start + Duration::from_secs(600))
    );
}

#[tokio::test]
async fn a_full_instance_skips_the_run_without_calling_the_target() {
    let scene = scene().await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let full = StorageFull::default();
    full.set(true);
    let runner = runner_full(&scene, full);

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].outcome, RunOutcome::Skipped);
    assert_eq!(reports[0].error.as_deref(), Some(STORAGE_FULL));
    assert_eq!(target.hits(), 0);
    assert!(captures(&scene).await.is_empty());
}

#[tokio::test]
async fn a_secret_too_short_to_redact_pauses_the_schedule_before_any_request() {
    let scene = scene().await;
    let keyring = Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"));
    store_secret(&scene, &keyring, "SHORT", "abc").await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(
        &scene,
        EVERY_FIVE,
        get(&target.url("/?k={{ secret.SHORT }}")),
        Some(start),
    )
    .await;
    let runner = runner(&scene, loopback_guard(), Some(keyring));

    let reports = runner.tick().await.expect("the tick works");
    assert!(reports[0].paused);
    assert_eq!(target.hits(), 0);
    let paused = reload(&scene, &schedule).await;
    assert!(!paused.enabled);
    assert_eq!(paused.paused_reason.as_deref(), Some(SECRET_TOO_SHORT));
}

#[tokio::test]
async fn the_recorded_final_url_never_holds_a_secret_in_a_normalised_form() {
    let scene = scene().await;
    let keyring = Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"));
    store_secret(&scene, &keyring, "SPACED", "pa ss$word").await;
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(
        &scene,
        EVERY_FIVE,
        get(&target.url("/?k={{ secret.SPACED }}")),
        Some(start),
    )
    .await;
    let runner = runner(&scene, loopback_guard(), Some(keyring));

    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    let stored = captures(&scene).await;
    let meta = stored[0].meta.to_string();
    assert!(meta.contains("[secret:SPACED]"), "{meta}");
    assert!(!meta.contains("pa%20ss"), "{meta}");
    assert!(!stored[0].url.contains("pa ss"), "{}", stored[0].url);
}

#[tokio::test]
async fn a_redirect_is_returned_as_the_answer_and_not_followed() {
    let scene = scene().await;
    let elsewhere = Target::status(200).await;
    let location = elsewhere.url("/moved");
    let target = Target::start(move |_| (302, location.clone())).await;
    let start = scene.clock.now_value();
    add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let runner = runner(&scene, loopback_guard(), None);

    let reports = runner.tick().await.expect("the tick works");
    assert_eq!(reports[0].status, Some(302));
    assert_eq!(elsewhere.hits(), 0);
}

#[tokio::test]
async fn a_schedule_capture_creates_no_forward_jobs() {
    let scene = scene().await;
    scene
        .store
        .create_forward_rule(
            &scene.endpoint,
            RETENTION,
            &NewForwardRule {
                enabled: true,
                target_url: "https://8.8.8.8/hook".to_owned(),
                method: None,
                content_type: None,
                header_allowlist: Vec::new(),
                append_path: false,
            },
        )
        .await
        .expect("the rule is stored");
    let target = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(&scene, EVERY_FIVE, get(&target.url("/")), Some(start)).await;
    let runner = runner(&scene, loopback_guard(), None);

    assert_eq!(runner.tick().await.expect("the tick works").len(), 1);
    assert_eq!(captures(&scene).await.len(), 1);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_jobs")
        .fetch_one(scene.db.pool())
        .await
        .expect("the jobs are counted");
    assert_eq!(jobs, 0);
}

#[tokio::test]
async fn a_patch_plans_from_the_row_a_settle_has_committed() {
    let scene = scene().await;
    let start = scene.clock.now_value();
    let schedule = add_schedule(&scene, EVERY_FIVE, get("http://127.0.0.1:9/"), Some(start)).await;
    let claimed = scene
        .store
        .claim_due_schedules(ScheduleClaimPolicy {
            now: start,
            limit: 8,
            lease: Duration::from_secs(60),
            retention: RETENTION,
        })
        .await
        .expect("the claim works")
        .remove(0);
    scene
        .store
        .settle_schedule(&ScheduleSettlement {
            id: claimed.id,
            lease: claimed.lease,
            scheduled_for: claimed.scheduled_for,
            last_run_at: Some(start),
            consecutive_failures: PAUSE_AFTER_FAILURES,
            paused_reason: Some("10 consecutive failures".to_owned()),
            next_run_at: None,
            run: NewScheduleRun {
                id: Uuid::now_v7(),
                started_at: start,
                duration_ms: 1,
                outcome: RunOutcome::Failed,
                status: Some(500),
                error: None,
                capture_id: None,
            },
        })
        .await
        .expect("a settle");

    let seen = std::sync::Mutex::new(None);
    let outcome = scene
        .store
        .patch_schedule::<()>(&scene.endpoint, RETENTION, schedule.id, |existing| {
            *seen.lock().expect("not poisoned") =
                Some((existing.enabled, existing.consecutive_failures));
            Ok(ScheduleUpdate {
                name: "renamed".to_owned(),
                cron: existing.cron.clone(),
                timezone: existing.timezone.clone(),
                request: existing.request.clone(),
                enabled: existing.enabled,
                next_run_at: existing.next_run_at,
                reset_failures: false,
                release_lease: false,
            })
        })
        .await
        .expect("the patch works");
    assert!(matches!(outcome, PatchScheduleOutcome::Updated(_)));
    assert_eq!(
        *seen.lock().expect("not poisoned"),
        Some((false, PAUSE_AFTER_FAILURES))
    );
    let after = reload(&scene, &schedule).await;
    assert!(!after.enabled);
    assert_eq!(after.next_run_at, None);
    assert_eq!(after.name, "renamed");
}

#[tokio::test]
async fn a_hung_target_does_not_hold_back_the_other_due_schedules() {
    let scene = scene().await;
    let tarpit = Target::tarpit().await;
    let fast = Target::status(200).await;
    let start = scene.clock.now_value();
    add_schedule(&scene, EVERY_FIVE, get(&tarpit.url("/")), Some(start)).await;
    let mut guard = loopback_guard();
    guard.total_timeout = Duration::from_secs(4);
    guard.hop_timeout = Duration::from_secs(4);
    let mut settings = config(500, 1000);
    settings.batch = 2;
    settings.poll_interval = Duration::from_millis(100);
    let runner = runner_with(&scene, guard, None, settings);
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let mut handle = runner.spawn(async move {
        let mut stopped = stopped;
        let _ = stopped.wait_for(|stop| *stop).await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    add_schedule(&scene, EVERY_FIVE, get(&fast.url("/")), Some(start)).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while fast.hits() == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(fast.hits(), 1);
    let _ = stop.send(true);
    handle.join().await;
}
