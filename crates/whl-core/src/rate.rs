use std::str::FromStr;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatePeriod {
    Second,
    Minute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RateSpec {
    pub count: u32,
    pub period: RatePeriod,
    pub burst: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RateSpecError {
    #[error("rate spec {0:?} must have the form <count>/<s|m>:<burst>")]
    Shape(String),
    #[error("rate count in {0:?} must be a positive integer")]
    Count(String),
    #[error("rate unit in {0:?} must be s (per second) or m (per minute)")]
    Unit(String),
    #[error("rate burst in {0:?} must be a positive integer")]
    Burst(String),
}

impl FromStr for RateSpec {
    type Err = RateSpecError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (rate, burst) = text
            .split_once(':')
            .ok_or_else(|| RateSpecError::Shape(text.to_owned()))?;
        let (count, unit) = rate
            .split_once('/')
            .ok_or_else(|| RateSpecError::Shape(text.to_owned()))?;

        let count = match count.parse::<u32>() {
            Ok(count) if count > 0 => count,
            _ => return Err(RateSpecError::Count(text.to_owned())),
        };
        let period = match unit {
            "s" => RatePeriod::Second,
            "m" => RatePeriod::Minute,
            _ => return Err(RateSpecError::Unit(text.to_owned())),
        };
        let burst = match burst.parse::<u32>() {
            Ok(burst) if burst > 0 => burst,
            _ => return Err(RateSpecError::Burst(text.to_owned())),
        };

        Ok(Self {
            count,
            period,
            burst,
        })
    }
}

impl TryFrom<String> for RateSpec {
    type Error = RateSpecError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}
