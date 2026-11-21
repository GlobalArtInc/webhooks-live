use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rhai::Engine;
use time::OffsetDateTime;

use crate::pool::submit_extended;
use crate::{
    EngineLimits, PoolConfig, RandomSeed, RunLimits, ScriptCompiler, ScriptError, ScriptJob,
    ScriptPool, SeededRandom, SystemClock, TenantKey,
};

macro_rules! gated_blocker {
    ($gate:ident, $entered:ident, $extension:ident) => {
        static $gate: AtomicBool = AtomicBool::new(false);
        static $entered: AtomicBool = AtomicBool::new(false);

        fn $extension(engine: &mut Engine) {
            engine.register_fn("block", || {
                $entered.store(true, Ordering::SeqCst);
                while !$gate.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                1_i64
            });
        }
    };
}

gated_blocker!(GATE_REPLACE, ENTERED_REPLACE, replace_extension);
gated_blocker!(GATE_BOUND, ENTERED_BOUND, bound_extension);
gated_blocker!(GATE_SHUTDOWN, ENTERED_SHUTDOWN, shutdown_extension);

fn limits(time: Duration) -> RunLimits {
    RunLimits {
        time,
        ..RunLimits::default()
    }
}

fn job(source: &str, time: Duration) -> ScriptJob {
    let compiler = ScriptCompiler::new(EngineLimits::default()).expect("limits are valid");
    ScriptJob {
        tenant: TenantKey::new("stuck"),
        script: compiler.compile(source).expect("script compiles"),
        request: serde_json::json!({}),
        vars: serde_json::Map::new(),
        now: OffsetDateTime::UNIX_EPOCH,
        limits: limits(time),
        clock: Arc::new(SystemClock::new()),
        random: Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(b"stuck"))),
    }
}

fn blocker() -> ScriptJob {
    job("block(); ()", Duration::from_millis(50))
}

fn honest() -> ScriptJob {
    job("#{ vars: #{ alive: true } }", Duration::from_secs(20))
}

fn config(threads: usize, max_abandoned_threads: usize) -> PoolConfig {
    PoolConfig {
        threads,
        max_abandoned_threads,
        stuck_grace: Duration::from_millis(100),
        shutdown_timeout: Duration::from_millis(300),
        result_grace: Duration::from_millis(100),
        max_queue_wait: Duration::from_millis(200),
        max_running_per_tenant: threads,
        ..PoolConfig::default()
    }
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "never happened: {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_worker_that_ignores_the_deadline_is_replaced_and_the_pool_keeps_serving() {
    let pool = ScriptPool::new(config(1, 4)).expect("pool starts");
    let stuck = submit_extended(&pool, blocker(), &replace_extension).expect("submitted");
    wait_for("the worker is declared stuck", || {
        pool.stats().stuck_workers == 1
    });
    assert_eq!(
        stuck.wait().expect_err("no result"),
        ScriptError::ResultTimeout
    );
    let stats = pool.stats();
    assert_eq!(stats.workers, 1);
    assert_eq!(stats.abandoned_threads, 1);

    let output = pool.run(honest()).expect("served by the replacement");
    assert_eq!(output.vars["alive"], serde_json::json!(true));

    GATE_REPLACE.store(true, Ordering::SeqCst);
    wait_for("the abandoned thread exits", || {
        pool.stats().abandoned_threads == 0
    });
    let output = pool.run(honest()).expect("accounting stays consistent");
    assert_eq!(output.vars["alive"], serde_json::json!(true));
    let stats = pool.stats();
    assert_eq!(stats.stuck_workers, 1);
    assert_eq!(stats.workers, 1);
    assert_eq!(stats.busy, 0);
}

#[test]
fn the_number_of_abandoned_threads_is_bounded_and_the_pool_refuses_work_beyond_it() {
    let pool = ScriptPool::new(config(1, 2)).expect("pool starts");
    let mut stuck = Vec::new();
    for round in 1..=3_u64 {
        stuck.push(submit_extended(&pool, blocker(), &bound_extension).expect("submitted"));
        wait_for("another worker is declared stuck", || {
            pool.stats().stuck_workers == round
        });
    }
    let stats = pool.stats();
    assert_eq!(stats.abandoned_threads, 3);
    assert_eq!(stats.workers, 0);
    let refused = pool.submit(honest()).expect_err("unhealthy");
    assert_eq!(refused, ScriptError::PoolUnhealthy);
    assert_eq!(refused.code(), "pool_unhealthy");
    assert_eq!(refused.outcome(), "error");

    GATE_BOUND.store(true, Ordering::SeqCst);
    wait_for("the pool recovers once the threads finish", || {
        pool.run(honest()).is_ok()
    });
    wait_for("every abandoned thread exits", || {
        pool.stats().abandoned_threads == 0
    });
    let stats = pool.stats();
    assert_eq!(stats.workers, 1);
    assert_eq!(stats.stuck_workers, 3);
}

#[test]
fn shutdown_returns_within_its_timeout_while_a_worker_is_stuck() {
    let pool = ScriptPool::new(config(2, 4)).expect("pool starts");
    let stuck = submit_extended(&pool, blocker(), &shutdown_extension).expect("submitted");
    wait_for("the host function is running", || {
        ENTERED_SHUTDOWN.load(Ordering::SeqCst)
    });
    let started = Instant::now();
    pool.shutdown();
    let elapsed = started.elapsed();
    assert!(!GATE_SHUTDOWN.load(Ordering::SeqCst));
    assert!(elapsed >= Duration::from_millis(250), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    let started = Instant::now();
    drop(pool);
    assert!(started.elapsed() < Duration::from_secs(1));
    GATE_SHUTDOWN.store(true, Ordering::SeqCst);
    assert!(stuck.wait().is_err());
}
