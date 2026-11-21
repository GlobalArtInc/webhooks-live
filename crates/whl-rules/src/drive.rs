use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use whl_script::{
    AFTER_RESPONSE_TIME, BEFORE_RESPONSE_TIME, RunLimits, ScriptError, ScriptJob, ScriptOutput,
    ScriptPhase, ScriptPool, SeededRandom, SystemClock, TenantKey,
};
use whl_telemetry::series::{SCRIPT_DURATION_SECONDS, SCRIPT_RUNS_TOTAL};

use crate::failure::{Failure, FailureCode};
use crate::machine::{
    Advance, Answer, Effect, Finished, Machine, ScriptCall, StateError, StoreCall, StoreValue,
};

pub const STATE_STEP_TIMEOUT: Duration = Duration::from_millis(200);
const SCRIPT_POLL: Duration = Duration::from_millis(1);
const OUTCOME_OK: &str = "ok";

pub trait StateBackend: Send + Sync {
    fn write(&self, call: StoreCall)
    -> impl Future<Output = Result<StoreValue, StateError>> + Send;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoState;

impl StateBackend for NoState {
    async fn write(&self, _call: StoreCall) -> Result<StoreValue, StateError> {
        Err(StateError::Unavailable)
    }
}

pub type SliceHook = Arc<dyn Fn() + Send + Sync>;

pub struct DriveEnv<'a, S: StateBackend> {
    pub pool: Option<&'a ScriptPool>,
    pub state: &'a S,
    pub script_memory_bytes: usize,
    pub state_timeout: Duration,
    pub slice_hook: Option<SliceHook>,
}

pub async fn drive<S: StateBackend, G: Send + 'static>(
    machine: Machine,
    env: &DriveEnv<'_, S>,
    budget: Duration,
    guard: G,
) -> Finished {
    match tokio::time::timeout(budget, run(machine, env, guard)).await {
        Ok(finished) => finished,
        Err(_) => Finished::failed(Failure::new(
            FailureCode::Budget,
            "the time budget for rules before the response is spent",
        )),
    }
}

async fn run<S: StateBackend, G: Send + 'static>(
    mut machine: Machine,
    env: &DriveEnv<'_, S>,
    mut guard: G,
) -> Finished {
    loop {
        let hook = env.slice_hook.clone();
        let stepped = tokio::task::spawn_blocking(move || {
            if let Some(hook) = hook {
                hook();
            }
            let advance = machine.advance();
            (machine, advance, guard)
        })
        .await;
        let (returned, advance, kept) = match stepped {
            Ok(pair) => pair,
            Err(_) => {
                return Finished::failed(Failure::new(
                    FailureCode::Internal,
                    "a rule step did not finish",
                ));
            }
        };
        machine = returned;
        guard = kept;
        match advance {
            Advance::Finished(finished) => return *finished,
            Advance::Checkpoint => {}
            Advance::Effect(effect) => {
                let answer = perform(effect, env).await;
                machine.resume(answer);
            }
        }
    }
}

pub async fn perform<S: StateBackend>(effect: Effect, env: &DriveEnv<'_, S>) -> Answer {
    match effect {
        Effect::Store(call) => {
            let outcome = tokio::time::timeout(env.state_timeout, env.state.write(call)).await;
            Answer::Store(outcome.unwrap_or(Err(StateError::Timeout)))
        }
        Effect::Script(call) => {
            let result = run_script(env.pool, env.script_memory_bytes, *call).await;
            Answer::Script(Box::new(result))
        }
        Effect::Outbound(_) => Answer::Outbound(Box::new(Err(Failure::new(
            FailureCode::Internal,
            "an outbound step cannot run in this context",
        )))),
    }
}

async fn run_script(
    pool: Option<&ScriptPool>,
    memory_bytes: usize,
    call: ScriptCall,
) -> Result<ScriptOutput, ScriptError> {
    let Some(pool) = pool else {
        return Err(ScriptError::ShutDown);
    };
    let ceiling = match call.phase {
        ScriptPhase::BeforeResponse => BEFORE_RESPONSE_TIME,
        ScriptPhase::AfterResponse => AFTER_RESPONSE_TIME,
    };
    let limits = RunLimits {
        memory_bytes,
        ..RunLimits::for_phase(call.phase, call.remaining.min(ceiling))
    };
    let job = ScriptJob {
        tenant: TenantKey::new(&call.tenant),
        script: call.script,
        request: call.request,
        vars: call.vars,
        now: call.now,
        limits,
        clock: Arc::new(SystemClock::new()),
        random: Box::new(SeededRandom::from_seed(&call.seed)),
    };
    let started = Instant::now();
    let result = match pool.submit(job) {
        Ok(pending) => loop {
            if let Some(result) = pending.try_result() {
                break result;
            }
            tokio::time::sleep(SCRIPT_POLL).await;
        },
        Err(error) => Err(error),
    };
    let outcome = match &result {
        Ok(_) => OUTCOME_OK,
        Err(error) => error.outcome(),
    };
    metrics::counter!(SCRIPT_RUNS_TOTAL, "outcome" => outcome).increment(1);
    metrics::histogram!(SCRIPT_DURATION_SECONDS).record(started.elapsed());
    result
}
