use std::hash::Hash;
use std::net::IpAddr;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use governor::clock::{Clock, DefaultClock};
use governor::middleware::StateInformationMiddleware;
use governor::nanos::Nanos;
use governor::state::StateStore;
use governor::state::keyed::ShrinkableKeyedStateStore;
use governor::{Quota, RateLimiter};
use uuid::Uuid;

use crate::config::RateConfig;
use crate::endpoint_id::EndpointId;
use crate::rate::{RatePeriod, RateSpec};

pub const DEFAULT_MAX_KEYS: usize = 100_000;

const IPV6_PREFIX_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LimiterError {
    #[error("the rate count and burst of {0:?} must be positive")]
    ZeroRate(RateSpec),
    #[error("the limiter key capacity must be positive")]
    ZeroCapacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    V4(u32),
    V6Prefix(u64),
}

impl ClientKey {
    pub fn from_ip(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V4(v4) => Self::V4(u32::from(v4)),
            IpAddr::V6(v6) => {
                let octets = v6.octets();
                let mut prefix = [0_u8; IPV6_PREFIX_BYTES];
                prefix.copy_from_slice(&octets[..IPV6_PREFIX_BYTES]);
                Self::V6Prefix(u64::from_be_bytes(prefix))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed { limit: u32, remaining: u32 },
    Limited { limit: u32, retry_after: Duration },
}

impl Verdict {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }
}

pub fn whole_seconds_up(duration: Duration) -> u64 {
    let extra = u64::from(duration.subsec_nanos() > 0);
    duration.as_secs().saturating_add(extra).max(1)
}

#[derive(Debug)]
pub struct BoundedStore<K: Hash + Eq> {
    cells: DashMap<K, AtomicU64>,
    live: AtomicUsize,
    max_keys: usize,
}

impl<K: Hash + Eq> BoundedStore<K> {
    fn new(max_keys: usize) -> Self {
        Self {
            cells: DashMap::new(),
            live: AtomicUsize::new(0),
            max_keys,
        }
    }

    fn reserve(&self) -> bool {
        let previous = self.live.fetch_add(1, Ordering::AcqRel);
        if previous >= self.max_keys {
            self.live.fetch_sub(1, Ordering::AcqRel);
            return false;
        }
        true
    }
}

fn swap<T, E>(
    cell: &AtomicU64,
    decide: impl Fn(Option<Nanos>) -> Result<(T, Nanos), E>,
) -> Result<T, E> {
    let mut previous = cell.load(Ordering::Acquire);
    loop {
        let (outcome, next) =
            decide(NonZeroU64::new(previous).map(|nanos| Nanos::from(nanos.get())))?;
        match cell.compare_exchange_weak(
            previous,
            u64::from(next),
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(outcome),
            Err(actual) => previous = actual,
        }
    }
}

impl<K: Hash + Eq + Clone> StateStore for BoundedStore<K> {
    type Key = K;

    fn measure_and_replace<T, F, E>(&self, key: &Self::Key, decide: F) -> Result<T, E>
    where
        F: Fn(Option<Nanos>) -> Result<(T, Nanos), E>,
    {
        if let Some(cell) = self.cells.get(key) {
            return swap(&cell, decide);
        }
        if !self.reserve() {
            return decide(None).map(|(outcome, _)| outcome);
        }
        match self.cells.entry(key.clone()) {
            Entry::Occupied(occupied) => {
                self.live.fetch_sub(1, Ordering::AcqRel);
                swap(occupied.get(), decide)
            }
            Entry::Vacant(vacant) => {
                let inserted = vacant.insert(AtomicU64::new(0));
                swap(&inserted, decide)
            }
        }
    }
}

impl<K: Hash + Eq + Clone> ShrinkableKeyedStateStore<K> for BoundedStore<K> {
    fn retain_recent(&self, drop_below: Nanos) {
        let threshold = u64::from(drop_below);
        let mut removed = 0_usize;
        self.cells.retain(|_, cell| {
            let keep = cell.load(Ordering::Relaxed) > threshold;
            if !keep {
                removed += 1;
            }
            keep
        });
        self.live.fetch_sub(removed, Ordering::AcqRel);
    }

    fn shrink_to_fit(&self) {
        self.cells.shrink_to_fit();
    }

    fn len(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

type Gcra<K> = RateLimiter<K, BoundedStore<K>, DefaultClock, StateInformationMiddleware>;

#[derive(Debug)]
pub struct KeyedLimiter<K: Hash + Eq + Clone> {
    gcra: Gcra<K>,
    burst: u32,
    max_keys: usize,
}

impl<K: Hash + Eq + Clone> KeyedLimiter<K> {
    pub fn new(spec: RateSpec, max_keys: usize) -> Result<Self, LimiterError> {
        if max_keys == 0 {
            return Err(LimiterError::ZeroCapacity);
        }
        let (Some(count), Some(burst)) = (NonZeroU32::new(spec.count), NonZeroU32::new(spec.burst))
        else {
            return Err(LimiterError::ZeroRate(spec));
        };
        let quota = match spec.period {
            RatePeriod::Second => Quota::per_second(count),
            RatePeriod::Minute => Quota::per_minute(count),
        }
        .allow_burst(burst);
        let gcra = RateLimiter::new(quota, BoundedStore::new(max_keys), DefaultClock::default());
        Ok(Self {
            gcra,
            burst: burst.get(),
            max_keys,
        })
    }

    pub fn check(&self, key: &K) -> Verdict {
        match self.gcra.check_key(key) {
            Ok(snapshot) => Verdict::Allowed {
                limit: self.burst,
                remaining: snapshot.remaining_burst_capacity(),
            },
            Err(not_until) => Verdict::Limited {
                limit: self.burst,
                retry_after: not_until.wait_time_from(self.gcra.clock().now()),
            },
        }
    }

    pub fn cleanup(&self) {
        self.gcra.retain_recent();
        self.gcra.shrink_to_fit();
    }

    pub fn tracked_keys(&self) -> usize {
        self.gcra.len()
    }

    pub fn max_keys(&self) -> usize {
        self.max_keys
    }

    pub fn burst(&self) -> u32 {
        self.burst
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimiterSettings {
    pub capture_ip: RateSpec,
    pub capture_endpoint: RateSpec,
    pub api_ip: RateSpec,
    pub create_ip: RateSpec,
    pub max_keys: usize,
}

impl LimiterSettings {
    pub fn from_config(rate: &RateConfig) -> Self {
        Self {
            capture_ip: rate.capture_ip,
            capture_endpoint: rate.capture_endpoint,
            api_ip: rate.api_ip,
            create_ip: rate.create_ip,
            max_keys: DEFAULT_MAX_KEYS,
        }
    }
}

#[derive(Debug)]
struct Inner {
    capture_ip: KeyedLimiter<ClientKey>,
    capture_endpoint: KeyedLimiter<EndpointId>,
    api_ip: KeyedLimiter<ClientKey>,
    api_token: KeyedLimiter<Uuid>,
    create_ip: KeyedLimiter<ClientKey>,
}

#[derive(Debug, Clone)]
pub struct Limiters {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackedKeys {
    pub capture_ip: usize,
    pub capture_endpoint: usize,
    pub api_ip: usize,
    pub api_token: usize,
    pub create_ip: usize,
}

impl Limiters {
    pub fn new(settings: &LimiterSettings) -> Result<Self, LimiterError> {
        Ok(Self {
            inner: Arc::new(Inner {
                capture_ip: KeyedLimiter::new(settings.capture_ip, settings.max_keys)?,
                capture_endpoint: KeyedLimiter::new(settings.capture_endpoint, settings.max_keys)?,
                api_ip: KeyedLimiter::new(settings.api_ip, settings.max_keys)?,
                api_token: KeyedLimiter::new(settings.api_ip, settings.max_keys)?,
                create_ip: KeyedLimiter::new(settings.create_ip, settings.max_keys)?,
            }),
        })
    }

    pub fn check_capture_ip(&self, address: IpAddr) -> Verdict {
        self.inner.capture_ip.check(&ClientKey::from_ip(address))
    }

    pub fn check_capture_endpoint(&self, endpoint: &EndpointId) -> Verdict {
        self.inner.capture_endpoint.check(endpoint)
    }

    pub fn check_api_ip(&self, address: IpAddr) -> Verdict {
        self.inner.api_ip.check(&ClientKey::from_ip(address))
    }

    pub fn check_api_token(&self, token: Uuid) -> Verdict {
        self.inner.api_token.check(&token)
    }

    pub fn check_create_ip(&self, address: IpAddr) -> Verdict {
        self.inner.create_ip.check(&ClientKey::from_ip(address))
    }

    pub fn cleanup(&self) {
        self.inner.capture_ip.cleanup();
        self.inner.capture_endpoint.cleanup();
        self.inner.api_ip.cleanup();
        self.inner.api_token.cleanup();
        self.inner.create_ip.cleanup();
    }

    pub fn tracked_keys(&self) -> TrackedKeys {
        TrackedKeys {
            capture_ip: self.inner.capture_ip.tracked_keys(),
            capture_endpoint: self.inner.capture_endpoint.tracked_keys(),
            api_ip: self.inner.api_ip.tracked_keys(),
            api_token: self.inner.api_token.tracked_keys(),
            create_ip: self.inner.create_ip.tracked_keys(),
        }
    }
}
