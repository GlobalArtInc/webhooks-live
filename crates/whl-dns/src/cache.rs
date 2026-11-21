use std::time::{Duration, Instant};

use dashmap::DashMap;
use whl_core::endpoint_id::EndpointId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Enabled,
    Absent,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    standing: Standing,
    fresh_until: Instant,
    keep_until: Instant,
}

#[derive(Debug)]
pub struct EndpointCache {
    entries: DashMap<EndpointId, Entry>,
    max_entries: usize,
    ttl: Duration,
    stale_ttl: Duration,
}

impl EndpointCache {
    pub fn new(ttl: Duration, stale_ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: DashMap::new(),
            max_entries,
            ttl,
            stale_ttl,
        }
    }

    pub fn get(&self, id: &EndpointId, now: Instant) -> Option<Standing> {
        let entry = self.entries.get(id)?;
        (entry.fresh_until > now).then_some(entry.standing)
    }

    pub fn get_stale_enabled(&self, id: &EndpointId, now: Instant) -> bool {
        self.entries
            .get(id)
            .is_some_and(|entry| entry.standing == Standing::Enabled && entry.keep_until > now)
    }

    pub fn put(&self, id: EndpointId, standing: Standing, now: Instant) -> bool {
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&id) {
            return false;
        }
        let fresh_until = now + self.ttl;
        let keep_until = match standing {
            Standing::Enabled => fresh_until + self.stale_ttl,
            Standing::Absent => fresh_until,
        };
        self.entries.insert(
            id,
            Entry {
                standing,
                fresh_until,
                keep_until,
            },
        );
        true
    }

    pub fn sweep(&self, now: Instant) {
        self.entries.retain(|_, entry| entry.keep_until > now);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
