use std::time::Duration;

use http::HeaderValue;
use ipnet::IpNet;

use crate::error::ForwardError;

pub const DEFAULT_MAX_REDIRECTS: u32 = 5;
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024;
pub const DEFAULT_USER_AGENT: &str = concat!("webhooks.live/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardening {
    pub refuse_dotless_hosts: bool,
    pub denied_ports: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct GuardConfig {
    pub allow: Vec<IpNet>,
    pub connect_timeout: Duration,
    pub hop_timeout: Duration,
    pub total_timeout: Duration,
    pub max_redirects: u32,
    pub max_response_bytes: usize,
    pub user_agent: String,
    pub hardening: Hardening,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            connect_timeout: Duration::from_secs(5),
            hop_timeout: Duration::from_secs(15),
            total_timeout: Duration::from_secs(15),
            max_redirects: DEFAULT_MAX_REDIRECTS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            hardening: Hardening::default(),
        }
    }
}

impl GuardConfig {
    pub(crate) fn validate(&self) -> Result<(), ForwardError> {
        let timeouts = [
            ("connect_timeout", self.connect_timeout),
            ("hop_timeout", self.hop_timeout),
            ("total_timeout", self.total_timeout),
        ];
        for (name, value) in timeouts {
            if value.is_zero() {
                return Err(ForwardError::Configuration(format!(
                    "{name} must not be zero"
                )));
            }
        }
        if HeaderValue::from_str(&self.user_agent).is_err() {
            return Err(ForwardError::Configuration(
                "user_agent is not a valid header value".to_owned(),
            ));
        }
        Ok(())
    }
}
