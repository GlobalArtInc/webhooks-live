use std::time::Duration;

use whl_core::config::Config;

pub const MEMBER_MAX_RETENTION: Duration = Duration::from_secs(365 * 24 * 3600);
pub const MEMBER_MAX_CAPTURES: u32 = 10_000;
pub const MEMBER_NEVER_EXPIRE_ENDPOINTS: u32 = 10;
pub const MEMBER_ALIASES: u32 = 5;
pub const DEFAULT_ALIAS_QUARANTINE: Duration = Duration::from_secs(30 * 24 * 3600);

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
        }
    }

    pub fn lifecycle_limits(&self, owned: bool) -> LifecycleCeilings {
        let instance_retention = i64::try_from(self.retention.as_secs()).unwrap_or(i64::MAX);
        if owned {
            let member_retention =
                i64::try_from(MEMBER_MAX_RETENTION.as_secs()).unwrap_or(i64::MAX);
            LifecycleCeilings {
                max_retention_seconds: member_retention.max(instance_retention),
                max_captures: MEMBER_MAX_CAPTURES.max(self.max_captures),
                never_expire_endpoints: MEMBER_NEVER_EXPIRE_ENDPOINTS,
            }
        } else {
            LifecycleCeilings {
                max_retention_seconds: instance_retention,
                max_captures: self.max_captures,
                never_expire_endpoints: 0,
            }
        }
    }
}
