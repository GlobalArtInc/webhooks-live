use std::time::Duration;

use whl_core::entropy::random_bytes;

const MIDPOINT: f64 = 0.5;
const MANTISSA_BITS: u32 = 53;

fn unit() -> f64 {
    match random_bytes::<8>() {
        Ok(bytes) => {
            let mantissa = u64::from_le_bytes(bytes) >> (u64::BITS - MANTISSA_BITS);
            mantissa as f64 / (1_u64 << MANTISSA_BITS) as f64
        }
        Err(error) => {
            tracing::debug!(%error, "no entropy for jitter, using the midpoint");
            MIDPOINT
        }
    }
}

pub(crate) fn spread(base: Duration, fraction: f64, unit: f64) -> Duration {
    let offset = 2.0 * unit.clamp(0.0, 1.0) - 1.0;
    base.mul_f64((1.0 + fraction * offset).max(0.0))
}

pub(crate) fn jittered(base: Duration, fraction: f64) -> Duration {
    spread(base, fraction, unit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_within_the_fraction_on_both_sides() {
        let base = Duration::from_secs(100);
        assert_eq!(spread(base, 0.2, 0.0), Duration::from_secs(80));
        assert_eq!(spread(base, 0.2, 0.5), base);
        assert_eq!(spread(base, 0.2, 1.0), Duration::from_secs(120));
        assert_eq!(spread(base, 0.2, 7.0), Duration::from_secs(120));
    }

    #[test]
    fn random_jitter_stays_in_bounds_and_varies() {
        let base = Duration::from_secs(100);
        let samples: Vec<Duration> = (0..200).map(|_| jittered(base, 0.1)).collect();
        assert!(samples.iter().all(
            |sample| *sample >= Duration::from_secs(90) && *sample <= Duration::from_secs(110)
        ));
        assert!(samples.iter().any(|sample| *sample != samples[0]));
    }
}
