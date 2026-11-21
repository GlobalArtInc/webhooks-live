use std::hash::Hash;
use std::time::{Duration, Instant};

use dashmap::DashMap;

const FULL_TABLE_RETRY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureSpec {
    pub max_failures: u32,
    pub window: Duration,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    failures: u32,
}

#[derive(Debug)]
pub(crate) struct FailureCounter<K: Hash + Eq + Clone> {
    cells: DashMap<K, Window>,
    spec: FailureSpec,
    max_keys: usize,
}

impl<K: Hash + Eq + Clone> FailureCounter<K> {
    pub(crate) fn new(spec: FailureSpec, max_keys: usize) -> Self {
        Self {
            cells: DashMap::new(),
            spec,
            max_keys,
        }
    }

    pub(crate) fn blocked_for(&self, key: &K) -> Option<Duration> {
        match self.cells.get(key) {
            Some(cell) => {
                let elapsed = cell.started.elapsed();
                if elapsed >= self.spec.window || cell.failures < self.spec.max_failures {
                    None
                } else {
                    Some(self.spec.window - elapsed)
                }
            }
            None if self.cells.len() >= self.max_keys => Some(FULL_TABLE_RETRY),
            None => None,
        }
    }

    pub(crate) fn record(&self, key: &K) {
        let now = Instant::now();
        if let Some(mut cell) = self.cells.get_mut(key) {
            if now.duration_since(cell.started) >= self.spec.window {
                *cell = Window {
                    started: now,
                    failures: 1,
                };
            } else {
                cell.failures = cell.failures.saturating_add(1);
            }
            return;
        }
        if self.cells.len() >= self.max_keys {
            return;
        }
        self.cells
            .entry(key.clone())
            .and_modify(|cell| cell.failures = cell.failures.saturating_add(1))
            .or_insert(Window {
                started: now,
                failures: 1,
            });
    }

    pub(crate) fn sweep(&self) {
        let window = self.spec.window;
        self.cells.retain(|_, cell| cell.started.elapsed() < window);
    }

    #[cfg(test)]
    pub(crate) fn tracked(&self) -> usize {
        self.cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(max_failures: u32, window: Duration, max_keys: usize) -> FailureCounter<u32> {
        FailureCounter::new(
            FailureSpec {
                max_failures,
                window,
            },
            max_keys,
        )
    }

    #[test]
    fn a_key_is_blocked_after_its_failures_and_other_keys_are_not() {
        let failures = counter(3, Duration::from_secs(60), 10);
        for _ in 0..2 {
            failures.record(&1);
        }
        assert_eq!(failures.blocked_for(&1), None);
        failures.record(&1);

        let wait = failures.blocked_for(&1).expect("the key is blocked");
        assert!(wait > Duration::from_secs(50) && wait <= Duration::from_secs(60));
        assert_eq!(failures.blocked_for(&2), None);
    }

    #[test]
    fn the_window_resets_the_count_and_the_sweep_drops_old_keys() {
        let failures = counter(1, Duration::from_millis(30), 10);
        failures.record(&1);
        assert!(failures.blocked_for(&1).is_some());

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(failures.blocked_for(&1), None);
        failures.sweep();
        assert_eq!(failures.tracked(), 0);
    }

    #[test]
    fn a_full_table_blocks_new_keys_and_keeps_serving_known_ones() {
        let failures = counter(5, Duration::from_secs(60), 2);
        failures.record(&1);
        failures.record(&2);

        assert!(failures.blocked_for(&3).is_some());
        failures.record(&3);
        assert_eq!(failures.tracked(), 2);
        assert_eq!(failures.blocked_for(&1), None);
    }
}
