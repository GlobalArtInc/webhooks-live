use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::ClientKey;

const UNIT_BYTES: usize = 1024;
const RESERVE_STEP_BYTES: usize = 256 * 1024;
const SECONDS_PER_HOUR: u64 = 3600;
pub const MAX_TRACKED_ENDPOINTS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct MemoryBudget {
    semaphore: Arc<Semaphore>,
}

impl MemoryBudget {
    pub fn new(total_bytes: usize) -> Self {
        let units = (total_bytes / UNIT_BYTES).clamp(1, Semaphore::MAX_PERMITS);
        Self {
            semaphore: Arc::new(Semaphore::new(units)),
        }
    }

    pub fn reservation(&self) -> Reservation {
        Reservation {
            budget: self.clone(),
            permit: None,
            reserved: 0,
        }
    }
}

#[derive(Debug)]
pub struct Reservation {
    budget: MemoryBudget,
    permit: Option<OwnedSemaphorePermit>,
    reserved: usize,
}

impl Reservation {
    pub fn ensure(&mut self, needed_bytes: usize) -> bool {
        while self.reserved < needed_bytes {
            let units = u32::try_from(RESERVE_STEP_BYTES / UNIT_BYTES).unwrap_or(u32::MAX);
            let Ok(granted) = Arc::clone(&self.budget.semaphore).try_acquire_many_owned(units)
            else {
                return false;
            };
            match self.permit.as_mut() {
                Some(permit) => permit.merge(granted),
                None => self.permit = Some(granted),
            }
            self.reserved += RESERVE_STEP_BYTES;
        }
        true
    }
}

#[derive(Debug)]
pub struct IpSessions {
    counts: DashMap<ClientKey, usize>,
    limit: usize,
}

impl IpSessions {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            counts: DashMap::new(),
            limit,
        })
    }

    pub fn acquire(self: &Arc<Self>, address: IpAddr) -> Option<IpSessionGuard> {
        let key = ClientKey::from_ip(address);
        match self.counts.entry(key) {
            Entry::Occupied(mut occupied) => {
                if *occupied.get() >= self.limit {
                    return None;
                }
                *occupied.get_mut() += 1;
            }
            Entry::Vacant(vacant) => {
                vacant.insert(1);
            }
        }
        Some(IpSessionGuard {
            owner: Arc::clone(self),
            key,
        })
    }
}

#[derive(Debug)]
pub struct IpSessionGuard {
    owner: Arc<IpSessions>,
    key: ClientKey,
}

impl Drop for IpSessionGuard {
    fn drop(&mut self) {
        if let Entry::Occupied(mut occupied) = self.owner.counts.entry(self.key) {
            if *occupied.get() <= 1 {
                occupied.remove();
            } else {
                *occupied.get_mut() -= 1;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Window {
    hour: u64,
    count: u32,
}

#[derive(Debug)]
pub struct EndpointQuota {
    limit: u32,
    windows: Mutex<HashMap<EndpointId, Window>>,
}

impl EndpointQuota {
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn has_room(&self, endpoint: &EndpointId) -> bool {
        let hour = current_hour();
        let windows = self.windows.lock().unwrap_or_else(PoisonError::into_inner);
        windows
            .get(endpoint)
            .is_none_or(|window| window.hour != hour || window.count < self.limit)
    }

    pub fn take(&self, endpoint: &EndpointId) -> bool {
        let hour = current_hour();
        let mut windows = self.windows.lock().unwrap_or_else(PoisonError::into_inner);
        if !windows.contains_key(endpoint) && windows.len() >= MAX_TRACKED_ENDPOINTS {
            windows.retain(|_, window| window.hour == hour);
            if windows.len() >= MAX_TRACKED_ENDPOINTS {
                return false;
            }
        }
        let window = windows
            .entry(endpoint.clone())
            .or_insert(Window { hour, count: 0 });
        if window.hour != hour {
            *window = Window { hour, count: 0 };
        }
        if window.count >= self.limit {
            return false;
        }
        window.count += 1;
        true
    }
}

fn current_hour() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / SECONDS_PER_HOUR)
}

pub const NEGATIVE_TTL: Duration = Duration::from_secs(5);
pub const MAX_NEGATIVE_ENTRIES: usize = 10_000;

#[derive(Debug, Default)]
pub struct NegativeCache {
    entries: Mutex<HashMap<EndpointId, Instant>>,
}

impl NegativeCache {
    pub fn contains(&self, endpoint: &EndpointId) -> bool {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .get(endpoint)
            .is_some_and(|expires| *expires > Instant::now())
    }

    pub fn insert(&self, endpoint: &EndpointId) {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= MAX_NEGATIVE_ENTRIES && !entries.contains_key(endpoint) {
            entries.retain(|_, expires| *expires > now);
            if entries.len() >= MAX_NEGATIVE_ENTRIES {
                return;
            }
        }
        entries.insert(endpoint.clone(), now + NEGATIVE_TTL);
    }
}
