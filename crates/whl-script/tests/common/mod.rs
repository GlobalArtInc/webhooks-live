#![allow(dead_code)]

use std::alloc::System;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_script::{
    CompiledScript, CountingAlloc, EngineLimits, MonotonicClock, PoolConfig, RandomSeed, RunLimits,
    ScriptCompiler, ScriptError, ScriptJob, ScriptOutput, ScriptPool, SeededRandom, SystemClock,
    TenantKey,
};

#[global_allocator]
static ALLOCATOR: CountingAlloc<System> = CountingAlloc::new(System);

pub struct SteppingClock {
    nanos: AtomicU64,
    step: u64,
}

impl SteppingClock {
    pub fn new(step: Duration) -> Self {
        Self {
            nanos: AtomicU64::new(0),
            step: u64::try_from(step.as_nanos()).unwrap_or(u64::MAX),
        }
    }

    pub fn calls(&self) -> u64 {
        self.nanos.load(Ordering::SeqCst) / self.step.max(1)
    }
}

impl MonotonicClock for SteppingClock {
    fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.nanos.fetch_add(self.step, Ordering::SeqCst))
    }
}

pub struct FrozenClock;

impl MonotonicClock for FrozenClock {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
}

pub fn roomy() -> RunLimits {
    RunLimits {
        time: Duration::from_secs(20),
        ..RunLimits::default()
    }
}

pub fn pool(threads: usize, queue_capacity: usize) -> ScriptPool {
    ScriptPool::new(PoolConfig {
        threads,
        queue_capacity,
        result_grace: Duration::from_secs(10),
        compile_timeout: Duration::from_secs(20),
        max_running_per_tenant: threads,
        max_queued_per_tenant: queue_capacity,
        ..PoolConfig::default()
    })
    .expect("pool starts")
}

pub fn compiler() -> ScriptCompiler {
    ScriptCompiler::new(EngineLimits::default()).expect("limits are valid")
}

pub fn compile(source: &str) -> CompiledScript {
    compiler().compile(source).expect("script compiles")
}

pub fn default_request() -> Value {
    json!({
        "method": "POST",
        "path": "/hook",
        "headers": {"x-event": "push"},
        "body": "{\"a\":1}",
    })
}

pub fn job_with(script: CompiledScript, request: Value, limits: RunLimits) -> ScriptJob {
    ScriptJob {
        tenant: TenantKey::new("tenant"),
        script,
        request,
        vars: serde_json::Map::new(),
        now: OffsetDateTime::UNIX_EPOCH,
        limits,
        clock: Arc::new(SystemClock::new()),
        random: Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(b"seed"))),
    }
}

pub fn job(script: CompiledScript) -> ScriptJob {
    job_with(script, default_request(), roomy())
}

pub fn run(pool: &ScriptPool, source: &str) -> Result<ScriptOutput, ScriptError> {
    pool.run(job(compile(source)))
}

pub fn run_with(
    pool: &ScriptPool,
    source: &str,
    limits: RunLimits,
) -> Result<ScriptOutput, ScriptError> {
    pool.run(job_with(compile(source), default_request(), limits))
}

pub fn vars_of(output: &ScriptOutput) -> Value {
    Value::Object(output.vars.clone())
}
