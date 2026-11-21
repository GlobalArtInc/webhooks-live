use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::random_bytes;
use whl_rules::{
    CompileError, CompiledRules, DriveEnv, Failure, FailureCode, Finished, MAX_LOG_BYTES,
    MAX_REGEX_DFA_BYTES, Machine, MachineInput, Mode, NoState, References, ResponseDraft,
    RuleError, RuleSource, RunView, STATE_STEP_TIMEOUT, StoredRule, drive, fit_entries,
    load_stored, rule_result_payload, rules_from_plan, timestamp, validate_set,
};
use whl_script::{DEFAULT_MEMORY_BYTES, EngineLimits, LimitsError, ScriptCompiler, ScriptPool};
use whl_secrets::Keyring;
use whl_store::Store;
use whl_store::Uuid;
use whl_store::captures::{Endpoint, StoredCapture};
use whl_store::rule_runs::{
    FinishOutcome, PLAN_LOAD_DEADLINE, PlanWrite, RuleRunSeed, SYNC_WRITE_DEADLINE, SyncResult,
    SyncState,
};
use whl_store::vars::VarStoreError;
use whl_telemetry::series::{RULE_RUNS_TOTAL, RULE_SYNC_DURATION_SECONDS};
use whl_template::{Context, RandomSeed};

use super::cache::{DEFAULT_CACHE_BYTES, RuleCache};
use super::facts::RuleFacts;
use super::state::StoreState;

pub const DEFAULT_SYNC_BUDGET: Duration = Duration::from_secs(3);
pub const MAX_SYNC_BUDGET: Duration = Duration::from_secs(10);
pub const DEFAULT_SYNC_CONCURRENCY: usize = 256;
pub const DEFAULT_PER_ENDPOINT: usize = 8;
pub const DEFAULT_DRY_RUN_CONCURRENCY: usize = 4;
pub const DEFAULT_VARIABLES_DEADLINE: Duration = Duration::from_millis(50);
const DRY_RUN_VARIABLES_DEADLINE: Duration = Duration::from_millis(500);
const WRITE_RESERVE: Duration = Duration::from_millis(500);
const COMPILE_SLOTS: usize = 4;
const SEED_KEY_BYTES: usize = 32;
const RECORD_SLOTS: usize = 32;
const SEED_CONTEXT: &[u8] = b"whl-rules-seed";
const PHASE: &str = "sync";
const OUTCOME_SUCCEEDED: &str = "succeeded";
const OUTCOME_FAILED: &str = "failed";
const OUTCOME_OVERLOADED: &str = "overloaded";
const TRIGGER_CAPTURE: &str = "capture";
const TRIGGER_DRY_RUN: &str = "dry_run";
const STATE_QUEUED: &str = "queued";
const STATE_SUCCEEDED: &str = "succeeded";
const STATE_FAILED: &str = "failed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulesSettings {
    pub enabled: bool,
    pub sync_budget: Duration,
    pub sync_concurrency: usize,
    pub per_endpoint: usize,
    pub dry_run_concurrency: usize,
    pub cache_bytes: usize,
    pub variables_deadline: Duration,
    pub plan_deadline: Duration,
    pub state_timeout: Duration,
    pub script_memory_bytes: usize,
}

impl Default for RulesSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sync_budget: DEFAULT_SYNC_BUDGET,
            sync_concurrency: DEFAULT_SYNC_CONCURRENCY,
            per_endpoint: DEFAULT_PER_ENDPOINT,
            dry_run_concurrency: DEFAULT_DRY_RUN_CONCURRENCY,
            cache_bytes: DEFAULT_CACHE_BYTES,
            variables_deadline: DEFAULT_VARIABLES_DEADLINE,
            plan_deadline: PLAN_LOAD_DEADLINE,
            state_timeout: STATE_STEP_TIMEOUT,
            script_memory_bytes: DEFAULT_MEMORY_BYTES,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("the rules sync budget must be above zero and at most {} ms", MAX_SYNC_BUDGET.as_millis())]
    Budget,
    #[error(
        "the rules concurrency limits must be between 1 and {}",
        Semaphore::MAX_PERMITS
    )]
    Concurrency,
    #[error("the script engine limits are invalid: {0}")]
    Limits(#[from] LimitsError),
    #[error("the rule cache budget must be above zero")]
    Cache,
    #[error("the random seed key cannot be generated")]
    Entropy,
}

#[derive(Debug, thiserror::Error)]
pub enum DryRunError {
    #[error("the dry run slots are busy")]
    Busy,
    #[error("the draft rules are invalid")]
    Invalid(RuleError),
    #[error("the stored rules no longer validate")]
    Stored(RuleError),
    #[error("the rules cannot be compiled: {0}")]
    Compile(#[from] CompileError),
    #[error("the rules engine is switched off")]
    Disabled,
    #[error("the dry run could not be set up")]
    Internal,
}

pub enum DryRunRules {
    Saved(Vec<StoredRule>),
    Drafts {
        rules: Vec<Value>,
        references: References,
    },
}

pub struct DryRunInput {
    pub endpoint: EndpointId,
    pub facts: RuleFacts,
    pub rules: DryRunRules,
    pub rules_version: i32,
    pub capture_id: Uuid,
}

pub(crate) struct LiveInput<'a> {
    pub endpoint: &'a Endpoint,
    pub capture: &'a StoredCapture,
    pub seed: RuleRunSeed,
    pub retention: Duration,
    pub status_override: Option<u16>,
    pub preflight: bool,
    pub keyring: Option<&'a Keyring>,
}

#[derive(Debug)]
pub(crate) enum Disposition {
    Untouched,
    Applied(ResponseDraft),
    Failed(FailureCode),
}

struct Settled {
    result: SyncResult,
    disposition: Disposition,
    outcome: &'static str,
}

struct Gate {
    cap: usize,
    counts: Mutex<HashMap<String, usize>>,
}

struct Turn {
    gate: Arc<Gate>,
    endpoint: String,
}

impl Gate {
    fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            cap,
            counts: Mutex::new(HashMap::new()),
        })
    }

    fn take(self: &Arc<Self>, endpoint: &str) -> Option<Turn> {
        let mut counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(endpoint.to_owned()).or_insert(0);
        if *count >= self.cap {
            return None;
        }
        *count += 1;
        Some(Turn {
            gate: Arc::clone(self),
            endpoint: endpoint.to_owned(),
        })
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut counts = self
            .gate
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&self.endpoint) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.endpoint);
            }
        }
    }
}

pub(crate) struct Slot {
    _permit: OwnedSemaphorePermit,
    _turn: Turn,
}

pub(crate) enum RuleAdmission {
    Off,
    Full,
    Granted(Slot),
}

struct Inner {
    settings: RulesSettings,
    slots: Arc<Semaphore>,
    endpoints: Arc<Gate>,
    dry_slots: Arc<Semaphore>,
    dry_endpoints: Arc<Gate>,
    compiles: Arc<Semaphore>,
    recordings: Arc<Semaphore>,
    cache: RuleCache,
    pool: Option<Arc<ScriptPool>>,
    compiler: ScriptCompiler,
    seed_key: [u8; SEED_KEY_BYTES],
}

#[derive(Clone)]
pub struct RulesRuntime {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for RulesRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RulesRuntime")
            .field("settings", &self.inner.settings)
            .finish_non_exhaustive()
    }
}

fn state_name(state: SyncState) -> &'static str {
    match state {
        SyncState::Queued => STATE_QUEUED,
        SyncState::Succeeded => STATE_SUCCEEDED,
        SyncState::Failed => STATE_FAILED,
    }
}

fn build_dry_rules(
    rules: DryRunRules,
    compiler: &ScriptCompiler,
) -> Result<CompiledRules, DryRunError> {
    let sources = match rules {
        DryRunRules::Saved(stored) => {
            load_stored(&stored, compiler).map_err(DryRunError::Stored)?
        }
        DryRunRules::Drafts { rules, references } => validate_set(&rules, &references, compiler)
            .map_err(DryRunError::Invalid)?
            .into_iter()
            .map(|rule| RuleSource {
                id: Uuid::nil(),
                rule,
            })
            .collect(),
    };
    CompiledRules::compile(sources, compiler).map_err(DryRunError::Compile)
}

fn within(limit: usize) -> bool {
    limit > 0 && limit <= Semaphore::MAX_PERMITS
}

fn seed_material(capture: &Uuid) -> Vec<u8> {
    let mut material = Vec::with_capacity(SEED_CONTEXT.len() + 1 + 16);
    material.extend_from_slice(SEED_CONTEXT);
    material.push(0);
    material.extend_from_slice(capture.as_bytes());
    material
}

fn dry_seed(capture: &Uuid) -> RandomSeed {
    RandomSeed::from_bytes(&seed_material(capture))
}

impl RulesRuntime {
    pub fn new(settings: RulesSettings, pool: Option<Arc<ScriptPool>>) -> Result<Self, RulesError> {
        if settings.sync_budget.is_zero() || settings.sync_budget > MAX_SYNC_BUDGET {
            return Err(RulesError::Budget);
        }
        if !within(settings.sync_concurrency)
            || !within(settings.per_endpoint)
            || !within(settings.dry_run_concurrency)
        {
            return Err(RulesError::Concurrency);
        }
        if settings.cache_bytes == 0 {
            return Err(RulesError::Cache);
        }
        let seed_key = random_bytes::<SEED_KEY_BYTES>().map_err(|_| RulesError::Entropy)?;
        let per_pattern = settings
            .per_endpoint
            .saturating_mul(2)
            .saturating_mul(MAX_REGEX_DFA_BYTES);
        Ok(Self {
            inner: Arc::new(Inner {
                slots: Arc::new(Semaphore::new(settings.sync_concurrency)),
                endpoints: Gate::new(settings.per_endpoint),
                dry_slots: Arc::new(Semaphore::new(settings.dry_run_concurrency)),
                dry_endpoints: Gate::new(1),
                compiles: Arc::new(Semaphore::new(COMPILE_SLOTS)),
                recordings: Arc::new(Semaphore::new(RECORD_SLOTS)),
                cache: RuleCache::new(settings.cache_bytes, per_pattern),
                seed_key,
                pool,
                compiler: ScriptCompiler::new(EngineLimits::default())?,
                settings,
            }),
        })
    }

    pub fn settings(&self) -> &RulesSettings {
        &self.inner.settings
    }

    pub fn compiler(&self) -> &ScriptCompiler {
        &self.inner.compiler
    }

    pub fn forget(&self, endpoint: &str) {
        self.inner.cache.forget(endpoint);
    }

    fn live_seed(
        &self,
        keyring: Option<&Keyring>,
        endpoint: &EndpointId,
        capture: &Uuid,
    ) -> Option<RandomSeed> {
        let material = seed_material(capture);
        match keyring {
            Some(keyring) => {
                let (_, key) = keyring.active_capture_mac_key(endpoint).ok()?;
                let tag = key.tag(&material).ok()?;
                Some(RandomSeed::from_bytes(tag.unpack()))
            }
            None => RandomSeed::derive(&self.inner.seed_key, &material).ok(),
        }
    }

    pub(crate) fn admit(&self, endpoint: &str) -> RuleAdmission {
        if !self.inner.settings.enabled {
            return RuleAdmission::Off;
        }
        match self.take_slot(endpoint) {
            Some(slot) => RuleAdmission::Granted(slot),
            None => RuleAdmission::Full,
        }
    }

    pub(crate) fn refuse(&self, store: &Store, capture: CaptureId) -> Disposition {
        metrics::counter!(RULE_RUNS_TOTAL, "phase" => PHASE, "outcome" => OUTCOME_OVERLOADED)
            .increment(1);
        self.requeue(store, capture);
        Disposition::Failed(FailureCode::Overloaded)
    }

    fn take_slot(&self, endpoint: &str) -> Option<Slot> {
        let turn = self.inner.endpoints.take(endpoint)?;
        let permit = Arc::clone(&self.inner.slots).try_acquire_owned().ok()?;
        Some(Slot {
            _permit: permit,
            _turn: turn,
        })
    }

    fn take_dry_slot(&self, endpoint: &str) -> Option<Slot> {
        let turn = self.inner.dry_endpoints.take(endpoint)?;
        let permit = Arc::clone(&self.inner.dry_slots).try_acquire_owned().ok()?;
        Some(Slot {
            _permit: permit,
            _turn: turn,
        })
    }

    fn run_budget(&self) -> Duration {
        let total = self.inner.settings.sync_budget;
        total.saturating_sub(WRITE_RESERVE.min(total / 4))
    }

    pub(crate) async fn before_response(
        &self,
        store: &Store,
        input: LiveInput<'_>,
        facts: RuleFacts,
        slot: Slot,
    ) -> Disposition {
        let started = Instant::now();
        let settled = self.execute(store, &input, facts, started, slot).await;
        let deadline = self
            .inner
            .settings
            .sync_budget
            .saturating_sub(started.elapsed());
        match store
            .finish_sync_run(&input.capture.id, &settled.result, deadline)
            .await
        {
            Ok(FinishOutcome::Written) => {}
            Ok(FinishOutcome::LeaseLost) => {
                tracing::warn!(
                    capture_id = %input.capture.id,
                    "the rule run was taken over before its result was written"
                );
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    capture_id = %input.capture.id,
                    "the rule run result could not be written; the lease will expire and the run is replayed"
                );
            }
        }
        metrics::counter!(RULE_RUNS_TOTAL, "phase" => PHASE, "outcome" => settled.outcome)
            .increment(1);
        metrics::histogram!(RULE_SYNC_DURATION_SECONDS).record(started.elapsed());
        settled.disposition
    }

    fn requeue(&self, store: &Store, capture: CaptureId) {
        let Some(slot) = Arc::clone(&self.inner.recordings).try_acquire_owned().ok() else {
            tracing::warn!(
                %capture,
                "an overloaded rule run was not queued because the recorder is saturated; its lease will expire"
            );
            return;
        };
        let store = store.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let result = Self::requeued().result;
            if let Err(error) = store
                .finish_sync_run(&capture, &result, SYNC_WRITE_DEADLINE)
                .await
            {
                tracing::warn!(%error, %capture, "an overloaded rule run could not be queued");
            }
        });
    }

    async fn execute(
        &self,
        store: &Store,
        input: &LiveInput<'_>,
        facts: RuleFacts,
        started: Instant,
        slot: Slot,
    ) -> Settled {
        let run_budget = self.run_budget();
        let left = || run_budget.saturating_sub(started.elapsed());
        let loading = self.compiled(store, input, left());
        let compiled = match tokio::time::timeout(left(), loading).await {
            Ok(Ok(compiled)) => compiled,
            Ok(Err(failure)) => return self.failed(input, failure),
            Err(_) => {
                return self.failed(
                    input,
                    Failure::new(FailureCode::Budget, "the rule set did not load in time"),
                );
            }
        };
        if !compiled.plan().has_before_part() {
            return Self::requeued();
        }
        let mut context = facts.into_context();
        let remaining = left();
        if let Err(failure) = self
            .load_variables(
                store,
                &input.endpoint.id,
                &compiled,
                &mut context,
                remaining,
                self.inner.settings.variables_deadline.min(remaining),
            )
            .await
        {
            return self.failed(input, failure);
        }
        let Some(seed) = self.live_seed(
            input.keyring,
            &input.endpoint.id,
            &input.capture.id.unpack(),
        ) else {
            return self.failed(
                input,
                Failure::new(FailureCode::Internal, "the random seed cannot be derived"),
            );
        };
        let remaining = left();
        if remaining.is_zero() {
            return self.failed(
                input,
                Failure::new(FailureCode::Budget, "no time is left to run the rules"),
            );
        }
        let machine = Machine::new(MachineInput {
            rules: Arc::clone(&compiled),
            context,
            seed,
            budget: remaining,
            mode: Mode::Live,
            tenant: input.endpoint.id.unpack().to_owned(),
        });
        let state = StoreState {
            store,
            endpoint: &input.endpoint.id,
            retention: input.retention,
        };
        let env = DriveEnv {
            pool: self.inner.pool.as_deref(),
            state: &state,
            script_memory_bytes: self.inner.settings.script_memory_bytes,
            state_timeout: self.inner.settings.state_timeout,
            slice_hook: None,
        };
        let finished = drive(machine, &env, remaining, slot).await;
        self.settle(input, Some(&compiled), finished)
    }

    fn failed(&self, input: &LiveInput<'_>, failure: Failure) -> Settled {
        self.settle(input, None, Finished::failed(failure))
    }

    fn requeued() -> Settled {
        Settled {
            result: SyncResult {
                state: SyncState::Queued,
                plan: PlanWrite::Keep,
                vars: json!({}),
                log: json!([]),
                response_status: None,
                capture_status: None,
                error: None,
                event: None,
            },
            disposition: Disposition::Untouched,
            outcome: OUTCOME_SUCCEEDED,
        }
    }

    fn settle(
        &self,
        input: &LiveInput<'_>,
        compiled: Option<&CompiledRules>,
        finished: Finished,
    ) -> Settled {
        let failure = finished.failure().cloned();
        let draft = finished.response;
        let (state, plan, error) = match (&failure, compiled) {
            (Some(failure), _) => (
                SyncState::Failed,
                PlanWrite::Clear,
                Some(failure.run_error()),
            ),
            (None, Some(_)) if finished.after.is_empty() => {
                (SyncState::Succeeded, PlanWrite::Clear, None)
            }
            (None, Some(compiled)) => match finished.after.plan(compiled.rules()) {
                Ok(plan) => (SyncState::Queued, PlanWrite::Set(plan), None),
                Err(error) => {
                    tracing::error!(%error, capture_id = %input.capture.id, "the after-response plan cannot be stored");
                    (
                        SyncState::Failed,
                        PlanWrite::Clear,
                        Some(format!("internal: {error}")),
                    )
                }
            },
            (None, None) => (
                SyncState::Failed,
                PlanWrite::Clear,
                Some("internal: the rules were not loaded".to_owned()),
            ),
        };
        let applied = failure.is_none();
        let response_status = applied.then_some(draft.status).flatten();
        let capture_status = if input.status_override.is_some() || input.preflight {
            None
        } else {
            response_status
        };
        let entries = fit_entries(finished.log, MAX_LOG_BYTES);
        let view = RunView {
            id: input.capture.id.unpack(),
            capture_id: input.capture.id.unpack(),
            trigger: TRIGGER_CAPTURE,
            state: state_name(state),
            rules_version: input.seed.rules_version,
            response_status,
            error: error.clone(),
            entries: entries.clone(),
            created_at: timestamp(input.capture.received_at),
            finished_at: (state != SyncState::Queued).then(|| timestamp(OffsetDateTime::now_utc())),
        };
        let event = match rule_result_payload(input.endpoint.id.unpack(), &view) {
            Ok(payload) => Some(payload),
            Err(error) => {
                tracing::error!(%error, "the rule result event cannot be encoded");
                None
            }
        };
        let result = SyncResult {
            state,
            plan,
            vars: Value::Object(finished.vars),
            log: serde_json::to_value(&entries).unwrap_or_else(|_| json!([])),
            response_status,
            capture_status,
            error,
            event,
        };
        let (disposition, outcome) = match failure {
            Some(failure) => (Disposition::Failed(failure.code), OUTCOME_FAILED),
            None => (Disposition::Applied(draft), OUTCOME_SUCCEEDED),
        };
        Settled {
            result,
            disposition,
            outcome,
        }
    }

    async fn compiled(
        &self,
        store: &Store,
        input: &LiveInput<'_>,
        deadline: Duration,
    ) -> Result<Arc<CompiledRules>, Failure> {
        let endpoint = input.endpoint.id.unpack();
        let version = input.seed.rules_version;
        if let Some(found) = self.inner.cache.lookup(endpoint, version) {
            return Ok(found);
        }
        if let Some(failure) = self.inner.cache.failure(endpoint, version) {
            return Err(failure);
        }
        let flight = self.inner.cache.flight(endpoint, version);
        let compiled = flight
            .cell()
            .get_or_try_init(|| self.load_and_compile(store, input, deadline))
            .await?;
        Ok(Arc::clone(compiled))
    }

    async fn load_and_compile(
        &self,
        store: &Store,
        input: &LiveInput<'_>,
        deadline: Duration,
    ) -> Result<Arc<CompiledRules>, Failure> {
        let endpoint = input.endpoint.id.unpack();
        let version = input.seed.rules_version;
        if let Some(found) = self.inner.cache.lookup(endpoint, version) {
            return Ok(found);
        }
        let plan = store
            .load_run_plan(
                &input.capture.id,
                self.inner.settings.plan_deadline.min(deadline),
            )
            .await
            .map_err(|error| {
                tracing::warn!(%error, endpoint_id = endpoint, "the rule set could not be loaded");
                Failure::new(FailureCode::Internal, "the rule set could not be loaded")
            })?
            .ok_or_else(|| Failure::new(FailureCode::Internal, "the rule run has no plan"))?;
        let permit = Arc::clone(&self.inner.compiles)
            .try_acquire_owned()
            .map_err(|_| {
                Failure::new(
                    FailureCode::Overloaded,
                    "too many rule sets are compiling at once",
                )
            })?;
        let inner = Arc::clone(&self.inner);
        let key = endpoint.to_owned();
        let built = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let stored = rules_from_plan(&plan).map_err(|error| {
                tracing::error!(%error, "the stored rule plan has the wrong shape");
                Failure::new(FailureCode::Internal, "the stored rules are unreadable")
            })?;
            let sources = load_stored(&stored, &inner.compiler).map_err(|error| {
                tracing::error!(
                    diagnostics = error.diagnostics.len(),
                    "the stored rules no longer validate"
                );
                Failure::new(FailureCode::Internal, "the stored rules no longer validate")
            })?;
            let compiled = CompiledRules::compile(sources, &inner.compiler).map_err(|error| {
                tracing::error!(%error, "the stored rules no longer compile");
                Failure::new(FailureCode::Internal, "the stored rules no longer compile")
            })?;
            let compiled = Arc::new(compiled);
            inner.cache.store(&key, version, Arc::clone(&compiled));
            Ok::<_, Failure>(compiled)
        })
        .await
        .map_err(|_| Failure::new(FailureCode::Internal, "the rule set job did not finish"))?;
        if let Err(failure) = &built {
            self.inner
                .cache
                .remember_failure(endpoint, version, failure);
        }
        built
    }

    async fn load_variables(
        &self,
        store: &Store,
        endpoint: &EndpointId,
        compiled: &CompiledRules,
        context: &mut Context,
        acquire: Duration,
        deadline: Duration,
    ) -> Result<(), Failure> {
        load_rule_variables(store, endpoint, compiled, context, acquire, deadline).await
    }

    pub async fn dry_run(&self, store: &Store, input: DryRunInput) -> Result<RunView, DryRunError> {
        if !self.inner.settings.enabled {
            return Err(DryRunError::Disabled);
        }
        let slot = self
            .take_dry_slot(input.endpoint.unpack())
            .ok_or(DryRunError::Busy)?;
        let started = Instant::now();
        let budget = self.inner.settings.sync_budget;
        let compiler = self.inner.compiler;
        let rules = input.rules;
        let (built, slot) =
            tokio::task::spawn_blocking(move || (build_dry_rules(rules, &compiler), slot))
                .await
                .map_err(|_| DryRunError::Internal)?;
        let compiled = Arc::new(built?);
        let mut context = input.facts.into_context();
        let remaining = budget.saturating_sub(started.elapsed());
        let loaded = self
            .load_variables(
                store,
                &input.endpoint,
                &compiled,
                &mut context,
                remaining,
                DRY_RUN_VARIABLES_DEADLINE.min(remaining),
            )
            .await;
        let seed = dry_seed(&input.capture_id);
        let finished = match loaded {
            Err(failure) => Finished::failed(failure),
            Ok(()) => {
                let remaining = budget.saturating_sub(started.elapsed());
                let machine = Machine::new(MachineInput {
                    rules: Arc::clone(&compiled),
                    context,
                    seed,
                    budget: remaining,
                    mode: Mode::DryRun,
                    tenant: input.endpoint.unpack().to_owned(),
                });
                let env = DriveEnv {
                    pool: self.inner.pool.as_deref(),
                    state: &NoState,
                    script_memory_bytes: self.inner.settings.script_memory_bytes,
                    state_timeout: self.inner.settings.state_timeout,
                    slice_hook: None,
                };
                drive(machine, &env, remaining, slot).await
            }
        };
        let failure = finished.failure().cloned();
        let response_status = finished.response.status;
        let now = OffsetDateTime::now_utc();
        Ok(RunView {
            id: input.capture_id,
            capture_id: input.capture_id,
            trigger: TRIGGER_DRY_RUN,
            state: if failure.is_some() {
                STATE_FAILED
            } else {
                STATE_SUCCEEDED
            },
            rules_version: input.rules_version,
            response_status: response_status.filter(|_| failure.is_none()),
            error: failure.map(|failure| failure.run_error()),
            entries: fit_entries(finished.log, MAX_LOG_BYTES),
            created_at: timestamp(now),
            finished_at: Some(timestamp(now)),
        })
    }
}

pub async fn load_rule_variables(
    store: &Store,
    endpoint: &EndpointId,
    compiled: &CompiledRules,
    context: &mut Context,
    acquire: Duration,
    deadline: Duration,
) -> Result<(), Failure> {
    let names = compiled.variable_names();
    if names.is_empty() {
        return Ok(());
    }
    if acquire.is_zero() || deadline.is_zero() {
        return Err(Failure::new(
            FailureCode::Budget,
            "no time is left to load the variables",
        ));
    }
    let vars = store
        .load_rule_vars(endpoint, &names, acquire, deadline)
        .await
        .map_err(|error| {
            tracing::warn!(%error, endpoint_id = endpoint.unpack(), "the rule variables could not be loaded");
            if matches!(error, VarStoreError::Deadline(_)) {
                Failure::new(FailureCode::Budget, "the variables did not load in time")
            } else {
                Failure::new(FailureCode::Internal, "the variables could not be loaded")
            }
        })?;
    for name in compiled.counter_names() {
        if let Some(value) = vars.counter(name) {
            context.counters.insert(name.clone(), value);
        }
    }
    for name in compiled.store_names() {
        if let Some(value) = vars.values().get(name) {
            context.store.insert(name.clone(), value.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyring(byte: &str) -> Keyring {
        let group = if byte == "x" { "eHh4" } else { "eXl5" };
        let tail = if byte == "x" { "eHg=" } else { "eXk=" };
        Keyring::parse(&format!("1:{}{tail}", group.repeat(10))).expect("a valid keyring")
    }

    fn runtime() -> RulesRuntime {
        RulesRuntime::new(RulesSettings::default(), None).expect("the runtime builds")
    }

    #[test]
    fn a_keyring_gives_every_runtime_the_same_seed_for_a_capture() {
        let endpoint = EndpointId::generate().expect("entropy is available");
        let capture = Uuid::from_u128(7);
        let (first, second) = (runtime(), runtime());
        let ring = keyring("x");

        let one = first.live_seed(Some(&ring), &endpoint, &capture);
        let two = second.live_seed(Some(&ring), &endpoint, &capture);

        assert!(one.is_some());
        assert_eq!(one, two);
        assert_ne!(
            one,
            first.live_seed(Some(&ring), &endpoint, &Uuid::from_u128(8))
        );
        assert_ne!(
            one,
            first.live_seed(Some(&keyring("y")), &endpoint, &capture)
        );
    }

    #[test]
    fn without_a_keyring_a_live_seed_cannot_be_derived_from_the_capture_id() {
        let endpoint = EndpointId::generate().expect("entropy is available");
        let capture = Uuid::from_u128(7);
        let (first, second) = (runtime(), runtime());

        let one = first.live_seed(None, &endpoint, &capture);

        assert!(one.is_some());
        assert_eq!(one, first.live_seed(None, &endpoint, &capture));
        assert_ne!(one, second.live_seed(None, &endpoint, &capture));
        assert_ne!(one, Some(dry_seed(&capture)));
    }
}
