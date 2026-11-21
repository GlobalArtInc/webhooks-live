use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;
use whl_core::config::{Config, is_valid_mail_domain};
use whl_store::captures::CapturePolicy;

use crate::error::MailSetupError;

pub const MAX_SUPPORTED_MESSAGE_BYTES: usize = 1 << 30;
pub const MIN_BUDGET_HEADROOM_BYTES: usize = 260 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDomain(String);

impl MailDomain {
    pub fn parse(text: &str) -> Result<Self, MailSetupError> {
        if !is_valid_mail_domain(text) {
            return Err(MailSetupError::Domain);
        }
        Ok(Self(text.to_ascii_lowercase()))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, candidate: &str) -> bool {
        self.0.eq_ignore_ascii_case(candidate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailLimits {
    pub max_message_bytes: usize,
    pub memory_budget: usize,
    pub max_line_bytes: usize,
    pub max_recipients: usize,
    pub max_commands: u32,
    pub max_errors: u32,
    pub messages_per_session: u32,
    pub max_sessions: usize,
    pub sessions_per_ip: usize,
    pub connections_per_minute_per_ip: u32,
    pub messages_per_minute_per_ip: u32,
    pub messages_per_hour_per_endpoint: u32,
    pub command_timeout: Duration,
    pub data_timeout: Duration,
    pub session_timeout: Duration,
    pub write_timeout: Duration,
    pub proxy_timeout: Duration,
    pub tls_timeout: Duration,
}

impl Default for MailLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 10 * 1024 * 1024,
            memory_budget: 256 * 1024 * 1024,
            max_line_bytes: 1024,
            max_recipients: 10,
            max_commands: 200,
            max_errors: 10,
            messages_per_session: 5,
            max_sessions: 200,
            sessions_per_ip: 5,
            connections_per_minute_per_ip: 60,
            messages_per_minute_per_ip: 30,
            messages_per_hour_per_endpoint: 30,
            command_timeout: Duration::from_secs(60),
            data_timeout: Duration::from_secs(120),
            session_timeout: Duration::from_secs(600),
            write_timeout: Duration::from_secs(30),
            proxy_timeout: Duration::from_secs(10),
            tls_timeout: Duration::from_secs(30),
        }
    }
}

impl MailLimits {
    pub fn validate(&self) -> Result<(), MailSetupError> {
        let positive = [
            self.max_message_bytes,
            self.memory_budget,
            self.max_line_bytes,
            self.max_recipients,
            self.max_sessions,
            self.sessions_per_ip,
        ];
        let counts = [
            self.max_commands,
            self.max_errors,
            self.messages_per_session,
            self.connections_per_minute_per_ip,
            self.messages_per_minute_per_ip,
            self.messages_per_hour_per_endpoint,
        ];
        let durations = [
            self.command_timeout,
            self.data_timeout,
            self.session_timeout,
            self.write_timeout,
            self.proxy_timeout,
            self.tls_timeout,
        ];
        if positive.contains(&0)
            || counts.contains(&0)
            || durations.iter().any(Duration::is_zero)
            || self.max_message_bytes > MAX_SUPPORTED_MESSAGE_BYTES
            || self.memory_budget
                < self
                    .max_message_bytes
                    .saturating_add(MIN_BUDGET_HEADROOM_BYTES)
        {
            return Err(MailSetupError::Limits);
        }
        Ok(())
    }
}

pub type StorageProbe = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone)]
pub struct MailSettings {
    pub listen_addr: SocketAddr,
    pub domain: MailDomain,
    pub trusted_proxies: Vec<IpNet>,
    pub tls: Option<TlsFiles>,
    pub limits: MailLimits,
    pub capture: CapturePolicy,
    pub storage_full: Option<StorageProbe>,
}

impl std::fmt::Debug for MailSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailSettings")
            .field("listen_addr", &self.listen_addr)
            .field("domain", &self.domain)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("tls", &self.tls)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl MailSettings {
    pub fn from_config(config: &Config) -> Result<Option<Self>, MailSetupError> {
        let mail = &config.mail;
        let Some(listen_addr) = mail.listen_addr else {
            return Ok(None);
        };
        let domain = mail.domain.as_deref().ok_or(MailSetupError::Domain)?;
        let tls = match (&mail.tls_cert, &mail.tls_key) {
            (Some(cert), Some(key)) => Some(TlsFiles {
                cert: cert.clone(),
                key: key.clone(),
            }),
            (None, None) => None,
            _ => return Err(MailSetupError::TlsFiles),
        };
        let body = config.capture.max_body_bytes.as_u64();
        let budget = config.capture.body_memory_budget.as_u64();
        let limits = MailLimits {
            max_message_bytes: usize::try_from(body).map_err(|_| MailSetupError::Limits)?,
            memory_budget: usize::try_from(budget).map_err(|_| MailSetupError::Limits)?,
            ..MailLimits::default()
        };
        limits.validate()?;
        Ok(Some(Self {
            listen_addr,
            domain: MailDomain::parse(domain)?,
            trusted_proxies: mail.trusted_proxies.clone(),
            tls,
            limits,
            capture: CapturePolicy {
                max_captures: config.capture.max_captures,
                retention: config.capture.retention,
            },
            storage_full: None,
        }))
    }

    pub fn with_storage_probe(mut self, probe: StorageProbe) -> Self {
        self.storage_full = Some(probe);
        self
    }
}
