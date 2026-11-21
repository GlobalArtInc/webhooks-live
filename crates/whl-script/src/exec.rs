use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rhai::{Dynamic, Engine, EvalAltResult, Position, Scope};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::alloc::MemoryWindow;
use crate::compiled::CompiledScript;
use crate::convert::{ConvertLimits, fingerprint, json_to_dynamic, object_to_dynamic};
use crate::engine::{REQUEST_VARIABLE, VARS_VARIABLE, build_engine};
use crate::env::{MonotonicClock, RandomSource};
use crate::error::{LinePosition, ScriptError, bounded_message};
use crate::guard::{Abort, RunGuard};
use crate::host::{HostLimits, HostState, register};
use crate::limits::RunLimits;
use crate::output::{LogBuffer, RunStats, ScriptOutput, extract};

const INPUT_MAX_DEPTH: usize = 32;
const INPUT_MAX_NODES: usize = 50_000;

pub struct ScriptJob {
    pub script: CompiledScript,
    pub request: Value,
    pub vars: serde_json::Map<String, Value>,
    pub now: OffsetDateTime,
    pub limits: RunLimits,
    pub clock: Arc<dyn MonotonicClock>,
    pub random: Box<dyn RandomSource>,
}

impl std::fmt::Debug for ScriptJob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptJob")
            .field("script", &self.script)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

pub(crate) type Extension<'a> = &'a (dyn Fn(&mut Engine) + Sync);

pub(crate) fn no_extension(_: &mut Engine) {}

fn position_of(position: Position) -> Option<LinePosition> {
    let line = position.line()?;
    Some(LinePosition {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: position
            .position()
            .map_or(1, |column| u32::try_from(column).unwrap_or(u32::MAX)),
    })
}

fn abort_error(abort: Abort) -> ScriptError {
    match abort {
        Abort::Time => ScriptError::Time,
        Abort::Memory => ScriptError::Memory,
        Abort::Cancelled => ScriptError::Cancelled,
        Abort::HostBudget => ScriptError::HostBudget,
    }
}

fn map_eval_error(error: &EvalAltResult, guard: &RunGuard) -> ScriptError {
    if let Some(abort) = guard.cause() {
        return abort_error(abort);
    }
    match error {
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => map_eval_error(inner, guard),
        EvalAltResult::ErrorTooManyOperations(_) => ScriptError::Operations,
        EvalAltResult::ErrorStackOverflow(_) => ScriptError::CallDepth,
        EvalAltResult::ErrorTerminated(..) => ScriptError::Cancelled,
        EvalAltResult::ErrorTooManyVariables(position) => ScriptError::TooManyVariables {
            position: position_of(*position),
        },
        EvalAltResult::ErrorDataTooLarge(what, position) => ScriptError::DataTooLarge {
            message: bounded_message(what),
            position: position_of(*position),
        },
        EvalAltResult::ErrorFunctionNotFound(name, position) => ScriptError::UndefinedFunction {
            name: bounded_message(name),
            position: position_of(*position),
        },
        EvalAltResult::ErrorVariableNotFound(name, position) => ScriptError::UndefinedVariable {
            name: bounded_message(name),
            position: position_of(*position),
        },
        EvalAltResult::ErrorRuntime(value, position) => ScriptError::Runtime {
            message: bounded_message(&value.to_string()),
            position: position_of(*position),
        },
        other => ScriptError::Runtime {
            message: bounded_message(&other.to_string()),
            position: position_of(other.position()),
        },
    }
}

fn input_limits(run: &RunLimits) -> ConvertLimits {
    ConvertLimits {
        max_depth: INPUT_MAX_DEPTH,
        max_nodes: INPUT_MAX_NODES,
        max_bytes: run.max_input_bytes,
        max_string_bytes: run.max_input_bytes,
        max_array_len: usize::MAX,
        max_map_len: usize::MAX,
    }
}

struct RandomLease<'a> {
    slot: &'a mut Box<dyn RandomSource>,
    state: Arc<HostState>,
}

impl<'a> RandomLease<'a> {
    fn new(slot: &'a mut Box<dyn RandomSource>, state: Arc<HostState>) -> Self {
        state.swap_random(slot);
        Self { slot, state }
    }
}

impl Drop for RandomLease<'_> {
    fn drop(&mut self) {
        self.state.swap_random(self.slot);
    }
}

fn run(
    job: &mut ScriptJob,
    deadline: Duration,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    extension: Extension<'_>,
) -> Result<ScriptOutput, ScriptError> {
    let limits = job.limits;
    let engine_limits = *job.script.limits();
    let started = job.clock.elapsed();
    let guard = Arc::new(RunGuard::new(
        Arc::clone(&job.clock),
        deadline,
        cancel,
        stop,
        limits.max_host_bytes,
    ));
    if let Some(abort) = guard.poll() {
        return Err(abort_error(abort));
    }
    if !job.request.is_object() {
        return Err(ScriptError::InvalidInput(
            "request must be a JSON object".to_owned(),
        ));
    }
    let request = json_to_dynamic(&job.request, input_limits(&limits))
        .map_err(|error| ScriptError::InvalidInput(format!("request: {error}")))?;
    let vars = object_to_dynamic(&job.vars, input_limits(&limits))
        .map_err(|error| ScriptError::InvalidInput(format!("vars: {error}")))?;
    let now = job
        .now
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| ScriptError::InvalidInput("now cannot be formatted".to_owned()))?;

    let log = Arc::new(Mutex::new(LogBuffer::new(limits.log_bytes)));
    let mut engine = build_engine(&engine_limits);
    engine.set_max_operations(limits.max_operations);
    let progress = Arc::clone(&guard);
    engine.on_progress(move |operations| progress.progress(operations));
    let printed = Arc::clone(&log);
    engine.on_print(move |text| {
        printed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_line(text);
    });
    let debugged = Arc::clone(&log);
    engine.on_debug(move |text, _, position| {
        let line = position.line().unwrap_or(0);
        debugged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_line(&format!("[debug line {line}] {text}"));
    });
    let state = Arc::new(HostState::new(
        Arc::clone(&guard),
        now,
        HostLimits {
            max_string_bytes: engine_limits.max_string_bytes,
            max_array_size: engine_limits.max_array_size,
            max_map_size: engine_limits.max_map_size,
            max_text_bytes: limits.max_host_text_bytes,
            regex_size_limit_bytes: limits.regex_size_limit_bytes,
        },
    ));
    let _lease = RandomLease::new(&mut job.random, Arc::clone(&state));
    register(&mut engine, &state);
    extension(&mut engine);

    let request_fingerprint = fingerprint(&request);
    let mut scope = Scope::new();
    scope.push_constant_dynamic(REQUEST_VARIABLE, request);
    scope.push_dynamic(VARS_VARIABLE, vars);
    let evaluated = engine.eval_ast_with_scope::<Dynamic>(&mut scope, job.script.ast());
    let returned = match evaluated {
        Ok(value) => value,
        Err(error) => return Err(map_eval_error(&error, &guard)),
    };
    let request_after = scope
        .get_value::<Dynamic>(REQUEST_VARIABLE)
        .and_then(|value| fingerprint(&value));
    if request_fingerprint.is_none() || request_after != request_fingerprint {
        return Err(ScriptError::Runtime {
            message: "the script modified the read-only request".to_owned(),
            position: None,
        });
    }
    let scope_vars = scope.get_value::<Dynamic>(VARS_VARIABLE);
    let extracted = extract(&returned, scope_vars.as_ref(), &engine_limits, &limits)?;
    let log = log.lock().unwrap_or_else(PoisonError::into_inner).finish();
    Ok(ScriptOutput {
        response: extracted.response,
        vars: extracted.vars,
        log,
        stats: RunStats {
            operations: guard.operations(),
            peak_memory_bytes: 0,
            elapsed: guard.elapsed().saturating_sub(started),
        },
    })
}

pub(crate) fn execute(
    job: &mut ScriptJob,
    deadline: Duration,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    extension: Extension<'_>,
) -> Result<ScriptOutput, ScriptError> {
    let window = MemoryWindow::enter(job.limits.memory_bytes);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        run(job, deadline, cancel, stop, extension)
    }));
    let peak = window.peak_bytes();
    let exceeded = window.exceeded();
    match outcome {
        Err(_) => Err(ScriptError::Panic),
        Ok(_) if exceeded => Err(ScriptError::Memory),
        Ok(Err(error)) => Err(error),
        Ok(Ok(mut output)) => {
            output.stats.peak_memory_bytes = peak;
            Ok(output)
        }
    }
}
