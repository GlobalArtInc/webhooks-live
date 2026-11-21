use std::time::Duration;

use whl_core::config::Config;

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
}
