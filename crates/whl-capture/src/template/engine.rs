use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::entropy::random_bytes;
use whl_store::Store;
use whl_store::captures::{Endpoint, NewCapture, StoredCapture};
use whl_telemetry::series::{
    TEMPLATE_FALLBACKS_TOTAL, TEMPLATE_RENDER_DURATION_SECONDS, TEMPLATE_RENDERS_TOTAL,
};
use whl_template::{RandomSeed, ResponseBudget, SystemClock};

use super::compiled::{
    CompiledResponse, ResponseSources, TemplateCache, TemplateViolation, compile,
};
use super::render::{EndpointFacts, RenderJob, run};
use super::report::{FallbackCode, Rendering};

pub const DEFAULT_RESPONSE_BUDGET: Duration = Duration::from_millis(100);
pub const DEFAULT_VARIABLES_DEADLINE: Duration = Duration::from_millis(50);
const MIN_CONCURRENCY: usize = 4;
const PER_ENDPOINT_RENDERS: usize = 2;
const RECORD_SLOTS: usize = 32;
const CONCURRENCY_PER_CPU: usize = 2;
const SEED_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateSettings {
    pub response_budget: Duration,
    pub variables_deadline: Duration,
    pub concurrency: usize,
}

impl Default for TemplateSettings {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self {
            response_budget: DEFAULT_RESPONSE_BUDGET,
            variables_deadline: DEFAULT_VARIABLES_DEADLINE,
            concurrency: (cpus * CONCURRENCY_PER_CPU).max(MIN_CONCURRENCY),
        }
    }
}

pub(crate) struct TemplateEngine {
    settings: TemplateSettings,
    slots: Arc<Semaphore>,
    cache: TemplateCache,
    seed_key: Option<[u8; SEED_KEY_BYTES]>,
    in_flight: Mutex<HashMap<String, usize>>,
    recordings: Arc<Semaphore>,
}

struct EndpointTurn<'a> {
    engine: &'a TemplateEngine,
    endpoint: String,
}

impl Drop for EndpointTurn<'_> {
    fn drop(&mut self) {
        let mut counts = self
            .engine
            .in_flight
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

impl std::fmt::Debug for TemplateEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TemplateEngine")
            .field("settings", &self.settings)
            .finish()
    }
}

pub(crate) struct RenderInput {
    pub endpoint: Endpoint,
    pub capture: StoredCapture,
    pub request: NewCapture,
    pub public_url: String,
    pub with_status: bool,
}

impl TemplateEngine {
    pub(crate) fn new(settings: TemplateSettings) -> Self {
        let seed_key = random_bytes::<SEED_KEY_BYTES>().ok();
        if seed_key.is_none() {
            tracing::error!("the template seed key cannot be generated; templates will fall back");
        }
        Self {
            slots: Arc::new(Semaphore::new(settings.concurrency)),
            settings,
            cache: TemplateCache::default(),
            seed_key,
            in_flight: Mutex::new(HashMap::new()),
            recordings: Arc::new(Semaphore::new(RECORD_SLOTS)),
        }
    }

    pub(crate) fn recording_slot(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.recordings).try_acquire_owned().ok()
    }

    fn take_turn(&self, endpoint: &str) -> Option<EndpointTurn<'_>> {
        let mut counts = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(endpoint.to_owned()).or_insert(0);
        if *count >= PER_ENDPOINT_RENDERS {
            return None;
        }
        *count += 1;
        Some(EndpointTurn {
            engine: self,
            endpoint: endpoint.to_owned(),
        })
    }

    pub(crate) async fn respond(&self, store: &Store, input: RenderInput) -> Rendering {
        let started = Instant::now();
        let rendering = self.render(store, input).await;
        let outcome = rendering.outcome();
        metrics::counter!(TEMPLATE_RENDERS_TOTAL, "outcome" => outcome).increment(1);
        if let Some(code) = rendering.fallback_code() {
            metrics::counter!(TEMPLATE_FALLBACKS_TOTAL, "reason" => code.as_str()).increment(1);
        }
        metrics::histogram!(TEMPLATE_RENDER_DURATION_SECONDS).record(started.elapsed());
        rendering
    }

    async fn render(&self, store: &Store, input: RenderInput) -> Rendering {
        let Some(_turn) = self.take_turn(input.endpoint.id.unpack()) else {
            return Rendering::fallback(FallbackCode::Overloaded);
        };
        let clock = SystemClock::new();
        let budget = Arc::new(ResponseBudget::start(&clock, self.settings.response_budget));
        let Some(seed) = self.seed_for(&input.capture) else {
            return Rendering::fallback(FallbackCode::Internal);
        };
        let compiled = match self.compiled(&input.endpoint).await {
            Ok(compiled) => compiled,
            Err(code) => return Rendering::fallback(code),
        };
        let names = compiled.variable_names();
        let vars = if names.is_empty() {
            None
        } else {
            let remaining = budget.remaining(&clock);
            let deadline = remaining.min(self.settings.variables_deadline);
            if deadline.is_zero() {
                return Rendering::fallback(FallbackCode::Budget);
            }
            match store.load_vars(&input.endpoint.id, &names, deadline).await {
                Ok(vars) => Some(vars),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        endpoint_id = input.endpoint.id.unpack(),
                        "the template variables could not be loaded"
                    );
                    return Rendering::fallback(FallbackCode::Variables);
                }
            }
        };
        let job = RenderJob {
            compiled,
            request: input.request,
            capture: input.capture,
            endpoint: EndpointFacts::new(&input.endpoint, &input.public_url),
            vars,
            seed,
            budget,
            clock,
            with_status: input.with_status,
        };
        match self.blocking(move || run(&job)).await {
            Ok(rendering) => rendering,
            Err(code) => Rendering::fallback(code),
        }
    }

    fn seed_for(&self, capture: &StoredCapture) -> Option<RandomSeed> {
        let key = self.seed_key.as_ref()?;
        let id = capture.id.unpack();
        RandomSeed::derive(key, id.as_bytes()).ok()
    }

    async fn compiled(&self, endpoint: &Endpoint) -> Result<Arc<CompiledResponse>, FallbackCode> {
        if let Some(compiled) = self.cache.lookup(endpoint) {
            return Ok(compiled);
        }
        let owned = endpoint.clone();
        let parsed = self
            .blocking(move || {
                let headers = owned
                    .headers
                    .iter()
                    .map(|pair| (pair.name.as_str(), pair.value.as_str()))
                    .collect();
                let sources = ResponseSources {
                    status: owned.status_template.as_deref(),
                    headers,
                    body: &owned.body,
                };
                compile(&sources).map_err(summarize)
            })
            .await?;
        match parsed {
            Ok(compiled) => {
                let compiled = Arc::new(compiled);
                self.cache.store(endpoint, Arc::clone(&compiled));
                Ok(compiled)
            }
            Err(count) => {
                tracing::error!(
                    endpoint_id = endpoint.id.unpack(),
                    violations = count,
                    "the stored response templates no longer validate"
                );
                Err(FallbackCode::Invalid)
            }
        }
    }

    async fn blocking<T, F>(&self, job: F) -> Result<T, FallbackCode>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let permit = Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| FallbackCode::Overloaded)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            job()
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "a template job failed");
            FallbackCode::Internal
        })
    }
}

fn summarize(violations: Vec<TemplateViolation>) -> usize {
    violations.len()
}
