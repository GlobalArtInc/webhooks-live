use std::net::IpAddr;
use std::sync::Arc;

use ipnet::IpNet;
use tokio::sync::Semaphore;
use whl_core::limiter::{ClientKey, KeyedLimiter, LimiterError, Verdict};
use whl_core::rate::{RatePeriod, RateSpec};
use whl_store::Store;
use whl_store::captures::CapturePolicy;
use whl_telemetry::series;

use crate::gate::{EndpointQuota, IpSessions, MemoryBudget, NegativeCache};
use crate::settings::{MailDomain, MailLimits, StorageProbe};
use crate::tls::TlsState;

const MAX_TRACKED_CLIENTS: usize = 100_000;

pub struct Shared {
    pub store: Store,
    pub domain: MailDomain,
    pub limits: MailLimits,
    pub policy: CapturePolicy,
    pub proxies: Vec<IpNet>,
    pub tls: Option<Arc<TlsState>>,
    pub memory: MemoryBudget,
    pub sessions: Arc<Semaphore>,
    pub ip_sessions: Arc<IpSessions>,
    pub connection_rate: KeyedLimiter<ClientKey>,
    pub message_rate: KeyedLimiter<ClientKey>,
    pub quota: EndpointQuota,
    pub unknown_endpoints: NegativeCache,
    pub storage_full: Option<StorageProbe>,
}

impl Shared {
    pub fn new(
        store: Store,
        settings: crate::settings::MailSettings,
        tls: Option<Arc<TlsState>>,
    ) -> Result<Self, LimiterError> {
        let limits = settings.limits;
        let per_minute = |count: u32| RateSpec {
            count,
            period: RatePeriod::Minute,
            burst: count,
        };
        Ok(Self {
            store,
            domain: settings.domain,
            policy: settings.capture,
            proxies: settings.trusted_proxies,
            tls,
            memory: MemoryBudget::new(limits.memory_budget),
            sessions: Arc::new(Semaphore::new(limits.max_sessions)),
            ip_sessions: IpSessions::new(limits.sessions_per_ip),
            connection_rate: KeyedLimiter::new(
                per_minute(limits.connections_per_minute_per_ip),
                MAX_TRACKED_CLIENTS,
            )?,
            message_rate: KeyedLimiter::new(
                per_minute(limits.messages_per_minute_per_ip),
                MAX_TRACKED_CLIENTS,
            )?,
            quota: EndpointQuota::new(limits.messages_per_hour_per_endpoint),
            unknown_endpoints: NegativeCache::default(),
            storage_full: settings.storage_full,
            limits,
        })
    }

    pub fn trusts(&self, peer: IpAddr) -> bool {
        let peer = peer.to_canonical();
        self.proxies.iter().any(|net| net.contains(&peer))
    }

    pub fn expects_proxy_header(&self) -> bool {
        !self.proxies.is_empty()
    }

    pub fn connection_allowed(&self, client: IpAddr) -> bool {
        matches!(
            self.connection_rate.check(&ClientKey::from_ip(client)),
            Verdict::Allowed { .. }
        )
    }

    pub fn message_allowed(&self, client: IpAddr) -> bool {
        matches!(
            self.message_rate.check(&ClientKey::from_ip(client)),
            Verdict::Allowed { .. }
        )
    }

    pub fn storage_is_full(&self) -> bool {
        self.storage_full.as_ref().is_some_and(|probe| probe())
    }

    pub fn cleanup(&self) {
        self.connection_rate.cleanup();
        self.message_rate.cleanup();
    }
}

pub fn refused(reason: &'static str) {
    metrics::counter!(series::MAIL_REFUSED_TOTAL, "reason" => reason).increment(1);
}

pub fn message_result(result: &'static str) {
    metrics::counter!(series::MAIL_MESSAGES_TOTAL, "result" => result).increment(1);
}
