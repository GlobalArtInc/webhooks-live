use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, Method, StatusCode};
use ipnet::IpNet;
use tokio::sync::Semaphore;
use whl_core::config::Config;
use whl_forward::{ForwardError, GuardConfig, GuardedClient, Hardening};

use crate::forward::ForwardSettings;

pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_SEND_BODY_BYTES: usize = 1536 * 1024;
pub const MAX_TIMEOUT_MS: u64 = 15_000;
pub const MAX_REDIRECTS: u32 = 5;
pub const MAX_HISTORY_PAGE: i64 = 50;
pub const MAX_IN_FLIGHT_PER_SUBJECT: usize = 4;
pub const MAX_HOPS: u32 = 3;
pub const DENIED_PORTS: [u16; 18] = [
    22, 23, 25, 110, 143, 465, 587, 993, 995, 2375, 2376, 3306, 5432, 6379, 9100, 10250, 11211,
    27017,
];

const DEFAULT_MAX_IN_FLIGHT: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerLimits {
    pub endpoint_per_hour: u32,
    pub network_per_day: u32,
    pub user_per_hour: u32,
}

#[derive(Debug)]
pub struct LocalRequest {
    pub method: Method,
    pub host: String,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub client_ip: IpAddr,
}

#[derive(Debug)]
pub struct LocalReply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("the in-process capture failed: {0}")]
    Failed(String),
}

pub type LocalFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LocalReply, LocalError>> + Send + 'a>>;

pub trait LocalDelivery: Send + Sync + 'static {
    fn deliver(&self, request: LocalRequest) -> LocalFuture<'_>;
}

pub(crate) struct Clients {
    pub direct: GuardedClient,
    pub following: GuardedClient,
    pub guard: GuardConfig,
}

#[derive(Clone)]
pub struct ComposerSettings {
    pub(crate) enabled: bool,
    pub(crate) limits: Option<ComposerLimits>,
    pub(crate) local: Option<Arc<dyn LocalDelivery>>,
    pub(crate) gate: Arc<Semaphore>,
    subjects: Arc<Mutex<HashMap<String, usize>>>,
    allowed_networks: Vec<IpNet>,
    clients: Arc<OnceLock<Result<Clients, ForwardError>>>,
}

impl ComposerSettings {
    pub fn new(enabled: bool, limits: ComposerLimits, max_in_flight: usize) -> Self {
        Self::tiered(enabled, max_in_flight).with_limits(limits)
    }

    pub fn with_limits(mut self, limits: ComposerLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn tiered(enabled: bool, max_in_flight: usize) -> Self {
        let permits = max_in_flight.clamp(1, Semaphore::MAX_PERMITS);
        Self {
            enabled,
            limits: None,
            local: None,
            gate: Arc::new(Semaphore::new(permits)),
            subjects: Arc::new(Mutex::new(HashMap::new())),
            allowed_networks: Vec::new(),
            clients: Arc::new(OnceLock::new()),
        }
    }

    pub fn from_config(config: &Config) -> Self {
        let composer = &config.composer;
        Self::tiered(
            composer.enabled,
            usize::try_from(composer.max_in_flight).unwrap_or(usize::MAX),
        )
    }

    pub fn with_local(mut self, local: Arc<dyn LocalDelivery>) -> Self {
        self.local = Some(local);
        self
    }

    pub fn with_allowed_networks(mut self, networks: Vec<IpNet>) -> Self {
        self.allowed_networks = networks;
        self
    }

    pub(crate) fn acquire_subject(&self, key: String) -> Option<SubjectPermit> {
        let mut held = self
            .subjects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = held.entry(key.clone()).or_insert(0);
        if *count >= MAX_IN_FLIGHT_PER_SUBJECT {
            return None;
        }
        *count += 1;
        Some(SubjectPermit {
            subjects: Arc::clone(&self.subjects),
            key,
        })
    }

    pub(crate) fn clients(&self, forward: &ForwardSettings) -> Result<&Clients, &ForwardError> {
        self.clients
            .get_or_init(|| self.build_clients(forward))
            .as_ref()
    }

    fn build_clients(&self, forward: &ForwardSettings) -> Result<Clients, ForwardError> {
        let base = GuardConfig {
            allow: self.allowed_networks.clone(),
            max_response_bytes: MAX_RESPONSE_BODY_BYTES,
            hop_timeout: Duration::from_millis(MAX_TIMEOUT_MS),
            total_timeout: Duration::from_millis(MAX_TIMEOUT_MS),
            max_redirects: 0,
            hardening: Hardening {
                refuse_dotless_hosts: true,
                denied_ports: DENIED_PORTS.to_vec(),
            },
            ..forward.guard.clone()
        };
        let following = GuardConfig {
            max_redirects: MAX_REDIRECTS,
            ..base.clone()
        };
        Ok(Clients {
            direct: GuardedClient::with_lookup(base.clone(), Arc::clone(&forward.lookup))?,
            following: GuardedClient::with_lookup(following, Arc::clone(&forward.lookup))?,
            guard: base,
        })
    }
}

pub(crate) struct SubjectPermit {
    subjects: Arc<Mutex<HashMap<String, usize>>>,
    key: String,
}

impl Drop for SubjectPermit {
    fn drop(&mut self) {
        let mut held = self
            .subjects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = held.get_mut(&self.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                held.remove(&self.key);
            }
        }
    }
}

impl Default for ComposerSettings {
    fn default() -> Self {
        Self::tiered(true, DEFAULT_MAX_IN_FLIGHT)
    }
}
