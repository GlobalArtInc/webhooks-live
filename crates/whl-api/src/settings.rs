use std::time::Duration;

use whl_core::config::Config;
use whl_core::tier::Tier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleCeilings {
    pub max_retention_seconds: i64,
    pub max_captures: u32,
    pub never_expire_endpoints: u32,
}

#[derive(Debug, Clone)]
pub struct ApiSettings {
    pub public_url: String,
    pub retention: Duration,
    pub max_captures: u32,
    pub max_body_bytes: u64,
    pub forwarding: bool,
    pub private_forward_targets_allowed: bool,
    pub ring_mode_allowed: bool,
    pub export_concurrency: u32,
}

impl ApiSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            public_url: config.public_url.as_str().trim_end_matches('/').to_owned(),
            retention: config.capture.retention,
            max_captures: config.capture.max_captures,
            max_body_bytes: config.capture.max_body_bytes.as_u64(),
            forwarding: config.forward.enabled,
            private_forward_targets_allowed: !config.forward.allow_cidrs.is_empty(),
            ring_mode_allowed: config.capture.ring_mode_allowed,
            export_concurrency: config.export.concurrency,
        }
    }

    pub fn lifecycle_limits(&self, tier: Tier<'_>) -> LifecycleCeilings {
        let instance_retention = i64::try_from(self.retention.as_secs()).unwrap_or(i64::MAX);
        let limits = tier.limits();
        let (max_retention_seconds, max_captures) = if tier.is_anonymous() {
            (
                limits.max_retention_seconds().min(instance_retention),
                limits.max_captures.min(self.max_captures),
            )
        } else {
            (
                limits.max_retention_seconds().max(instance_retention),
                limits.max_captures.max(self.max_captures),
            )
        };
        LifecycleCeilings {
            max_retention_seconds,
            max_captures,
            never_expire_endpoints: limits.never_expire_endpoints,
        }
    }
}
