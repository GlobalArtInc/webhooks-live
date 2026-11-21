use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::OnceCell;
use whl_rules::CompiledRules;

pub(crate) const CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
const IDLE_TTL: Duration = Duration::from_secs(600);

type FlightKey = (String, i32);
type FlightCell = Arc<OnceCell<Arc<CompiledRules>>>;

struct Entry {
    version: i32,
    rules: Arc<CompiledRules>,
    weight: usize,
    tick: u64,
    used: Instant,
}

#[derive(Default)]
struct Entries {
    map: HashMap<String, Entry>,
    weight: usize,
    tick: u64,
}

impl Entries {
    fn remove(&mut self, endpoint: &str) {
        if let Some(removed) = self.map.remove(endpoint) {
            self.weight = self.weight.saturating_sub(removed.weight);
        }
    }

    fn purge_idle(&mut self, now: Instant) {
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

pub(crate) struct RuleCache {
    budget: usize,
    entries: Mutex<Entries>,
    flights: Mutex<HashMap<FlightKey, FlightCell>>,
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
        if flights
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cell))
        {
            flights.remove(&self.key);
        }
    }
}

impl RuleCache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            budget,
            entries: Mutex::new(Entries::default()),
            flights: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn lookup(&self, endpoint: &str, version: i32) -> Option<Arc<CompiledRules>> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.tick += 1;
        let tick = entries.tick;
        let stale = match entries.map.get_mut(endpoint) {
            Some(entry) if entry.version == version => {
                entry.tick = tick;
                entry.used = Instant::now();
                return Some(Arc::clone(&entry.rules));
            }
            Some(_) => true,
            None => false,
        };
        if stale {
            entries.remove(endpoint);
        }
        None
    }

    pub(crate) fn store(&self, endpoint: &str, version: i32, rules: Arc<CompiledRules>) {
        let weight = rules.weight_bytes();
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
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
        let cell = Arc::clone(
            self.flights
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key.clone())
                .or_default(),
        );
        Flight {
            cache: self,
            key,
            cell,
        }
    }

    #[cfg(test)]
    pub(crate) fn weight(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .weight
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, endpoint: &str) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map
            .contains_key(endpoint)
    }

    #[cfg(test)]
    pub(crate) fn flights_in_progress(&self) -> usize {
        self.flights
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use whl_rules::{References, RuleSource, validate_set};
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
        let weight = rules.weight_bytes();
        let cache = RuleCache::new(weight * 5 / 2);
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
    fn a_set_heavier_than_the_whole_budget_is_not_cached_and_frees_the_old_entry() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() - 1);

        cache.store("only", 1, rules);

        assert!(!cache.holds("only"));
        assert_eq!(cache.weight(), 0);
    }

    #[test]
    fn a_new_version_and_a_forgotten_endpoint_leave_nothing_behind() {
        let rules = compiled("^/a");
        let cache = RuleCache::new(rules.weight_bytes() * 4);
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
    fn a_flight_is_shared_by_concurrent_callers_and_removed_when_they_leave() {
        let cache = RuleCache::new(1024);
        let first = cache.flight("endpoint", 1);
        let second = cache.flight("endpoint", 1);

        assert!(std::ptr::eq(first.cell(), second.cell()));
        assert_eq!(cache.flights_in_progress(), 1);
        drop(first);
        drop(second);
        assert_eq!(cache.flights_in_progress(), 0);
    }
}
