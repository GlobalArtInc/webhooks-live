mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use serde_json::json;
use whl_script::{PoolConfig, PoolError, RunLimits, ScriptError, ScriptJob, ScriptPool, TenantKey};

fn spinner(time: Duration) -> ScriptJob {
    job_with(
        compile("loop {}"),
        default_request(),
        RunLimits {
            max_operations: u64::MAX / 2,
            time,
            ..roomy()
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
        max_queue_wait: Duration::from_millis(50),
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
                    let input = job_with(script.clone(), json!({ "n": n }), roomy());
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
        PoolConfig {
            max_queue_wait: Duration::ZERO,
            ..PoolConfig::default()
        },
        PoolConfig {
            stuck_grace: Duration::ZERO,
            ..PoolConfig::default()
        },
        PoolConfig {
            shutdown_timeout: Duration::ZERO,
            ..PoolConfig::default()
        },
        PoolConfig {
            max_abandoned_threads: 100_000,
            ..PoolConfig::default()
        },
        PoolConfig {
            max_running_per_tenant: 0,
            ..PoolConfig::default()
        },
        PoolConfig {
            max_queued_per_tenant: 0,
            ..PoolConfig::default()
        },
    ] {
        assert!(matches!(ScriptPool::new(broken), Err(PoolError::Config(_))));
    }
    let defaults = PoolConfig::default();
    assert_eq!(defaults.threads, 2);
    assert_eq!(defaults.stack_bytes, 8 * 1024 * 1024);
    assert_eq!(defaults.result_grace, Duration::from_secs(1));
    assert_eq!(defaults.max_queue_wait, Duration::from_secs(1));
    assert_eq!(defaults.stuck_grace, Duration::from_secs(2));
    assert_eq!(defaults.max_abandoned_threads, 8);
    assert_eq!(defaults.max_running_per_tenant, 1);
    assert_eq!(defaults.max_queued_per_tenant, 8);
}

fn tenant_job(tenant: &str, mut job: ScriptJob) -> ScriptJob {
    job.tenant = TenantKey::new(tenant);
    job
}

fn quick(tenant: &str) -> ScriptJob {
    tenant_job(tenant, job(compile("#{ vars: #{ ok: true } }")))
}

fn fair_pool(threads: usize, max_queue_wait: Duration) -> ScriptPool {
    ScriptPool::new(PoolConfig {
        threads,
        max_queue_wait,
        result_grace: Duration::from_secs(10),
        ..PoolConfig::default()
    })
    .expect("pool starts")
}

#[test]
fn the_execution_deadline_starts_when_a_worker_picks_the_job() {
    let pool = fair_pool(1, Duration::from_secs(10));
    let slow = pool
        .submit(tenant_job("slow", spinner(Duration::from_millis(1500))))
        .expect("first job");
    wait_until(|| pool.stats().busy == 1);
    let mut honest = quick("honest");
    honest.limits.time = Duration::from_millis(500);
    let queued = pool.submit(honest).expect("second job queues");
    assert_eq!(slow.wait().expect_err("deadline"), ScriptError::Time);
    let output = queued.wait().expect("the wait in the queue did not count");
    assert_eq!(output.vars["ok"], json!(true));
}

#[test]
fn a_job_that_waits_longer_than_the_queue_limit_is_rejected_with_queue_timeout() {
    let pool = fair_pool(1, Duration::from_millis(150));
    let blocker = pool
        .submit(tenant_job("blocker", spinner(Duration::from_secs(4))))
        .expect("first job");
    wait_until(|| pool.stats().busy == 1);
    let started = Instant::now();
    let error = pool
        .submit(quick("waiter"))
        .expect("queued")
        .wait()
        .expect_err("expired in the queue");
    assert_eq!(error, ScriptError::QueueTimeout);
    assert_eq!(error.code(), "queue_timeout");
    assert_eq!(error.outcome(), "time");
    assert!(
        started.elapsed() < Duration::from_millis(3500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(pool.queued(), 0);
    blocker.cancel();
}

#[test]
fn per_tenant_limits_refuse_the_surplus_and_leave_other_tenants_alone() {
    let pool = ScriptPool::new(PoolConfig {
        threads: 2,
        max_running_per_tenant: 1,
        max_queued_per_tenant: 2,
        max_queue_wait: Duration::from_secs(30),
        result_grace: Duration::from_secs(30),
        ..PoolConfig::default()
    })
    .expect("pool starts");
    let long = || tenant_job("flood", spinner(Duration::from_secs(20)));
    let running = pool.submit(long()).expect("running");
    wait_until(|| pool.stats().busy == 1);
    let held = [
        running,
        pool.submit(long()).expect("queued one"),
        pool.submit(long()).expect("queued two"),
    ];
    let refused = pool.submit(long()).expect_err("queue_full");
    assert_eq!(refused, ScriptError::QueueFull);
    assert_eq!(refused.code(), "queue_full");
    assert_eq!(pool.stats().busy, 1);
    assert_eq!(pool.queued(), 2);
    let output = pool
        .run(quick("other"))
        .expect("another tenant is served at once");
    assert_eq!(output.vars["ok"], json!(true));
    for pending in &held {
        pending.cancel();
    }
}

#[test]
fn a_flooding_tenant_does_not_starve_an_honest_one() {
    let pool = Arc::new(fair_pool(2, Duration::from_secs(1)));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flooder = {
        let pool = Arc::clone(&pool);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut refused = 0_u32;
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                match pool.submit(tenant_job("flood", spinner(Duration::from_millis(100)))) {
                    Ok(pending) => kept.push(pending),
                    Err(ScriptError::QueueFull) => refused += 1,
                    Err(error) => panic!("unexpected {error:?}"),
                }
                std::thread::sleep(Duration::from_millis(3));
            }
            refused
        })
    };
    wait_until(|| pool.stats().busy >= 1);
    let mut slowest = Duration::ZERO;
    for _ in 0..12 {
        let started = Instant::now();
        let output = pool.run(quick("honest")).expect("honest runs are served");
        assert_eq!(output.vars["ok"], json!(true));
        slowest = slowest.max(started.elapsed());
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let refused = flooder.join().expect("flooder finishes");
    assert!(refused > 0, "the flood was never refused");
    assert!(slowest < Duration::from_secs(2), "{slowest:?}");
    assert_eq!(pool.stats().stuck_workers, 0);
}

#[test]
fn queued_jobs_are_taken_round_robin_across_tenants() {
    let pool = Arc::new(fair_pool(1, Duration::from_secs(30)));
    let blocker = pool
        .submit(tenant_job("blocker", spinner(Duration::from_millis(1500))))
        .expect("occupies the worker");
    wait_until(|| pool.stats().busy == 1);
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut waiters = Vec::new();
    for (tenant, label) in [("a", "a1"), ("a", "a2"), ("a", "a3"), ("b", "b1")] {
        let pending = pool
            .submit(tenant_job(tenant, spinner(Duration::from_millis(150))))
            .expect("queued");
        let order = Arc::clone(&order);
        waiters.push(std::thread::spawn(move || {
            let result = pending.wait();
            order.lock().expect("lock").push(label);
            result
        }));
    }
    assert_eq!(blocker.wait().expect_err("deadline"), ScriptError::Time);
    for waiter in waiters {
        assert_eq!(
            waiter.join().expect("waiter").expect_err("deadline"),
            ScriptError::Time
        );
    }
    let order = order.lock().expect("lock").clone();
    assert_eq!(order, vec!["a1", "b1", "a2", "a3"]);
}

#[test]
fn two_pools_do_not_share_state() {
    let first = ScriptPool::new(PoolConfig {
        threads: 1,
        max_queued_per_tenant: 1,
        ..PoolConfig::default()
    })
    .expect("first pool");
    let second = fair_pool(1, Duration::from_secs(1));
    let hold = first
        .submit(tenant_job("shared", spinner(Duration::from_secs(20))))
        .expect("running in the first pool");
    wait_until(|| first.stats().busy == 1);
    let queued = first
        .submit(tenant_job("shared", spinner(Duration::from_secs(20))))
        .expect("queued in the first pool");
    assert_eq!(
        first
            .submit(tenant_job("shared", spinner(Duration::from_secs(1))))
            .expect_err("the first pool is full for this tenant"),
        ScriptError::QueueFull
    );
    let output = second
        .run(quick("shared"))
        .expect("the second pool does not see the first one's tenant limits");
    assert_eq!(output.vars["ok"], json!(true));
    assert_eq!(second.stats().stuck_workers, 0);
    assert_eq!(second.queued(), 0);
    first.shutdown();
    assert_eq!(
        second.run(quick("shared")).expect("still alive").vars["ok"],
        json!(true)
    );
    hold.cancel();
    queued.cancel();
}

#[test]
fn invalid_limits_are_refused_before_queueing() {
    let pool = pool(1, 1);
    let mut broken = job(compile("()"));
    broken.limits.max_operations = 0;
    assert_eq!(pool.submit(broken).expect_err("refused").code(), "limits");
    assert_eq!(pool.queued(), 0);
}
