use std::time::Duration;

use rand::TryRngCore;
use rand::rngs::OsRng;

pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_secs(10);
pub const DEFAULT_BACKOFF_MULTIPLIER: u32 = 6;
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(3600);
pub const DEFAULT_JITTER_PERCENT: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub multiplier: u32,
    pub max_backoff: Duration,
    pub jitter_percent: u32,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            max_backoff: DEFAULT_MAX_BACKOFF,
            jitter_percent: DEFAULT_JITTER_PERCENT,
        }
    }

    pub fn has_attempts_left(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    pub fn base_delay(&self, attempt: u32) -> Duration {
        let mut delay = self.initial_backoff;
        for _ in 1..attempt {
            delay = delay.saturating_mul(self.multiplier);
            if delay >= self.max_backoff {
                return self.max_backoff;
            }
        }
        delay.min(self.max_backoff)
    }

    pub fn delay_after(&self, attempt: u32) -> Duration {
        self.jittered(attempt, random_unit())
    }

    pub fn jittered(&self, attempt: u32, unit: f64) -> Duration {
        let base = self.base_delay(attempt);
        let spread = f64::from(self.jitter_percent.min(100)) / 100.0;
        let factor = 1.0 + spread * unit.clamp(-1.0, 1.0);
        base.mul_f64(factor)
    }
}

fn random_unit() -> f64 {
    let mut bytes = [0_u8; 4];
    if OsRng.try_fill_bytes(&mut bytes).is_err() {
        return 0.0;
    }
    f64::from(u32::from_le_bytes(bytes)) / f64::from(u32::MAX) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_by_the_multiplier_from_the_initial_backoff() {
        let policy = RetryPolicy::new(5);
        assert_eq!(policy.base_delay(1), Duration::from_secs(10));
        assert_eq!(policy.base_delay(2), Duration::from_secs(60));
        assert_eq!(policy.base_delay(3), Duration::from_secs(360));
    }

    #[test]
    fn never_exceeds_the_maximum_backoff() {
        let policy = RetryPolicy::new(50);
        assert_eq!(policy.base_delay(10), DEFAULT_MAX_BACKOFF);
        assert_eq!(policy.base_delay(40), DEFAULT_MAX_BACKOFF);
    }

    #[test]
    fn jitter_stays_within_the_configured_spread() {
        let policy = RetryPolicy::new(3);
        assert_eq!(policy.jittered(1, -1.0), Duration::from_secs(8));
        assert_eq!(policy.jittered(1, 0.0), Duration::from_secs(10));
        assert_eq!(policy.jittered(1, 1.0), Duration::from_secs(12));
        assert_eq!(policy.jittered(1, 50.0), Duration::from_secs(12));
        for _ in 0..200 {
            let delay = policy.delay_after(2);
            assert!(delay >= Duration::from_secs(48) && delay <= Duration::from_secs(72));
        }
    }

    #[test]
    fn stops_when_the_attempt_budget_is_spent() {
        let policy = RetryPolicy::new(3);
        assert!(policy.has_attempts_left(1));
        assert!(policy.has_attempts_left(2));
        assert!(!policy.has_attempts_left(3));
    }
}
