use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use whl_core::alias::{Alias, POSITIVE_CACHE_TTL};
use whl_core::endpoint_id::EndpointId;

pub const ALIAS_POSITIVE_TTL: Duration = POSITIVE_CACHE_TTL;
pub const ALIAS_NEGATIVE_TTL: Duration = Duration::from_secs(5);
pub const ALIAS_CACHE_CAPACITY: usize = 10_000;
pub const ALIAS_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachedAlias {
    Miss,
    Unknown,
    Endpoint(EndpointId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AliasCacheSettings {
    pub positive_ttl: Duration,
    pub negative_ttl: Duration,
    pub capacity: usize,
    pub sweep_interval: Duration,
}

impl Default for AliasCacheSettings {
    fn default() -> Self {
        Self {
            positive_ttl: ALIAS_POSITIVE_TTL,
            negative_ttl: ALIAS_NEGATIVE_TTL,
            capacity: ALIAS_CACHE_CAPACITY,
            sweep_interval: ALIAS_SWEEP_INTERVAL,
        }
    }
}

#[derive(Debug)]
struct Entry {
    target: Option<EndpointId>,
    expires: Instant,
}

#[derive(Debug, Default)]
struct Table {
    entries: HashMap<String, Entry>,
    last_sweep: Option<Instant>,
}

#[derive(Debug)]
struct Shared {
    settings: AliasCacheSettings,
    table: Mutex<Table>,
}

#[derive(Debug, Clone)]
pub struct AliasCache {
    shared: Arc<Shared>,
}

impl Default for AliasCache {
    fn default() -> Self {
        Self::new(AliasCacheSettings::default())
    }
}

impl AliasCache {
    pub fn new(settings: AliasCacheSettings) -> Self {
        Self {
            shared: Arc::new(Shared {
                settings,
                table: Mutex::new(Table::default()),
            }),
        }
    }

    pub fn get(&self, alias: &Alias) -> CachedAlias {
        let now = Instant::now();
        let mut table = self
            .shared
            .table
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entries = &mut table.entries;
        match entries.get(alias.unpack()) {
            Some(entry) if entry.expires > now => match &entry.target {
                Some(id) => CachedAlias::Endpoint(id.clone()),
                None => CachedAlias::Unknown,
            },
            Some(_) => {
                entries.remove(alias.unpack());
                CachedAlias::Miss
            }
            None => CachedAlias::Miss,
        }
    }

    pub fn remember(&self, alias: &Alias, target: Option<EndpointId>) {
        let settings = &self.shared.settings;
        let ttl = if target.is_some() {
            settings.positive_ttl
        } else {
            settings.negative_ttl
        };
        if ttl.is_zero() {
            return;
        }
        let now = Instant::now();
        let mut table = self
            .shared
            .table
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if table.entries.len() >= settings.capacity && !table.entries.contains_key(alias.unpack()) {
            let sweep_due = table
                .last_sweep
                .is_none_or(|last| now.duration_since(last) >= settings.sweep_interval);
            if !sweep_due {
                return;
            }
            table.last_sweep = Some(now);
            table.entries.retain(|_, entry| entry.expires > now);
            if table.entries.len() >= settings.capacity {
                return;
            }
        }
        table.entries.insert(
            alias.unpack().to_owned(),
            Entry {
                target,
                expires: now + ttl,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.shared
            .table
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(text: &str) -> Alias {
        Alias::parse(text).expect("a valid alias")
    }

    fn id() -> EndpointId {
        EndpointId::parse("7k2m9q4xw8c1d5f3h6j0n2p4").expect("a valid id")
    }

    #[test]
    fn a_remembered_alias_is_served_until_its_time_runs_out() {
        let cache = AliasCache::default();
        let hit = alias("hit-me");
        let gone = alias("gone-name");
        assert_eq!(cache.get(&hit), CachedAlias::Miss);
        cache.remember(&hit, Some(id()));
        cache.remember(&gone, None);
        assert_eq!(cache.get(&hit), CachedAlias::Endpoint(id()));
        assert_eq!(cache.get(&gone), CachedAlias::Unknown);

        let short = AliasCache::new(AliasCacheSettings {
            positive_ttl: Duration::from_millis(20),
            negative_ttl: Duration::from_millis(20),
            capacity: 10,
            sweep_interval: Duration::ZERO,
        });
        short.remember(&hit, Some(id()));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(short.get(&hit), CachedAlias::Miss);
        assert!(short.is_empty());
    }

    #[test]
    fn the_cache_stays_within_its_capacity_under_a_flood_of_names() {
        let cache = AliasCache::new(AliasCacheSettings {
            positive_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_secs(60),
            capacity: 3,
            sweep_interval: Duration::ZERO,
        });
        for index in 0..20 {
            cache.remember(&alias(&format!("name-{index}")), None);
        }
        assert_eq!(cache.len(), 3);
        cache.remember(&alias("name-0"), Some(id()));
        assert_eq!(cache.get(&alias("name-0")), CachedAlias::Endpoint(id()));
    }

    #[test]
    fn a_full_cache_sweeps_expired_entries_at_most_once_per_interval() {
        let settings = |sweep_interval| AliasCacheSettings {
            positive_ttl: Duration::from_millis(300),
            negative_ttl: Duration::from_millis(300),
            capacity: 2,
            sweep_interval,
        };
        let sweeping = AliasCache::new(settings(Duration::ZERO));
        let throttled = AliasCache::new(settings(Duration::from_secs(3600)));
        for cache in [&sweeping, &throttled] {
            cache.remember(&alias("first-name"), None);
            cache.remember(&alias("second-name"), None);
            cache.remember(&alias("early-name"), Some(id()));
            assert_eq!(cache.get(&alias("early-name")), CachedAlias::Miss);
        }
        std::thread::sleep(Duration::from_millis(600));

        sweeping.remember(&alias("late-name"), Some(id()));
        assert_eq!(
            sweeping.get(&alias("late-name")),
            CachedAlias::Endpoint(id())
        );

        throttled.remember(&alias("late-name"), Some(id()));
        assert_eq!(throttled.get(&alias("late-name")), CachedAlias::Miss);
        assert_eq!(throttled.len(), 2);
    }
}
