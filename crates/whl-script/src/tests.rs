use std::alloc::System;
use std::sync::mpsc::channel;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use rhai::Engine;
use time::OffsetDateTime;

use crate::pool::submit_extended;
use crate::{
    CountingAlloc, EngineLimits, MemoryWindow, PoolConfig, RandomSeed, RunLimits, ScriptCompiler,
    ScriptError, ScriptJob, ScriptPool, SeededRandom, SystemClock, TenantKey, allocator_installed,
    memory_exceeded,
};

#[global_allocator]
static ALLOCATOR: CountingAlloc<System> = CountingAlloc::new(System);

const MIB: usize = 1024 * 1024;

fn roomy() -> RunLimits {
    RunLimits {
        time: Duration::from_secs(20),
        ..RunLimits::default()
    }
}

fn allocate(bytes: usize) -> Vec<u8> {
    std::hint::black_box(Vec::<u8>::with_capacity(bytes))
}

fn hog_extension(engine: &mut Engine) {
    engine.register_fn("hog", |bytes: i64| {
        let block = vec![1u8; usize::try_from(bytes).unwrap_or(0)];
        std::hint::black_box(&block);
        i64::try_from(block.len()).unwrap_or(0)
    });
    engine.register_fn("boom", || -> i64 { panic!("host function failure") });
    engine.register_fn("thread_name", || {
        std::thread::current().name().unwrap_or("").to_owned()
    });
}

fn job(source: &str, limits: RunLimits) -> ScriptJob {
    let compiler = ScriptCompiler::new(EngineLimits::default()).expect("limits are valid");
    ScriptJob {
        tenant: TenantKey::new("unit"),
        script: compiler.compile(source).expect("script compiles"),
        request: serde_json::json!({}),
        vars: serde_json::Map::new(),
        now: OffsetDateTime::UNIX_EPOCH,
        limits,
        clock: Arc::new(SystemClock::new()),
        random: Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(b"unit"))),
    }
}

fn pool(threads: usize) -> ScriptPool {
    ScriptPool::new(PoolConfig {
        threads,
        ..PoolConfig::default()
    })
    .expect("pool starts")
}

#[test]
fn the_allocator_is_detected_as_installed() {
    assert!(allocator_installed());
}

#[test]
fn live_and_peak_bytes_follow_allocations_exactly() {
    let window = MemoryWindow::enter(usize::MAX);
    let before = window.live_bytes();
    let block = allocate(MIB);
    assert_eq!(window.live_bytes() - before, MIB);
    drop(block);
    assert_eq!(window.live_bytes(), before);
    assert!(window.peak_bytes() >= before + MIB);
}

#[test]
fn growing_a_buffer_is_counted_by_its_final_capacity() {
    let window = MemoryWindow::enter(usize::MAX);
    let mut buffer: Vec<u8> = Vec::new();
    buffer.reserve_exact(1000);
    let first = window.live_bytes();
    buffer.reserve_exact(5000);
    assert_eq!(window.live_bytes() - first, buffer.capacity() - 1000);
    drop(buffer);
    assert_eq!(window.live_bytes(), 0);
}

#[test]
fn allocation_outside_a_window_is_not_counted() {
    let block = allocate(MIB);
    let window = MemoryWindow::enter(usize::MAX);
    assert_eq!(window.live_bytes(), 0);
    drop(block);
    assert_eq!(window.live_bytes(), 0);
}

#[test]
fn exceeding_the_limit_sets_the_flag_and_a_new_window_starts_clean() {
    {
        let window = MemoryWindow::enter(64 * 1024);
        assert!(!window.exceeded());
        let block = allocate(128 * 1024);
        assert!(window.exceeded());
        assert!(memory_exceeded());
        drop(block);
        assert!(window.exceeded());
    }
    assert!(!memory_exceeded());
    let window = MemoryWindow::enter(64 * 1024);
    assert!(!window.exceeded());
    assert_eq!(window.live_bytes(), 0);
}

#[test]
fn nested_windows_restore_the_outer_account() {
    let outer = MemoryWindow::enter(usize::MAX);
    let kept = allocate(MIB);
    let outer_live = outer.live_bytes();
    {
        let inner = MemoryWindow::enter(usize::MAX);
        let block = allocate(2 * MIB);
        assert_eq!(inner.live_bytes(), 2 * MIB);
        drop(block);
    }
    assert_eq!(outer.live_bytes(), outer_live);
    drop(kept);
}

#[test]
fn memory_allocated_in_one_thread_and_freed_in_another_does_not_corrupt_either_account() {
    let (to_other, from_first) = channel::<Vec<u8>>();
    let (done_tx, done_rx) = channel::<usize>();
    let freeing = std::thread::spawn(move || {
        let window = MemoryWindow::enter(usize::MAX);
        let block = from_first.recv().expect("block arrives");
        drop(block);
        done_tx.send(window.live_bytes()).expect("report");
        window.live_bytes()
    });
    let allocating = std::thread::spawn(move || {
        let window = MemoryWindow::enter(usize::MAX);
        let base = window.live_bytes();
        let block = allocate(MIB);
        assert_eq!(window.live_bytes() - base, MIB);
        to_other.send(block).expect("send block");
        done_rx.recv().expect("other side finished");
        let still_counted = window.live_bytes();
        assert!(still_counted >= base + MIB);
        assert!(still_counted - base - MIB < 8192);
        let more = allocate(MIB);
        assert_eq!(window.live_bytes() - still_counted, MIB);
        drop(more);
        assert_eq!(window.live_bytes(), still_counted);
    });
    assert!(freeing.join().expect("freeing thread") < 8192);
    allocating.join().expect("allocating thread");
}

#[test]
fn concurrent_threads_keep_independent_exact_accounts() {
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (1..=threads)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let window = MemoryWindow::enter(usize::MAX);
                barrier.wait();
                let base = window.live_bytes();
                let size = index * 4096;
                for _ in 0..200 {
                    let first = allocate(size);
                    let second = allocate(size);
                    assert_eq!(window.live_bytes() - base, 2 * size);
                    drop(first);
                    drop(second);
                    assert_eq!(window.live_bytes(), base);
                }
                assert!(window.peak_bytes() >= base + 2 * size);
                assert!(window.peak_bytes() < base + 2 * size + 4096);
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("thread finishes");
    }
}

#[test]
fn a_host_function_that_allocates_past_the_budget_fails_the_run_without_taking_down_the_process() {
    let pool = pool(1);
    let limits = RunLimits {
        memory_bytes: MIB,
        ..roomy()
    };
    let result = submit_extended(
        &pool,
        job("hog(8 * 1024 * 1024); ()", limits),
        &hog_extension,
    )
    .expect("submitted")
    .wait();
    assert_eq!(result.expect_err("over budget"), ScriptError::Memory);

    let huge = submit_extended(
        &pool,
        job("hog(64 * 1024 * 1024); ()", roomy()),
        &hog_extension,
    )
    .expect("submitted")
    .wait();
    assert_eq!(
        huge.expect_err("over the default budget"),
        ScriptError::Memory
    );

    let follow_up = pool.run(job("#{ vars: #{ ok: true } }", roomy()));
    assert!(follow_up.is_ok());
}

#[test]
fn a_host_function_within_the_budget_succeeds() {
    let pool = pool(1);
    let result = submit_extended(
        &pool,
        job("#{ vars: #{ n: hog(1024 * 1024) } }", roomy()),
        &hog_extension,
    )
    .expect("submitted")
    .wait()
    .expect("run succeeds");
    assert_eq!(result.vars.get("n"), Some(&serde_json::json!(1024 * 1024)));
    assert!(result.stats.peak_memory_bytes >= MIB);
}

#[test]
fn a_panic_inside_a_script_run_is_reported_and_the_worker_survives() {
    let pool = pool(1);
    let result = submit_extended(&pool, job("boom(); ()", roomy()), &hog_extension)
        .expect("submitted")
        .wait();
    let error = result.expect_err("the host function panicked");
    assert_eq!(error, ScriptError::Panic);
    assert_eq!(error.code(), "script_panic");
    assert_eq!(error.outcome(), "panic");
    assert!(!memory_exceeded());

    let follow_up = pool.run(job("#{ vars: #{ alive: true } }", roomy()));
    assert!(follow_up.is_ok());
}

#[test]
fn scripts_run_on_the_dedicated_pool_threads_with_the_configured_stack() {
    let pool = pool(2);
    let output = submit_extended(
        &pool,
        job("#{ vars: #{ name: thread_name() } }", roomy()),
        &hog_extension,
    )
    .expect("submitted")
    .wait()
    .expect("run succeeds");
    let name = output.vars["name"].as_str().expect("thread name");
    assert!(name.starts_with("whl-script-"), "{name}");
    assert_ne!(std::thread::current().name(), Some(name));
}

#[test]
fn a_panic_outside_the_script_run_is_caught_by_the_worker() {
    let pool = pool(1);
    let pending = pool
        .submit_internal(Duration::from_secs(5), |_, _| -> Result<(), ScriptError> {
            panic!("task failure")
        })
        .expect("submitted");
    assert_eq!(pending.wait().expect_err("panicked"), ScriptError::Panic);
    let follow_up = pool.run(job("()", roomy()));
    assert!(follow_up.is_ok());
}

struct CountingClock {
    reads: std::sync::atomic::AtomicU64,
}

impl crate::MonotonicClock for CountingClock {
    fn elapsed(&self) -> Duration {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Duration::ZERO
    }
}

#[test]
fn the_clock_is_read_at_most_every_sixteenth_operation() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    assert_eq!(crate::DEADLINE_CHECK_INTERVAL, 16);
    let clock = Arc::new(CountingClock {
        reads: AtomicU64::new(0),
    });
    let guard = crate::guard::RunGuard::new(
        clock.clone(),
        Duration::from_secs(60),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        1 << 20,
    );
    for operations in 1..=1600_u64 {
        assert!(guard.progress(operations).is_none());
    }
    assert_eq!(clock.reads.load(Ordering::SeqCst), 100);
}

fn compile_on_small_stack(source: String) -> (Result<(), usize>, Duration) {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let compiler = ScriptCompiler::new(EngineLimits::default()).expect("limits are valid");
            let started = std::time::Instant::now();
            let outcome = compiler
                .compile(&source)
                .map(|_| ())
                .map_err(|error| error.diagnostics.len());
            (outcome, started.elapsed())
        })
        .expect("the thread starts")
        .join()
        .expect("the compiler does not overflow the stack")
}

#[test]
fn hostile_sources_are_refused_or_compiled_quickly_without_overflowing_a_small_stack() {
    let limit = EngineLimits::default().max_source_bytes;
    let sources = [
        "(".repeat(limit),
        "[".repeat(limit),
        "#{a:".repeat(limit / 4),
        "{".repeat(limit),
        "if true {".repeat(limit / 9),
        "!".repeat(limit),
        "-".repeat(limit),
        format!("let a = {};", "1+".repeat(limit / 2 - 8) + "1"),
        format!("let a = {}1;", "a.".repeat(limit / 2 - 16)),
        format!("let a = \"{}\";", "x".repeat(limit - 16)),
        "fn f(a){}\n".repeat(limit / 10),
        "let a = 1;\n".repeat(limit / 11),
        "\u{0}".repeat(limit),
        "\"".repeat(limit),
        "/*".repeat(limit / 2),
        format!(
            "{}1{}",
            "(".repeat(limit / 2 - 1),
            ")".repeat(limit / 2 - 1)
        ),
        format!(
            "{}1{}",
            "[".repeat(limit / 2 - 1),
            "]".repeat(limit / 2 - 1)
        ),
        format!(
            "{}1{}",
            "{".repeat(limit / 2 - 1),
            "}".repeat(limit / 2 - 1)
        ),
    ];
    for source in sources {
        let label: String = source.chars().take(12).collect();
        assert!(source.len() <= limit, "{label}");
        let (outcome, elapsed) = compile_on_small_stack(source);
        assert!(elapsed < Duration::from_secs(2), "{label}: {elapsed:?}");
        if let Err(count) = outcome {
            assert!(count >= 1, "{label}");
        }
    }
    let balanced = format!(
        "{}1{}",
        "(".repeat(limit / 2 - 1),
        ")".repeat(limit / 2 - 1)
    );
    assert_eq!(compile_on_small_stack(balanced).0, Err(1));
}

#[test]
fn a_source_over_the_limit_is_refused_before_it_is_parsed() {
    let limit = EngineLimits::default().max_source_bytes;
    let (outcome, elapsed) = compile_on_small_stack("(".repeat(limit + 1));
    assert_eq!(outcome, Err(1));
    assert!(elapsed < Duration::from_millis(200), "{elapsed:?}");
}
