use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use time::OffsetDateTime;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::capture_id::CaptureId;
use whl_core::config::Config;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::IdGenerationError;
use whl_core::limiter::Limiters;
use whl_script::ScriptPool;
use whl_secrets::Keyring;
use whl_store::Store;
use whl_store::captures::CapturePolicy;

use crate::client_ip::TrustedProxies;
use crate::delay::DelaySlots;
use crate::rules::{RulesRuntime, RulesSettings};
use crate::seams::{Admission, StorageFull, admit_client, admit_endpoint};
use crate::template::{TemplateEngine, TemplateSettings};

pub const DEFAULT_BODY_READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_BUDGET_WAIT: Duration = Duration::from_secs(2);
pub const MAX_SUPPORTED_BODY_BYTES: usize = 1 << 30;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateError {
    #[error(
        "the body limit of {0} bytes is above the supported maximum of {MAX_SUPPORTED_BODY_BYTES}"
    )]
    BodyLimit(u64),
    #[error("the memory budget of {0} bytes is not supported")]
    Budget(u64),
    #[error("the capture limit of {0} is above the supported maximum")]
    CaptureLimit(u32),
    #[error("the public URL cannot be used in replies")]
    PublicUrl,
    #[error("the delayed response cap of {0} is not supported")]
    MaxDelayed(u32),
    #[error("the rules runtime cannot be set up")]
    Rules,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("the body memory budget stayed exhausted for the allowed wait")]
pub struct BudgetExhausted;

pub trait CaptureSource: Send + Sync {
    fn now(&self) -> OffsetDateTime;
    fn capture_id(&self) -> Result<CaptureId, IdGenerationError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSource;

impl CaptureSource for SystemSource {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    fn capture_id(&self) -> Result<CaptureId, IdGenerationError> {
        CaptureId::generate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    pub max_body_bytes: usize,
    pub max_captures: u32,
    pub retention: Duration,
    pub body_memory_budget: usize,
    pub body_read_timeout: Duration,
    pub budget_wait: Duration,
}

impl CaptureLimits {
    pub fn from_config(config: &Config) -> Result<Self, StateError> {
        let capture = &config.capture;
        let body = capture.max_body_bytes.as_u64();
        let budget = capture.body_memory_budget.as_u64();
        Ok(Self {
            max_body_bytes: usize::try_from(body).map_err(|_| StateError::BodyLimit(body))?,
            max_captures: capture.max_captures,
            retention: capture.retention,
            body_memory_budget: usize::try_from(budget).map_err(|_| StateError::Budget(budget))?,
            body_read_timeout: DEFAULT_BODY_READ_TIMEOUT,
            budget_wait: DEFAULT_BUDGET_WAIT,
        })
    }

    fn validate(&self) -> Result<(), StateError> {
        if self.max_body_bytes > MAX_SUPPORTED_BODY_BYTES {
            return Err(StateError::BodyLimit(self.max_body_bytes as u64));
        }
        if self.body_memory_budget > Semaphore::MAX_PERMITS {
            return Err(StateError::Budget(self.body_memory_budget as u64));
        }
        if i32::try_from(self.max_captures).is_err() {
            return Err(StateError::CaptureLimit(self.max_captures));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CaptureSettings {
    pub limits: CaptureLimits,
    pub trusted_proxies: Vec<IpNet>,
    pub public_url: String,
    pub max_delayed: usize,
}

impl CaptureSettings {
    pub fn from_config(config: &Config) -> Result<Self, StateError> {
        Ok(Self {
            limits: CaptureLimits::from_config(config)?,
            trusted_proxies: config.trusted_proxies.clone(),
            public_url: config.public_url.as_str().trim_end_matches('/').to_owned(),
            max_delayed: usize::try_from(config.capture.max_delayed)
                .map_err(|_| StateError::MaxDelayed(config.capture.max_delayed))?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BodyBudget {
    semaphore: Arc<Semaphore>,
    total: usize,
}

impl BodyBudget {
    pub fn new(total: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(total)),
            total,
        }
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn in_use(&self) -> usize {
        self.total
            .saturating_sub(self.semaphore.available_permits())
    }

    pub async fn reserve(
        &self,
        bytes: usize,
        wait: Duration,
    ) -> Result<Option<OwnedSemaphorePermit>, BudgetExhausted> {
        if bytes == 0 {
            return Ok(None);
        }
        let units = u32::try_from(bytes).map_err(|_| BudgetExhausted)?;
        let acquired =
            tokio::time::timeout(wait, Arc::clone(&self.semaphore).acquire_many_owned(units))
                .await
                .map_err(|_| BudgetExhausted)?;
        acquired.map(Some).map_err(|_| BudgetExhausted)
    }
}

#[derive(Clone)]
pub struct CaptureState {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    limits: CaptureLimits,
    proxies: TrustedProxies,
    budget: BodyBudget,
    source: Arc<dyn CaptureSource>,
    storage_full: StorageFull,
    limiters: Option<Limiters>,
    public_url: String,
    delays: DelaySlots,
    secrets: Option<Arc<Keyring>>,
    templates: Arc<TemplateEngine>,
    rules: RulesRuntime,
}

impl CaptureState {
    pub fn new(store: Store, settings: CaptureSettings) -> Result<Self, StateError> {
        settings.limits.validate()?;
        if settings.public_url.is_empty() {
            return Err(StateError::PublicUrl);
        }
        if settings.max_delayed > Semaphore::MAX_PERMITS {
            return Err(StateError::MaxDelayed(
                u32::try_from(settings.max_delayed).unwrap_or(u32::MAX),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                budget: BodyBudget::new(settings.limits.body_memory_budget),
                limits: settings.limits,
                proxies: TrustedProxies::new(settings.trusted_proxies),
                source: Arc::new(SystemSource),
                storage_full: StorageFull::default(),
                limiters: None,
                public_url: settings.public_url,
                delays: DelaySlots::new(settings.max_delayed),
                secrets: None,
                templates: Arc::new(TemplateEngine::new(TemplateSettings::default())),
                rules: RulesRuntime::new(RulesSettings::default(), None)
                    .map_err(|_| StateError::Rules)?,
            }),
        })
    }

    pub fn with_rules(
        self,
        settings: RulesSettings,
        pool: Option<Arc<ScriptPool>>,
    ) -> Result<Self, StateError> {
        let rules = RulesRuntime::new(settings, pool).map_err(|_| StateError::Rules)?;
        Ok(Self {
            inner: Arc::new(Inner {
                store: self
                    .inner
                    .store
                    .clone()
                    .with_rules_enabled(settings.enabled),
                limits: self.inner.limits,
                proxies: self.inner.proxies.clone(),
                budget: self.inner.budget.clone(),
                source: Arc::clone(&self.inner.source),
                storage_full: self.inner.storage_full.clone(),
                limiters: self.inner.limiters.clone(),
                public_url: self.inner.public_url.clone(),
                delays: self.inner.delays.clone(),
                secrets: self.inner.secrets.clone(),
                templates: Arc::clone(&self.inner.templates),
                rules,
            }),
        })
    }

    pub fn rules(&self) -> &RulesRuntime {
        &self.inner.rules
    }

    pub fn with_templates(self, settings: TemplateSettings) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: self.inner.store.clone(),
                limits: self.inner.limits,
                proxies: self.inner.proxies.clone(),
                budget: self.inner.budget.clone(),
                source: Arc::clone(&self.inner.source),
                storage_full: self.inner.storage_full.clone(),
                limiters: self.inner.limiters.clone(),
                public_url: self.inner.public_url.clone(),
                delays: self.inner.delays.clone(),
                secrets: self.inner.secrets.clone(),
                templates: Arc::new(TemplateEngine::new(settings)),
                rules: self.inner.rules.clone(),
            }),
        }
    }

    pub fn with_limiters(self, limiters: Limiters) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: self.inner.store.clone(),
                limits: self.inner.limits,
                proxies: self.inner.proxies.clone(),
                budget: self.inner.budget.clone(),
                source: Arc::clone(&self.inner.source),
                storage_full: self.inner.storage_full.clone(),
                limiters: Some(limiters),
                public_url: self.inner.public_url.clone(),
                delays: self.inner.delays.clone(),
                secrets: self.inner.secrets.clone(),
                templates: Arc::clone(&self.inner.templates),
                rules: self.inner.rules.clone(),
            }),
        }
    }

    pub fn with_secrets(self, keyring: Arc<Keyring>) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: self.inner.store.clone(),
                limits: self.inner.limits,
                proxies: self.inner.proxies.clone(),
                budget: self.inner.budget.clone(),
                source: Arc::clone(&self.inner.source),
                storage_full: self.inner.storage_full.clone(),
                limiters: self.inner.limiters.clone(),
                public_url: self.inner.public_url.clone(),
                delays: self.inner.delays.clone(),
                secrets: Some(keyring),
                templates: Arc::clone(&self.inner.templates),
                rules: self.inner.rules.clone(),
            }),
        }
    }

    pub fn with_source(self, source: Arc<dyn CaptureSource>) -> Self {
        Self {
            inner: Arc::new(Inner {
                store: self.inner.store.clone(),
                limits: self.inner.limits,
                proxies: self.inner.proxies.clone(),
                budget: self.inner.budget.clone(),
                source,
                storage_full: self.inner.storage_full.clone(),
                limiters: self.inner.limiters.clone(),
                public_url: self.inner.public_url.clone(),
                delays: self.inner.delays.clone(),
                secrets: self.inner.secrets.clone(),
                templates: Arc::clone(&self.inner.templates),
                rules: self.inner.rules.clone(),
            }),
        }
    }

    pub fn storage_full(&self) -> &StorageFull {
        &self.inner.storage_full
    }

    pub fn budget(&self) -> &BodyBudget {
        &self.inner.budget
    }

    pub fn delays(&self) -> &DelaySlots {
        &self.inner.delays
    }

    pub fn limits(&self) -> &CaptureLimits {
        &self.inner.limits
    }

    pub(crate) fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn templates(&self) -> &TemplateEngine {
        &self.inner.templates
    }

    pub(crate) fn proxies(&self) -> &TrustedProxies {
        &self.inner.proxies
    }

    pub(crate) fn source(&self) -> &dyn CaptureSource {
        self.inner.source.as_ref()
    }

    pub(crate) fn secrets(&self) -> Option<&Keyring> {
        self.inner.secrets.as_deref()
    }

    pub(crate) fn public_url(&self) -> &str {
        &self.inner.public_url
    }

    pub(crate) fn policy(&self) -> CapturePolicy {
        CapturePolicy {
            max_captures: self.inner.limits.max_captures,
            retention: self.inner.limits.retention,
        }
    }

    pub(crate) fn admit(&self, client_ip: IpAddr) -> Admission {
        admit_client(self.inner.limiters.as_ref(), client_ip)
    }

    pub(crate) fn admit_endpoint(&self, endpoint: &EndpointId) -> Admission {
        admit_endpoint(self.inner.limiters.as_ref(), endpoint)
    }
}
