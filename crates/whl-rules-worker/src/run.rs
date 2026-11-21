use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;
use whl_capture::{RuleFacts, StoreState};
use whl_core::endpoint_id::EndpointId;
use whl_rules::{
    Action, Advance, Answer, DriveEnv, Effect, Entry, Failure, FailureCode, MAX_DETAIL_BYTES,
    MAX_LOG_BYTES, Machine, MachineInput, Mode, OutboundStep, Outcome, Resumption, RunView,
    STATE_STEP_TIMEOUT, STEP_BUDGET, StepEntry, StepKind, bound_output, bound_text,
    entries_from_value, fit_entries, perform as perform_local, render_message, rule_result_payload,
    side_effect_call, steps_to_value, timestamp,
};
use whl_secrets::Keyring;
use whl_store::rule_exec::{
    ClaimedFrom, ClaimedRun, Disposition, FinalState, RunSave, RunTrigger, SaveOutcome,
};
use whl_telemetry::series::{RULE_RUNS_TOTAL, RULE_STEPS_TOTAL};
use whl_template::{RandomSeed, StepData};

use crate::outbound::{Ctx, StepAttempt, perform};
use crate::prepare::{Deferred, Prep, Prepared, prepare};
use crate::worker::Shared;

const SEED_CONTEXT: &[u8] = b"whl-rules-after-seed";

struct Halt;

type Flow<T> = Result<T, Halt>;

pub(crate) async fn execute(shared: &Shared, run: &ClaimedRun) {
    if run.previous == ClaimedFrom::Sync {
        metrics::counter!(RULE_RUNS_TOTAL, "phase" => "async", "outcome" => "replayed")
            .increment(1);
    }
    let runner = Runner { shared, run };
    let _ = runner.process().await;
}

struct Runner<'a> {
    shared: &'a Shared,
    run: &'a ClaimedRun,
}

fn seed_for(
    keyring: Option<&Keyring>,
    endpoint: &EndpointId,
    capture: &Uuid,
    position: usize,
) -> Option<RandomSeed> {
    let mut material = Vec::with_capacity(SEED_CONTEXT.len() + 1 + 16 + 8);
    material.extend_from_slice(SEED_CONTEXT);
    material.push(0);
    material.extend_from_slice(capture.as_bytes());
    material.extend_from_slice(&u64::try_from(position).unwrap_or(u64::MAX).to_be_bytes());
    match keyring {
        Some(keyring) => {
            let (_, key) = keyring.active_capture_mac_key(endpoint).ok()?;
            let tag = key.tag(&material).ok()?;
            Some(RandomSeed::from_bytes(tag.unpack()))
        }
        None => Some(RandomSeed::from_bytes(&material)),
    }
}

fn trigger_name(trigger: RunTrigger) -> &'static str {
    match trigger {
        RunTrigger::Capture => "capture",
        RunTrigger::Rerun => "rerun",
    }
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn attempt_count(attempt: u32) -> i16 {
    i16::try_from(attempt).unwrap_or(i16::MAX)
}

impl Runner<'_> {
    async fn process(&self) -> Flow<()> {
        let prepared = match prepare(self.shared, self.run).await {
            Ok(prepared) => prepared,
            Err(Prep::Halt) => return Err(Halt),
            Err(Prep::Fail(failure)) => return self.fail_early(failure).await,
        };
        let Prepared {
            endpoint,
            detail,
            hop,
            facts,
            compiled,
            deferred,
            machine_start,
            payload,
            opened,
            scrub,
            context,
            log,
            steps,
        } = prepared;
        let ctx = Ctx {
            store: &self.shared.store,
            client: &self.shared.client,
            settings: &self.shared.settings,
            run: self.run,
            endpoint: &endpoint,
            detail: &detail,
            hop,
            opened: &opened,
            scrub: &scrub,
        };
        let count = deferred.len();
        let mut log = log;
        let mut steps = steps;
        let mut position = usize::try_from(self.run.next_step).unwrap_or(0);
        let mut retry = (self.run.step_attempts > 0)
            .then_some(u32::from(self.run.step_attempts.unsigned_abs()) + 1);

        while position < count {
            let Some(entry) = deferred.get(position) else {
                break;
            };
            let attempt = retry.take().unwrap_or(1);
            let started = Instant::now();
            let result = run_deferred(&ctx, &facts, entry, position, attempt).await;
            let elapsed = millis(started.elapsed());
            let step = &entry.step;
            let kind = StepKind::of(&step.action);
            match result {
                StepAttempt::Retry { after } => {
                    return self
                        .save(
                            position,
                            self.run.vars.clone(),
                            log,
                            steps_to_value(&steps),
                            Disposition::Retry {
                                after,
                                step_attempts: attempt_count(attempt),
                            },
                        )
                        .await;
                }
                StepAttempt::Done(done) => {
                    log.push(step_entry(
                        entry,
                        kind,
                        Outcome::Ok,
                        elapsed,
                        done.detail,
                        Some(done.output),
                    ));
                    count_step(kind, Outcome::Ok);
                    if let Some(name) = &step.name {
                        let mut data = done.data.unwrap_or_default();
                        data.duration_ms = elapsed;
                        steps.insert(name.clone(), data);
                    }
                }
                StepAttempt::Failed(failure) => {
                    log.push(step_entry(
                        entry,
                        kind,
                        Outcome::Failed,
                        elapsed,
                        Some(failure.detail.clone()),
                        None,
                    ));
                    count_step(kind, Outcome::Failed);
                    if let Some(name) = &step.name {
                        steps.insert(
                            name.clone(),
                            StepData {
                                duration_ms: elapsed,
                                error: Some(failure.code.as_str().to_owned()),
                                ..StepData::default()
                            },
                        );
                    }
                    if !step.continue_on_error {
                        return self
                            .finish(
                                position + 1,
                                self.run.vars.clone(),
                                log,
                                steps_to_value(&steps),
                                Some(failure),
                            )
                            .await;
                    }
                }
            }
            position += 1;
            let stop = *self.shared.stopping.borrow();
            self.save(
                position,
                self.run.vars.clone(),
                log.clone(),
                steps_to_value(&steps),
                self.progress(stop),
            )
            .await?;
            if stop {
                return Ok(());
            }
        }

        let Some(seed) = seed_for(
            self.shared.keyring.as_deref(),
            &endpoint,
            &self.run.capture_id,
            position,
        ) else {
            let failure = Failure::new(FailureCode::Internal, "the random seed cannot be derived");
            return self
                .finish(
                    position,
                    self.run.vars.clone(),
                    log,
                    steps_to_value(&steps),
                    Some(failure),
                )
                .await;
        };
        let input = MachineInput {
            rules: Arc::clone(&compiled),
            context,
            seed,
            budget: STEP_BUDGET,
            mode: Mode::After,
            tenant: endpoint.unpack().to_owned(),
        };
        let mut machine = Machine::resumed(
            input,
            Resumption {
                position: machine_start + position.saturating_sub(count),
                payload,
                steps,
                log,
            },
        );
        let state = StoreState {
            store: &self.shared.store,
            endpoint: &endpoint,
            retention: self.shared.settings.retention,
        };
        let env = DriveEnv {
            pool: self.shared.pool.as_deref(),
            state: &state,
            script_memory_bytes: self.shared.settings.script_memory_bytes,
            state_timeout: STATE_STEP_TIMEOUT,
            slice_hook: None,
        };
        let global = |absolute: usize| count + absolute.saturating_sub(machine_start);
        loop {
            let stepped = tokio::task::spawn_blocking(move || {
                let advance = machine.advance();
                (machine, advance)
            })
            .await;
            let (returned, advance) = match stepped {
                Ok(pair) => pair,
                Err(_) => {
                    tracing::error!(run_id = %self.run.id, "a rule step did not finish");
                    return Err(Halt);
                }
            };
            machine = returned;
            match advance {
                Advance::Checkpoint => {
                    let snapshot = machine.progress();
                    let stop = *self.shared.stopping.borrow();
                    self.save(
                        global(snapshot.position),
                        Value::Object(snapshot.vars),
                        snapshot.log,
                        steps_to_value(&snapshot.steps),
                        self.progress(stop),
                    )
                    .await?;
                    if stop {
                        return Ok(());
                    }
                }
                Advance::Finished(finished) => {
                    let finished = *finished;
                    let snapshot = machine.progress();
                    let failure = finished.failure().cloned();
                    return self
                        .finish(
                            global(snapshot.position),
                            Value::Object(finished.vars),
                            finished.log,
                            steps_to_value(&snapshot.steps),
                            failure,
                        )
                        .await;
                }
                Advance::Effect(Effect::Outbound(step)) => {
                    let position = global(step.position);
                    let attempt = retry.take().unwrap_or(1);
                    match perform(&ctx, &step, position, attempt).await {
                        StepAttempt::Retry { after } => {
                            let snapshot = machine.progress();
                            return self
                                .save(
                                    position,
                                    Value::Object(snapshot.vars),
                                    snapshot.log,
                                    steps_to_value(&snapshot.steps),
                                    Disposition::Retry {
                                        after,
                                        step_attempts: attempt_count(attempt),
                                    },
                                )
                                .await;
                        }
                        StepAttempt::Done(done) => {
                            machine.resume(Answer::Outbound(Box::new(Ok(done))));
                        }
                        StepAttempt::Failed(failure) => {
                            machine.resume(Answer::Outbound(Box::new(Err(failure))));
                        }
                    }
                }
                Advance::Effect(other) => {
                    let answer = perform_local(other, &env).await;
                    machine.resume(answer);
                }
            }
        }
    }

    fn progress(&self, stop: bool) -> Disposition {
        if stop {
            Disposition::Release { step_attempts: 0 }
        } else {
            Disposition::Continue {
                lease: self.shared.settings.lease,
            }
        }
    }

    async fn fail_early(&self, failure: Failure) -> Flow<()> {
        let log = entries_from_value(&self.run.log).unwrap_or_default();
        self.finish(
            usize::try_from(self.run.next_step).unwrap_or(0),
            self.run.vars.clone(),
            log,
            self.run.step_results.clone(),
            Some(failure),
        )
        .await
    }

    async fn save(
        &self,
        position: usize,
        vars: Value,
        log: Vec<Entry>,
        steps: Value,
        disposition: Disposition,
    ) -> Flow<()> {
        let entries = fit_entries(log, MAX_LOG_BYTES);
        self.write(position, vars, &entries, steps, disposition)
            .await
    }

    async fn write(
        &self,
        position: usize,
        vars: Value,
        entries: &[Entry],
        steps: Value,
        disposition: Disposition,
    ) -> Flow<()> {
        let log = serde_json::to_value(entries).map_err(|error| {
            tracing::error!(%error, run_id = %self.run.id, "the run log cannot be encoded");
            Halt
        })?;
        let save = RunSave {
            run: self.run.id,
            claim: self.run.claim,
            position: i32::try_from(position).unwrap_or(i32::MAX),
            vars,
            log,
            steps,
            disposition,
        };
        match self.shared.store.save_rule_run(&save).await {
            Ok(SaveOutcome::Saved) => Ok(()),
            Ok(SaveOutcome::LeaseLost) => {
                tracing::warn!(run_id = %self.run.id, "the rule run was taken over by another worker");
                Err(Halt)
            }
            Err(error) => {
                tracing::warn!(%error, run_id = %self.run.id, "the rule run cannot be saved; its lease will expire and a peer resumes it");
                Err(Halt)
            }
        }
    }

    async fn finish(
        &self,
        position: usize,
        vars: Value,
        log: Vec<Entry>,
        steps: Value,
        failure: Option<Failure>,
    ) -> Flow<()> {
        let entries = fit_entries(log, MAX_LOG_BYTES);
        let (state, name, error) = match &failure {
            Some(failure) => (FinalState::Failed, "failed", Some(failure.run_error())),
            None => (FinalState::Succeeded, "succeeded", None),
        };
        let view = RunView {
            id: self.run.id,
            capture_id: self.run.capture_id,
            trigger: trigger_name(self.run.trigger),
            state: name,
            rules_version: self.run.rules_version,
            response_status: self
                .run
                .response_status
                .and_then(|status| u16::try_from(status).ok()),
            error: error.clone(),
            entries: entries.clone(),
            created_at: self.run.created_at.clone(),
            finished_at: Some(timestamp(OffsetDateTime::now_utc())),
        };
        let event = match rule_result_payload(&self.run.endpoint_id, &view) {
            Ok(payload) => Some(payload),
            Err(error) => {
                tracing::error!(%error, run_id = %self.run.id, "the rule result event cannot be encoded");
                None
            }
        };
        self.write(
            position,
            vars,
            &entries,
            steps,
            Disposition::Finish {
                state,
                error,
                event,
            },
        )
        .await?;
        metrics::counter!(RULE_RUNS_TOTAL, "phase" => "async", "outcome" => name).increment(1);
        Ok(())
    }
}

fn count_step(kind: StepKind, outcome: Outcome) {
    metrics::counter!(
        RULE_STEPS_TOTAL,
        "kind" => kind.as_str(),
        "outcome" => outcome.as_str()
    )
    .increment(1);
}

fn step_entry(
    entry: &Deferred,
    kind: StepKind,
    outcome: Outcome,
    duration_ms: u64,
    detail: Option<String>,
    output: Option<Value>,
) -> Entry {
    Entry::Step(StepEntry {
        rule_id: entry.item.rule_id,
        step: entry.item.step,
        name: entry.step.name.clone(),
        kind: kind.as_str(),
        outcome,
        duration_ms,
        detail: detail.map(|text| bound_text(&text, MAX_DETAIL_BYTES)),
        output: output.map(bound_output),
    })
}

async fn run_deferred(
    ctx: &Ctx<'_>,
    facts: &RuleFacts,
    entry: &Deferred,
    position: usize,
    attempt: u32,
) -> StepAttempt {
    let step = &entry.step;
    let message = match &step.action {
        Action::Notify {
            message: Some(source),
            ..
        } => {
            let mut context = facts.context();
            context.vars = entry
                .snapshot
                .vars
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            let seed = RandomSeed::from_bytes(ctx.run.capture_id.as_bytes());
            match render_message(source, &context, &seed) {
                Ok(text) => Some(text),
                Err(failure) => return StepAttempt::Failed(failure),
            }
        }
        _ => None,
    };
    let call = match side_effect_call(&step.action, message, entry.snapshot.payload.as_deref()) {
        Ok(call) => call,
        Err(failure) => return StepAttempt::Failed(failure),
    };
    let outbound = OutboundStep {
        rule_id: entry.item.rule_id,
        rule_name: entry.item.rule_name.clone(),
        step: entry.item.step,
        position,
        name: step.name.clone(),
        kind: StepKind::of(&step.action),
        continue_on_error: step.continue_on_error,
        call,
    };
    perform(ctx, &outbound, position, attempt).await
}
