use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::env::MonotonicClock;

#[derive(Debug)]
struct Spent {
    previous: Duration,
    total: Duration,
}

#[derive(Debug)]
pub struct ResponseBudget {
    allowance: Duration,
    spent: Mutex<Spent>,
}

impl ResponseBudget {
    pub fn start(clock: &dyn MonotonicClock, allowance: Duration) -> Self {
        Self {
            allowance,
            spent: Mutex::new(Spent {
                previous: clock.elapsed(),
                total: Duration::ZERO,
            }),
        }
    }

    pub fn allowance(&self) -> Duration {
        self.allowance
    }

    pub fn remaining(&self, clock: &dyn MonotonicClock) -> Duration {
        let reading = clock.elapsed();
        let mut spent = self.spent.lock().unwrap_or_else(PoisonError::into_inner);
        spent.total = spent
            .total
            .saturating_add(reading.saturating_sub(spent.previous));
        spent.previous = reading;
        self.allowance.saturating_sub(spent.total)
    }
}
