use std::time::Duration;

use rand::TryRngCore;
use rand::rngs::OsRng;

pub const INITIAL_DELAY: Duration = Duration::from_secs(1);
pub const MAX_DELAY: Duration = Duration::from_secs(30);
pub const JITTER_FRACTION: f64 = 0.25;

const MIDPOINT: f64 = 0.5;
const MANTISSA_BITS: u32 = 53;

fn unit() -> f64 {
    let mut bytes = [0_u8; 8];
    match OsRng.try_fill_bytes(&mut bytes) {
        Ok(()) => {
            let mantissa = u64::from_le_bytes(bytes) >> (u64::BITS - MANTISSA_BITS);
            mantissa as f64 / (1_u64 << MANTISSA_BITS) as f64
        }
        Err(_) => MIDPOINT,
    }
}

pub fn spread(base: Duration, fraction: f64, unit: f64) -> Duration {
    let offset = 2.0 * unit.clamp(0.0, 1.0) - 1.0;
    base.mul_f64((1.0 + fraction * offset).max(0.0))
}

#[derive(Debug, Clone)]
pub struct Backoff {
    current: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

impl Backoff {
    pub fn new() -> Self {
        Self {
            current: INITIAL_DELAY,
        }
    }

    pub fn reset(&mut self) {
        self.current = INITIAL_DELAY;
    }

    pub fn next_delay(&mut self, floor: Option<Duration>) -> Duration {
        self.next_with(floor, unit())
    }

    pub fn next_with(&mut self, floor: Option<Duration>, unit: f64) -> Duration {
        let delay = spread(self.current, JITTER_FRACTION, unit);
        self.current = (self.current * 2).min(MAX_DELAY);
        floor.map_or(delay, |floor| delay.max(floor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_from_one_second_up_to_the_cap() {
        let mut backoff = Backoff::new();
        let delays: Vec<u64> = (0..7)
            .map(|_| backoff.next_with(None, MIDPOINT).as_secs())
            .collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn spreads_the_delay_within_the_jitter_fraction() {
        let low = spread(Duration::from_secs(4), JITTER_FRACTION, 0.0);
        let high = spread(Duration::from_secs(4), JITTER_FRACTION, 1.0);
        assert_eq!(low, Duration::from_secs(3));
        assert_eq!(high, Duration::from_secs(5));
    }

    #[test]
    fn starts_over_after_a_reset() {
        let mut backoff = Backoff::new();
        backoff.next_with(None, MIDPOINT);
        backoff.next_with(None, MIDPOINT);
        backoff.reset();
        assert_eq!(backoff.next_with(None, MIDPOINT), INITIAL_DELAY);
    }

    #[test]
    fn honours_a_longer_retry_after() {
        let mut backoff = Backoff::new();
        let delay = backoff.next_with(Some(Duration::from_secs(20)), MIDPOINT);
        assert_eq!(delay, Duration::from_secs(20));
    }
}
