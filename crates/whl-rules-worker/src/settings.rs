use std::time::Duration;

use thiserror::Error;
use whl_core::tier::Tiers;
use whl_forward::RetryPolicy;
use whl_script::DEFAULT_MEMORY_BYTES;

pub const DEFAULT_CONCURRENCY: usize = 16;
pub const MAX_CONCURRENCY: usize = 256;
pub const MAX_CLAIM_BATCH: usize = 16;
pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const DEFAULT_MAX_CLAIMS: i32 = 100;
pub const DEFAULT_RUNS_PER_ENDPOINT: i32 = 2;
pub const MAX_OUTBOUND_STEPS_PER_RUN: usize = 20;
pub const MAX_ATTEMPTS_LIMIT: u32 = 10;
pub const STEP_DEADLINE: Duration = Duration::from_secs(30);
pub const MIN_LEASE_MARGIN: Duration = Duration::from_secs(5);
pub const DENIED_PORTS: [u16; 18] = [
    22, 23, 25, 110, 143, 465, 587, 993, 995, 2375, 2376, 3306, 5432, 6379, 9100, 10250, 11211,
    27017,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SettingsError {
    #[error("the concurrency must be between 1 and {MAX_CONCURRENCY}")]
    Concurrency,
    #[error("the attempt limit must be between 1 and {MAX_ATTEMPTS_LIMIT}")]
    MaxAttempts,
    #[error("the poll interval must not be zero")]
    PollInterval,
    #[error("the lease must exceed the step deadline by at least {MIN_LEASE_MARGIN:?}")]
    Lease,
    #[error("the claim limit and the runs per endpoint must be above zero")]
    Claims,
}

#[derive(Debug, Clone)]
pub struct WorkerSettings {
    pub concurrency: usize,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub max_claims: i32,
    pub runs_per_endpoint: i32,
    pub max_outbound_steps: usize,
    pub retry: RetryPolicy,
    pub tiers: Tiers,
    pub public_url: String,
    pub retention: Duration,
    pub script_memory_bytes: usize,
}

impl WorkerSettings {
    pub fn new(
        public_url: impl Into<String>,
        retention: Duration,
        concurrency: usize,
        max_attempts: u32,
        tiers: Tiers,
    ) -> Self {
        Self {
            concurrency,
            lease: DEFAULT_LEASE,
            poll_interval: DEFAULT_POLL_INTERVAL,
            max_claims: DEFAULT_MAX_CLAIMS,
            runs_per_endpoint: DEFAULT_RUNS_PER_ENDPOINT,
            max_outbound_steps: MAX_OUTBOUND_STEPS_PER_RUN,
            retry: RetryPolicy::new(max_attempts),
            tiers,
            public_url: public_url.into(),
            retention,
            script_memory_bytes: DEFAULT_MEMORY_BYTES,
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.concurrency == 0 || self.concurrency > MAX_CONCURRENCY {
            return Err(SettingsError::Concurrency);
        }
        if self.retry.max_attempts == 0 || self.retry.max_attempts > MAX_ATTEMPTS_LIMIT {
            return Err(SettingsError::MaxAttempts);
        }
        if self.poll_interval.is_zero() {
            return Err(SettingsError::PollInterval);
        }
        if self.lease < STEP_DEADLINE.saturating_add(MIN_LEASE_MARGIN) {
            return Err(SettingsError::Lease);
        }
        if self.max_claims < 1 || self.runs_per_endpoint < 1 {
            return Err(SettingsError::Claims);
        }
        Ok(())
    }
}
