use std::time::Duration;

use whl_core::tier::Tiers;

pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const DEFAULT_HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(60);
pub const MAX_CLAIM_BATCH: usize = 8;
pub const MAX_ATTEMPTS_LIMIT: u32 = 10;
pub const SUMMARY_LOOK_BACK_HOURS: i32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyLimits {
    pub network_per_hour: i32,
    pub global_per_hour: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("the concurrency limit must be at least 1")]
    Concurrency,
    #[error("the attempt limit must be between 1 and {MAX_ATTEMPTS_LIMIT}")]
    MaxAttempts,
    #[error("every hourly limit must be at least 1")]
    Limits,
    #[error("the poll interval must not be zero")]
    PollInterval,
}

#[derive(Debug, Clone)]
pub struct NotifySettings {
    pub concurrency: usize,
    pub max_attempts: u32,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub housekeeping_interval: Duration,
    pub limits: NotifyLimits,
    pub tiers: Tiers,
    pub public_url: String,
}

impl NotifySettings {
    pub fn new(
        public_url: String,
        limits: NotifyLimits,
        concurrency: usize,
        max_attempts: u32,
    ) -> Self {
        Self {
            concurrency,
            max_attempts,
            lease: DEFAULT_LEASE,
            poll_interval: DEFAULT_POLL_INTERVAL,
            housekeeping_interval: DEFAULT_HOUSEKEEPING_INTERVAL,
            limits,
            tiers: Tiers::default(),
            public_url,
        }
    }

    pub fn with_tiers(mut self, tiers: Tiers) -> Self {
        self.tiers = tiers;
        self
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.concurrency == 0 {
            return Err(SettingsError::Concurrency);
        }
        if self.max_attempts == 0 || self.max_attempts > MAX_ATTEMPTS_LIMIT {
            return Err(SettingsError::MaxAttempts);
        }
        if self.poll_interval.is_zero() {
            return Err(SettingsError::PollInterval);
        }
        let limits = self.limits;
        if limits.network_per_hour < 1 || limits.global_per_hour < 1 {
            return Err(SettingsError::Limits);
        }
        Ok(())
    }
}
