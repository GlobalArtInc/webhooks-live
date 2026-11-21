mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use serde_json::json;
use whl_script::{PoolConfig, PoolError, RunLimits, ScriptError, ScriptJob, ScriptPool};

fn spinner(time: Duration) -> ScriptJob {
    job_with(
        compile("loop {}"),
        default_request(),
        RunLimits {
            max_operations: u64::MAX / 2,
            time,
            ..RunLimits::default()
        },
    )
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "condition never held"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_full_queue_refuses_new_work_and_shutdown_stops_what_runs_and_drops_what_waits() {
    let pool = pool(1, 1);
    let running = pool
        .submit(spinner(Duration::from_secs(60)))
        .expect("first job");
    wait_until(|| pool.queued() == 0);
    let waiting = pool
        .submit(spinner(Duration::from_secs(60)))
        .expect("second job queues");
    assert_eq!(pool.queued(), 1);
    let refused = pool
        .submit(spinner(Duration::from_secs(60)))
        .expect_err("queue is full");
    assert_eq!(refused, ScriptError::Overloaded);
    assert_eq!(refused.code(), "overloaded");

    let started = Instant::now();
    pool.shutdown();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(running.wait().expect_err("stopped"), ScriptError::Cancelled);
    assert_eq!(waiting.wait().expect_err("dropped"), ScriptError::ShutDown);
    assert_eq!(pool.queued(), 0);
    assert_eq!(
        pool.submit(spinner(Duration::from_secs(1)))
            .expect_err("closed"),
        ScriptError::ShutDown
    );
    pool.shutdown();
}

#[test]
fn dropping_the_pool_joins_its_workers() {
    let pool = pool(2, 4);
    let pending = pool.submit(spinner(Duration::from_secs(60))).expect("job");
    wait_until(|| pool.queued() == 0);
    let started = Instant::now();
    drop(pool);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(pending.wait().is_err());
}

#[test]
fn a_late_result_times_out_and_cancels_so_the_worker_is_free_again() {
    let pool = ScriptPool::new(PoolConfig {
        threads: 1,
        result_grace: Duration::from_millis(50),
        ..PoolConfig::default()
    })
    .expect("pool starts");
    let mut stuck = spinner(Duration::from_millis(50));
    stuck.clock = Arc::new(FrozenClock);
    let error = pool.run(stuck).expect_err("no result in time");
    assert_eq!(error, ScriptError::ResultTimeout);
    assert_eq!(error.outcome(), "time");

    let mut follow_up = job(compile("#{ vars: #{ free: true } }"));
    follow_up.limits.time = Duration::from_secs(5);
    let output = pool.run(follow_up).expect("the worker is free");
    assert_eq!(output.vars["free"], json!(true));
}

#[test]
fn results_can_be_polled_without_blocking() {
    let pool = pool(1, 4);
    let pending = pool
        .submit(spinner(Duration::from_millis(100)))
        .expect("job");
    assert!(pending.try_result().is_none());
    let mut outcome = None;
    wait_until(|| {
        outcome = pending.try_result();
        outcome.is_some()
    });
    assert_eq!(
        outcome.expect("finished").expect_err("deadline"),
        ScriptError::Time
    );
}

#[test]
fn many_clients_share_the_bounded_pool() {
    let pool = Arc::new(pool(2, 256));
    let script = compile("#{ vars: #{ n: request.n * 2 } }");
    let handles: Vec<_> = (0..6)
        .map(|client| {
            let pool = Arc::clone(&pool);
            let script = script.clone();
            std::thread::spawn(move || {
                for round in 0..20_i64 {
                    let n = client * 100 + round;
                    let input = job_with(script.clone(), json!({ "n": n }), RunLimits::default());
                    let output = pool.run(input).expect("run");
                    assert_eq!(output.vars["n"], json!(n * 2));
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("client finishes");
    }
}

#[test]
fn throughput_stays_sane() {
    let pool = Arc::new(pool(2, 256));
    let script = compile(
        "let total = 0; for i in 0..50 { total += i; } #{ vars: #{ total: total, h: sha256(request.method) } }",
    );
    let runs = 400;
    let started = Instant::now();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let pool = Arc::clone(&pool);
            let script = script.clone();
            std::thread::spawn(move || {
                for _ in 0..runs / 4 {
                    pool.run(job(script.clone())).expect("run");
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("client finishes");
    }
    let elapsed = started.elapsed();
    let rate = f64::from(runs) / elapsed.as_secs_f64();
    println!("throughput: {rate:.0} runs per second ({runs} runs in {elapsed:?})");
    assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
    assert!(rate > 20.0, "{rate}");
}

#[test]
fn the_pool_configuration_is_validated() {
    for broken in [
        PoolConfig {
            threads: 0,
            ..PoolConfig::default()
        },
        PoolConfig {
            queue_capacity: 0,
            ..PoolConfig::default()
        },
        PoolConfig {
            stack_bytes: 4096,
            ..PoolConfig::default()
        },
        PoolConfig {
            result_grace: Duration::ZERO,
            ..PoolConfig::default()
        },
        PoolConfig {
            threads: 10_000,
            ..PoolConfig::default()
        },
    ] {
        assert!(matches!(ScriptPool::new(broken), Err(PoolError::Config(_))));
    }
    let defaults = PoolConfig::default();
    assert_eq!(defaults.threads, 2);
    assert_eq!(defaults.stack_bytes, 8 * 1024 * 1024);
}

#[test]
fn invalid_limits_are_refused_before_queueing() {
    let pool = pool(1, 1);
    let mut broken = job(compile("()"));
    broken.limits.max_operations = 0;
    assert_eq!(pool.submit(broken).expect_err("refused").code(), "limits");
    assert_eq!(pool.queued(), 0);
}
