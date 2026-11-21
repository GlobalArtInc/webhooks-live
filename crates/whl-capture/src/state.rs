use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use time::OffsetDateTime;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::capture_id::CaptureId;
use whl_core::config::Config;
use whl_core::entropy::IdGenerationError;
use whl_store::Store;
use whl_store::captures::CapturePolicy;

use crate::client_ip::TrustedProxies;
use crate::seams::{Admission, StorageFull, admit_client};

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
}

impl CaptureSettings {
    pub fn from_config(config: &Config) -> Result<Self, StateError> {
        Ok(Self {
            limits: CaptureLimits::from_config(config)?,
            trusted_proxies: config.trusted_proxies.clone(),
            public_url: config.public_url.as_str().trim_end_matches('/').to_owned(),
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
    public_url: String,
}

impl CaptureState {
    pub fn new(store: Store, settings: CaptureSettings) -> Result<Self, StateError> {
        settings.limits.validate()?;
        if settings.public_url.is_empty() {
            return Err(StateError::PublicUrl);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                budget: BodyBudget::new(settings.limits.body_memory_budget),
                limits: settings.limits,
                proxies: TrustedProxies::new(settings.trusted_proxies),
                source: Arc::new(SystemSource),
                storage_full: StorageFull::default(),
                public_url: settings.public_url,
            }),
        })
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
                public_url: self.inner.public_url.clone(),
            }),
        }
    }

    pub fn storage_full(&self) -> &StorageFull {
        &self.inner.storage_full
    }

    pub fn budget(&self) -> &BodyBudget {
        &self.inner.budget
    }

    pub fn limits(&self) -> &CaptureLimits {
        &self.inner.limits
    }

    pub(crate) fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn proxies(&self) -> &TrustedProxies {
        &self.inner.proxies
    }

    pub(crate) fn source(&self) -> &dyn CaptureSource {
        self.inner.source.as_ref()
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
        admit_client(client_ip)
    }
}
