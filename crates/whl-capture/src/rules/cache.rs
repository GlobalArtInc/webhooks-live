use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::OnceCell;
use whl_rules::{CompiledRules, Failure};

pub const DEFAULT_CACHE_BYTES: usize = 128 * 1024 * 1024;
const IDLE_TTL: Duration = Duration::from_secs(600);
const PURGE_INTERVAL: Duration = Duration::from_secs(10);
const FAILURE_TTL: Duration = Duration::from_secs(60);
const MAX_FAILURES: usize = 256;

type FlightKey = (String, i32);
type FlightCell = Arc<OnceCell<Arc<CompiledRules>>>;

struct Entry {
    version: i32,
    rules: Arc<CompiledRules>,
    weight: usize,
    tick: u64,
    used: Instant,
}

struct Entries {
    map: HashMap<String, Entry>,
    weight: usize,
    tick: u64,
    purged: Instant,
}

impl Entries {
    fn new(now: Instant) -> Self {
        Self {
            map: HashMap::new(),
            weight: 0,
            tick: 0,
            purged: now,
        }
    }

    fn remove(&mut self, endpoint: &str) {
        if let Some(removed) = self.map.remove(endpoint) {
            self.weight = self.weight.saturating_sub(removed.weight);
        }
    }

    fn purge_idle(&mut self, now: Instant) {
        self.purged = now;
        let idle: Vec<String> = self
            .map
            .iter()
            .filter(|(_, entry)| now.saturating_duration_since(entry.used) > IDLE_TTL)
            .map(|(endpoint, _)| endpoint.clone())
            .collect();
        for endpoint in idle {
            self.remove(&endpoint);
        }
    }

    fn evict_oldest(&mut self) -> bool {
        let oldest = self
            .map
            .iter()
            .min_by_key(|(_, entry)| entry.tick)
            .map(|(endpoint, _)| endpoint.clone());
        match oldest {
            Some(endpoint) => {
                self.remove(&endpoint);
                true
            }
            None => false,
        }
    }
}

struct FlightSlot {
    cell: FlightCell,
    waiters: usize,
}

pub(crate) struct RuleCache {
    budget: usize,
    per_pattern: usize,
    entries: Mutex<Entries>,
    flights: Mutex<HashMap<FlightKey, FlightSlot>>,
    failures: Mutex<HashMap<FlightKey, (Instant, Failure)>>,
}

pub(crate) struct Flight<'a> {
    cache: &'a RuleCache,
    key: FlightKey,
    cell: FlightCell,
}

impl Flight<'_> {
    pub(crate) fn cell(&self) -> &OnceCell<Arc<CompiledRules>> {
        &self.cell
    }
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let mut flights = self
            .cache
            .flights
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = flights.get_mut(&self.key) {
            slot.waiters = slot.waiters.saturating_sub(1);
            if slot.waiters == 0 {
                flights.remove(&self.key);
            }
        }
    }
}

impl RuleCache {
    pub(crate) fn new(budget: usize, per_pattern: usize) -> Self {
        Self {
            budget,
            per_pattern,
            entries: Mutex::new(Entries::new(Instant::now())),
            flights: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn weigh(&self, rules: &CompiledRules) -> usize {
        rules
            .weight_bytes()
            .saturating_add(rules.pattern_count().saturating_mul(self.per_pattern))
    }

    pub(crate) fn lookup(&self, endpoint: &str, version: i32) -> Option<Arc<CompiledRules>> {
        self.lookup_at(endpoint, version, Instant::now())
    }

    fn lookup_at(&self, endpoint: &str, version: i32, now: Instant) -> Option<Arc<CompiledRules>> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if now.saturating_duration_since(entries.purged) >= PURGE_INTERVAL {
            entries.purge_idle(now);
        }
        entries.tick += 1;
        let tick = entries.tick;
        let stale = match entries.map.get_mut(endpoint) {
            Some(entry) if entry.version == version => {
                entry.tick = tick;
                entry.used = now;
                return Some(Arc::clone(&entry.rules));
            }
            Some(entry) => entry.version < version,
            None => false,
        };
        if stale {
            entries.remove(endpoint);
        }
        None
    }

    pub(crate) fn store(&self, endpoint: &str, version: i32, rules: Arc<CompiledRules>) {
        let weight = self.weigh(&rules);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries
            .map
            .get(endpoint)
            .is_some_and(|held| held.version > version)
        {
            return;
        }
        entries.remove(endpoint);
        if weight > self.budget {
            return;
        }
        let now = Instant::now();
        entries.purge_idle(now);
        while entries.weight.saturating_add(weight) > self.budget && entries.evict_oldest() {}
        entries.tick += 1;
        let tick = entries.tick;
        entries.weight = entries.weight.saturating_add(weight);
        entries.map.insert(
            endpoint.to_owned(),
            Entry {
                version,
                rules,
                weight,
                tick,
                used: now,
            },
        );
    }

    pub(crate) fn forget(&self, endpoint: &str) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(endpoint);
    }

    pub(crate) fn flight(&self, endpoint: &str, version: i32) -> Flight<'_> {
        let key = (endpoint.to_owned(), version);
        let mut flights = self.flights.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = flights.entry(key.clone()).or_insert_with(|| FlightSlot {
            cell: FlightCell::default(),
            waiters: 0,
        });
        slot.waiters += 1;
        let cell = Arc::clone(&slot.cell);
        drop(flights);
        Flight {
            cache: self,
            key,
            cell,
        }
    }

    pub(crate) fn remember_failure(&self, endpoint: &str, version: i32, failure: &Failure) {
        self.remember_failure_at(endpoint, version, failure, Instant::now());
    }

    fn remember_failure_at(&self, endpoint: &str, version: i32, failure: &Failure, now: Instant) {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        if failures.len() >= MAX_FAILURES {
            failures.retain(|_, (since, _)| now.saturating_duration_since(*since) < FAILURE_TTL);
        }
        if failures.len() >= MAX_FAILURES {
            failures.clear();
        }
        failures.insert((endpoint.to_owned(), version), (now, failure.clone()));
    }

    pub(crate) fn failure(&self, endpoint: &str, version: i32) -> Option<Failure> {
        self.failure_at(endpoint, version, Instant::now())
    }

    fn failure_at(&self, endpoint: &str, version: i32, now: Instant) -> Option<Failure> {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (endpoint.to_owned(), version);
        match failures.get(&key) {
            Some((since, failure)) if now.saturating_duration_since(*since) < FAILURE_TTL => {
                Some(failure.clone())
            }
            Some(_) => {
                failures.remove(&key);
                None
            }
            None => None,
        }
    }

    #[cfg(test)]
    fn weight(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .weight
    }

    #[cfg(test)]
    fn holds(&self, endpoint: &str) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map
            .contains_key(endpoint)
    }

    #[cfg(test)]
    fn flights_in_progress(&self) -> usize {
        self.flights
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use whl_rules::{FailureCode, References, RuleSource, validate_set};
    use whl_script::{EngineLimits, ScriptCompiler};
    use whl_store::Uuid;

    use super::*;

    fn compiled(pattern: &str) -> Arc<CompiledRules> {
        let scripts = ScriptCompiler::new(EngineLimits::default()).expect("the limits are valid");
        let rules = validate_set(
            &[json!({
                "name": "rule",
                "condition": {"value": "request.path", "op": "matches", "arg": pattern},
                "steps": [{"kind": "stop"}],
            })],
            &References::default(),
            &scripts,
        )
        .expect("the rule is valid");
        let sources = rules
            .into_iter()
            .map(|rule| RuleSource {
                id: Uuid::nil(),
                rule,
            })
            .collect();
        Arc::new(CompiledRules::compile(sources, &scripts).expect("the rule compiles"))
    }

    #[test]
    fn the_cache_holds_to_its_byte_budget_and_evicts_the_least_recently_used_set() {
        let rules = compiled("^/a");
        let weight = rules.weight_bytes() + rules.pattern_count() * 1000;
        let cache = RuleCache::new(weight * 5 / 2, 1000);
        cache.store("first", 1, Arc::clone(&rules));
        cache.store("second", 1, Arc::clone(&rules));
        assert!(cache.lookup("first", 1).is_some());

        cache.store("third", 1, Arc::clone(&rules));

        assert!(cache.holds("first"));
        assert!(!cache.holds("second"));
        assert!(cache.holds("third"));
        assert!(cache.weight() <= weight * 5 / 2);
    }

    #[test]
    fn every_pattern_counts_the_caches_of_its_concurrent_users_against_the_budget() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() + 1000, 5000);

        cache.store("only", 1, Arc::clone(&rules));

        assert!(!cache.holds("only"));
        assert_eq!(cache.weight(), 0);
    }

    #[test]
    fn a_new_version_and_a_forgotten_endpoint_leave_nothing_behind() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() * 4, 0);
        cache.store("endpoint", 1, Arc::clone(&rules));

        assert!(cache.lookup("endpoint", 2).is_none());
        assert!(!cache.holds("endpoint"));
        assert_eq!(cache.weight(), 0);

        cache.store("endpoint", 2, rules);
        cache.forget("endpoint");
        assert!(!cache.holds("endpoint"));
        assert_eq!(cache.weight(), 0);
    }

    #[test]
    fn a_late_compile_of_an_old_version_does_not_replace_the_newer_entry() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() * 4, 0);
        cache.store("endpoint", 5, Arc::clone(&rules));

        cache.store("endpoint", 4, Arc::clone(&rules));

        assert!(cache.lookup("endpoint", 5).is_some());
        assert!(cache.lookup("endpoint", 4).is_none());
    }

    #[test]
    fn a_lookup_releases_idle_entries_without_any_store() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() * 4, 0);
        cache.store("idle", 1, Arc::clone(&rules));
        let later = Instant::now() + IDLE_TTL + PURGE_INTERVAL;

        assert!(cache.lookup_at("other", 1, later).is_none());

        assert!(!cache.holds("idle"));
        assert_eq!(cache.weight(), 0);
    }

    #[test]
    fn a_flight_is_shared_by_concurrent_callers_and_removed_when_the_last_one_leaves() {
        let cache = RuleCache::new(1024, 0);
        let first = cache.flight("endpoint", 1);
        let second = cache.flight("endpoint", 1);

        assert!(std::ptr::eq(first.cell(), second.cell()));
        drop(first);
        assert_eq!(cache.flights_in_progress(), 1);
        drop(second);
        assert_eq!(cache.flights_in_progress(), 0);
    }

    #[test]
    fn a_failed_compile_is_remembered_for_a_minute_and_then_forgotten() {
        let cache = RuleCache::new(1024, 0);
        let failure = Failure::new(FailureCode::Internal, "the stored rules no longer compile");
        let now = Instant::now();
        cache.remember_failure_at("endpoint", 3, &failure, now);

        assert_eq!(cache.failure_at("endpoint", 3, now), Some(failure));
        assert_eq!(cache.failure_at("endpoint", 4, now), None);
        assert_eq!(
            cache.failure_at("endpoint", 3, now + FAILURE_TTL + Duration::from_secs(1)),
            None
        );
    }
}
