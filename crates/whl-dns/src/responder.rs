use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::{DNSClass, RecordType};
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::KeyedLimiter;
use whl_store::Store;
use whl_store::dns::{
    DnsCaptureOutcome, DnsCapturePolicy, DnsEndpointState, DnsFlags, DnsMeta, DnsTransport,
    NewDnsCapture,
};
use whl_telemetry::series;

use crate::cache::{EndpointCache, Standing};
use crate::error::DnsError;
use crate::limits::{Admission, Everyone, ResponseLimiter};
use crate::query::{Parsed, Request};
use crate::settings::DnsSettings;
use crate::zone::{
    Target, Zone, bad_version, encode, encode_within, refusal, rejection_bytes, truncated,
    udp_limit,
};

const CLASS_TEXT: &str = "IN";
const FAILURE_LOG_INTERVAL: Duration = Duration::from_secs(10);
const POOL_RESERVE: u32 = 2;

#[derive(Clone)]
pub struct StorageProbe(Arc<dyn Fn() -> bool + Send + Sync>);

impl StorageProbe {
    pub fn new(probe: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(probe))
    }

    pub fn is_full(&self) -> bool {
        (self.0)()
    }
}

impl fmt::Debug for StorageProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StorageProbe")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
        }
    }

    fn stored(self) -> DnsTransport {
        match self {
            Self::Udp => DnsTransport::Udp,
            Self::Tcp => DnsTransport::Tcp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolver {
    pub address: IpAddr,
    pub transport: Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Captured,
    Answered,
    NxDomain,
    Refused,
    RateLimited,
    Dropped,
    ServFail,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Answered => "answered",
            Self::NxDomain => "nxdomain",
            Self::Refused => "refused",
            Self::RateLimited => "rate_limited",
            Self::Dropped => "dropped",
            Self::ServFail => "servfail",
        }
    }
}

pub fn count(outcome: Outcome, transport: Transport) {
    metrics::counter!(
        series::DNS_QUERIES_TOTAL,
        "result" => outcome.as_str(),
        "transport" => transport.as_str()
    )
    .increment(1);
}

struct Reply {
    outcome: Outcome,
    bytes: Option<Vec<u8>>,
}

impl Reply {
    fn message(
        outcome: Outcome,
        message: &Message,
        request: &Request,
        limit: Option<usize>,
    ) -> Self {
        Self {
            outcome,
            bytes: encode_within(message, request, limit),
        }
    }

    fn silent(outcome: Outcome) -> Self {
        Self {
            outcome,
            bytes: None,
        }
    }
}

#[derive(Debug)]
pub struct Responder {
    zone: Zone,
    zone_text: String,
    store: Store,
    policy: DnsCapturePolicy,
    cache: EndpointCache,
    responses: ResponseLimiter,
    captures: KeyedLimiter<EndpointId>,
    lookups: KeyedLimiter<Everyone>,
    database_permits: Semaphore,
    database_lookups: AtomicU64,
    storage_full: StorageProbe,
    started: Instant,
    last_failure_log: AtomicU64,
}

impl Responder {
    pub fn new(
        settings: &DnsSettings,
        store: Store,
        storage_full: StorageProbe,
    ) -> Result<Self, DnsError> {
        let limits = &settings.limits;
        let pool_size = store.pool().options().get_max_connections();
        let permits = limits
            .max_concurrent_stores
            .min(usize::try_from(pool_size.saturating_sub(POOL_RESERVE)).unwrap_or(usize::MAX))
            .max(1);
        Ok(Self {
            zone: Zone::new(
                settings.zone.clone(),
                settings.ns_name.clone(),
                settings.soa_rname.clone(),
                settings.serial,
                settings.answer_a,
                settings.answer_aaaa,
            ),
            zone_text: settings.zone_text(),
            store,
            policy: settings.policy,
            cache: EndpointCache::new(limits.cache_ttl, limits.stale_ttl, limits.cache_entries),
            responses: ResponseLimiter::new(settings.rate_source, settings.rate_global)?,
            captures: KeyedLimiter::new(
                settings.rate_capture_endpoint,
                whl_core::limiter::DEFAULT_MAX_KEYS,
            )?,
            lookups: KeyedLimiter::new(limits.lookup_rate, 1)?,
            database_permits: Semaphore::new(permits),
            database_lookups: AtomicU64::new(0),
            storage_full,
            started: Instant::now(),
            last_failure_log: AtomicU64::new(0),
        })
    }

    pub fn admit_datagram(&self, resolver: IpAddr) -> Admission {
        self.responses.admit_datagram(resolver)
    }

    pub fn admit_stream(&self) -> Admission {
        self.responses.admit_stream()
    }

    pub fn sweep(&self) {
        self.cache.sweep(Instant::now());
        self.responses.cleanup();
        self.captures.cleanup();
        self.lookups.cleanup();
    }

    pub fn database_lookups(&self) -> u64 {
        self.database_lookups.load(Ordering::Relaxed)
    }

    pub fn cached_endpoints(&self) -> usize {
        self.cache.len()
    }

    pub async fn respond(&self, parsed: Parsed, resolver: Resolver, slip: bool) -> Option<Vec<u8>> {
        let reply = self.resolve(parsed, resolver, slip).await;
        count(reply.outcome, resolver.transport);
        reply.bytes
    }

    async fn resolve(&self, parsed: Parsed, resolver: Resolver, slip: bool) -> Reply {
        let request = match parsed {
            Parsed::Drop => return Reply::silent(Outcome::Dropped),
            Parsed::Reject(rejection) => {
                return Reply {
                    outcome: Outcome::Refused,
                    bytes: Some(rejection_bytes(&rejection)),
                };
            }
            Parsed::Query(request) => request,
        };
        let limit = (resolver.transport == Transport::Udp).then(|| udp_limit(&request));
        if slip {
            return Reply {
                outcome: Outcome::RateLimited,
                bytes: encode(&truncated(&request)),
            };
        }
        if request.edns_version() != 0 {
            return Reply::message(Outcome::Refused, &bad_version(&request), &request, limit);
        }
        let refused =
            |code| Reply::message(Outcome::Refused, &refusal(&request, code), &request, limit);
        if request.query.query_class() != DNSClass::IN {
            return refused(ResponseCode::Refused);
        }
        match request.query.query_type() {
            RecordType::AXFR | RecordType::IXFR => return refused(ResponseCode::Refused),
            RecordType::OPT => return refused(ResponseCode::FormErr),
            _ => {}
        }
        match self.zone.classify(request.query.name()) {
            Target::Outside => refused(ResponseCode::Refused),
            Target::Apex => Reply::message(
                Outcome::Answered,
                &self.zone.apex_answer(&request),
                &request,
                limit,
            ),
            Target::Unknown => self.negative(&request, limit),
            Target::Captured { id, below } => {
                self.captured(&request, resolver, id, below, limit).await
            }
        }
    }

    fn negative(&self, request: &Request, limit: Option<usize>) -> Reply {
        Reply::message(
            Outcome::NxDomain,
            &self.zone.nxdomain(request),
            request,
            limit,
        )
    }

    fn server_failure(&self, request: &Request, limit: Option<usize>) -> Reply {
        Reply::message(
            Outcome::ServFail,
            &refusal(request, ResponseCode::ServFail),
            request,
            limit,
        )
    }

    fn answered(&self, request: &Request, limit: Option<usize>) -> Reply {
        Reply::message(
            Outcome::Answered,
            &self.zone.captured_answer(request, None),
            request,
            limit,
        )
    }

    async fn standing(&self, id: &EndpointId) -> Result<Standing, Outcome> {
        if let Some(standing) = self.cache.get(id, Instant::now()) {
            return Ok(standing);
        }
        if !self.lookups.check(&Everyone).is_allowed() {
            return self.stale_or(id, Outcome::RateLimited);
        }
        let Ok(_permit) = self.database_permits.try_acquire() else {
            return self.stale_or(id, Outcome::Dropped);
        };
        self.database_lookups.fetch_add(1, Ordering::Relaxed);
        let state = match self
            .store
            .dns_endpoint_state(id, self.policy.retention)
            .await
        {
            Ok(state) => state,
            Err(error) => {
                self.log_failure("the DNS endpoint lookup failed", &error);
                return Err(Outcome::ServFail);
            }
        };
        let standing = match state {
            DnsEndpointState::Enabled => Standing::Enabled,
            DnsEndpointState::Disabled | DnsEndpointState::Gone => Standing::Absent,
        };
        self.cache.put(id.clone(), standing, Instant::now());
        Ok(standing)
    }

    fn stale_or(&self, id: &EndpointId, refused: Outcome) -> Result<Standing, Outcome> {
        if self.cache.get_stale_enabled(id, Instant::now()) {
            Ok(Standing::Enabled)
        } else {
            Err(refused)
        }
    }

    fn log_failure(&self, what: &'static str, error: &dyn fmt::Display) {
        let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let interval = u64::try_from(FAILURE_LOG_INTERVAL.as_millis()).unwrap_or(u64::MAX);
        let last = self.last_failure_log.load(Ordering::Relaxed);
        let due = last == 0 || now.saturating_sub(last) >= interval;
        if due
            && self
                .last_failure_log
                .compare_exchange(last, now.max(1), Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!(%error, "{what}");
        }
    }

    async fn captured(
        &self,
        request: &Request,
        resolver: Resolver,
        id: EndpointId,
        below: String,
        limit: Option<usize>,
    ) -> Reply {
        match self.standing(&id).await {
            Ok(Standing::Enabled) => {}
            Ok(Standing::Absent) => return self.negative(request, limit),
            Err(Outcome::ServFail) => return self.server_failure(request, limit),
            Err(outcome) => return Reply::silent(outcome),
        }
        if self.storage_full.is_full() {
            return self.answered(request, limit);
        }
        if !self.captures.check(&id).is_allowed() {
            return self.answered(request, limit);
        }
        let Ok(_permit) = self.database_permits.try_acquire() else {
            return self.answered(request, limit);
        };
        let capture_id = match CaptureId::generate() {
            Ok(capture_id) => capture_id,
            Err(error) => {
                tracing::error!(%error, "a DNS capture id could not be generated");
                return self.server_failure(request, limit);
            }
        };
        let capture = NewDnsCapture {
            id: capture_id,
            endpoint_id: id.clone(),
            received_at: OffsetDateTime::now_utc(),
            path: below,
            zone: self.zone_text.clone(),
            resolver: resolver.address,
            meta: meta_of(request, resolver.transport),
        };
        match self.store.store_dns_capture(capture, self.policy).await {
            Ok(DnsCaptureOutcome::Stored { .. }) => Reply::message(
                Outcome::Captured,
                &self.zone.captured_answer(request, Some(capture_id)),
                request,
                limit,
            ),
            Ok(DnsCaptureOutcome::Full) => self.answered(request, limit),
            Ok(DnsCaptureOutcome::Gone) => {
                self.cache.put(id, Standing::Absent, Instant::now());
                self.negative(request, limit)
            }
            Err(error) => {
                self.log_failure("storing a DNS capture failed", &error);
                self.server_failure(request, limit)
            }
        }
    }
}

fn meta_of(request: &Request, transport: Transport) -> DnsMeta {
    DnsMeta {
        name: request.query.name().to_ascii(),
        record_type: type_text(request.query.query_type()),
        class: CLASS_TEXT.to_owned(),
        transport: transport.stored(),
        dns_id: request.id,
        flags: DnsFlags {
            rd: request.recursion_desired,
            cd: request.checking_disabled,
            dnssec_ok: request.dnssec_ok(),
        },
        edns_udp_size: request.edns.as_ref().map(|edns| edns.max_payload()),
        client_subnet: request.client_subnet(),
    }
}

fn type_text(record_type: RecordType) -> String {
    match record_type {
        RecordType::Unknown(code) => format!("TYPE{code}"),
        known => known.to_string(),
    }
}
