use std::hash::{BuildHasher, RandomState};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use whl_core::limiter::{DEFAULT_MAX_KEYS, KeyedLimiter, LimiterError};
use whl_core::rate::RateSpec;

const V4_NETWORK_MASK: u32 = 0xffff_ff00;
const V6_NETWORK_BYTES: usize = 7;
const SLIP_BUCKETS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkKey {
    V4(u32),
    V6(u64),
}

impl NetworkKey {
    pub fn from_ip(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V4(v4) => Self::V4(u32::from(v4) & V4_NETWORK_MASK),
            IpAddr::V6(v6) => {
                let octets = v6.octets();
                let mut prefix = [0_u8; 8];
                prefix[1..=V6_NETWORK_BYTES].copy_from_slice(&octets[..V6_NETWORK_BYTES]);
                Self::V6(u64::from_be_bytes(prefix))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Everyone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Answer,
    Slip,
    Drop,
}

#[derive(Debug)]
pub struct ResponseLimiter {
    source: KeyedLimiter<NetworkKey>,
    global: KeyedLimiter<Everyone>,
    slips: Box<[AtomicU64]>,
    hasher: RandomState,
}

impl ResponseLimiter {
    pub fn new(source: RateSpec, global: RateSpec) -> Result<Self, LimiterError> {
        Ok(Self {
            source: KeyedLimiter::new_strict(source, DEFAULT_MAX_KEYS)?,
            global: KeyedLimiter::new(global, 1)?,
            slips: (0..SLIP_BUCKETS).map(|_| AtomicU64::new(0)).collect(),
            hasher: RandomState::new(),
        })
    }

    pub fn admit_datagram(&self, resolver: IpAddr) -> Admission {
        let key = NetworkKey::from_ip(resolver);
        if !self.source.check(&key).is_allowed() {
            return if self.slip_due(&key) {
                Admission::Slip
            } else {
                Admission::Drop
            };
        }
        self.admit_stream()
    }

    pub fn admit_stream(&self) -> Admission {
        if self.global.check(&Everyone).is_allowed() {
            Admission::Answer
        } else {
            Admission::Drop
        }
    }

    pub fn cleanup(&self) {
        self.source.cleanup();
        self.global.cleanup();
    }

    pub fn tracked_sources(&self) -> usize {
        self.source.tracked_keys()
    }

    fn slip_due(&self, key: &NetworkKey) -> bool {
        let bucket =
            usize::try_from(self.hasher.hash_one(key) % SLIP_BUCKETS as u64).unwrap_or_default();
        self.slips[bucket]
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(2)
    }
}

#[derive(Debug)]
pub struct ConnectionTable {
    counts: DashMap<IpAddr, usize>,
    per_address: usize,
}

#[derive(Debug)]
pub struct ConnectionGuard {
    table: Arc<ConnectionTable>,
    address: IpAddr,
}

impl ConnectionTable {
    pub fn new(per_address: usize) -> Arc<Self> {
        Arc::new(Self {
            counts: DashMap::new(),
            per_address,
        })
    }

    pub fn enter(self: &Arc<Self>, address: IpAddr) -> Option<ConnectionGuard> {
        match self.counts.entry(address) {
            Entry::Occupied(mut occupied) => {
                if *occupied.get() >= self.per_address {
                    return None;
                }
                *occupied.get_mut() += 1;
            }
            Entry::Vacant(vacant) => {
                if self.per_address == 0 {
                    return None;
                }
                vacant.insert(1);
            }
        }
        Some(ConnectionGuard {
            table: Arc::clone(self),
            address,
        })
    }

    pub fn tracked_addresses(&self) -> usize {
        self.counts.len()
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Entry::Occupied(mut occupied) = self.table.counts.entry(self.address) {
            if *occupied.get() <= 1 {
                occupied.remove();
            } else {
                *occupied.get_mut() -= 1;
            }
        }
    }
}
