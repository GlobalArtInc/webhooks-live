mod de;

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use bytesize::ByteSize;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use ipnet::IpNet;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::rate::RateSpec;

const FILE_ENV: &str = "WEBHOOKS_LIVE_CONFIG";
const ENV_PREFIX: &str = "WEBHOOKS_LIVE_";

pub const MIN_DB_POOL_SIZE: u32 = 2;

const DEFAULTS: &str = r#"
listen_addr = "0.0.0.0:8080"
admin_addr = "0.0.0.0:9090"
log = "info"
trusted_proxies = []

[database]
pool_size = 20

[auth]
mode = "open"
session_idle = "12h"
session_max = "7d"
max_pending_logins = 50000

[auth.oidc]
scopes = ["openid", "email", "profile"]

[capture]
retention = "7d"
max_captures = 500
max_body_bytes = "10MiB"
body_memory_budget = "256MiB"
max_delayed = 2000
ring_mode_allowed = true
storage_cap_bytes = "20GiB"

[rate]
capture_ip = "30/s:60"
capture_endpoint = "20/s:100"
api_ip = "20/s:40"
create_ip = "10/m:10"

[forward]
enabled = true
concurrency = 32
max_attempts = 3
allow_cidrs = []

[shutdown]
drain = "5s"
grace = "25s"
"#;

const ENV_KEYS: &[(&str, &str)] = &[
    ("PUBLIC_URL", "public_url"),
    ("DATABASE_URL", "database.url"),
    ("DB_POOL_SIZE", "database.pool_size"),
    ("LISTEN_ADDR", "listen_addr"),
    ("ADMIN_ADDR", "admin_addr"),
    ("AUTH_MODE", "auth.mode"),
    ("OIDC_ISSUER", "auth.oidc.issuer"),
    ("OIDC_CLIENT_ID", "auth.oidc.client_id"),
    ("OIDC_CLIENT_SECRET", "auth.oidc.client_secret"),
    ("OIDC_SCOPES", "auth.oidc.scopes"),
    ("OIDC_PROVIDER_NAME", "auth.oidc.provider_name"),
    ("SESSION_IDLE", "auth.session_idle"),
    ("SESSION_MAX", "auth.session_max"),
    ("AUTH_MAX_PENDING_LOGINS", "auth.max_pending_logins"),
    ("TRUSTED_PROXIES", "trusted_proxies"),
    ("RETENTION", "capture.retention"),
    ("MAX_CAPTURES", "capture.max_captures"),
    ("MAX_BODY_BYTES", "capture.max_body_bytes"),
    ("BODY_MEMORY_BUDGET", "capture.body_memory_budget"),
    ("MAX_DELAYED", "capture.max_delayed"),
    ("RING_MODE_ALLOWED", "capture.ring_mode_allowed"),
    ("STORAGE_CAP_BYTES", "capture.storage_cap_bytes"),
    ("RATE_CAPTURE_IP", "rate.capture_ip"),
    ("RATE_CAPTURE_ENDPOINT", "rate.capture_endpoint"),
    ("RATE_API_IP", "rate.api_ip"),
    ("RATE_CREATE_IP", "rate.create_ip"),
    ("FORWARD_ENABLED", "forward.enabled"),
    ("FORWARD_CONCURRENCY", "forward.concurrency"),
    ("FORWARD_MAX_ATTEMPTS", "forward.max_attempts"),
    ("FORWARD_ALLOW_CIDRS", "forward.allow_cidrs"),
    ("SHUTDOWN_DRAIN", "shutdown.drain"),
    ("SHUTDOWN_GRACE", "shutdown.grace"),
    ("LOG", "log"),
];

#[derive(Debug, Deserialize)]
pub struct Config {
    pub public_url: Url,
    pub listen_addr: SocketAddr,
    pub admin_addr: SocketAddr,
    pub log: String,
    #[serde(deserialize_with = "de::list")]
    pub trusted_proxies: Vec<IpNet>,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub capture: CaptureConfig,
    pub rate: RateConfig,
    pub forward: ForwardConfig,
    pub shutdown: ShutdownConfig,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: DatabaseUrl,
    pub pool_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub oidc: OidcConfig,
    #[serde(deserialize_with = "de::duration")]
    pub session_idle: Duration,
    #[serde(deserialize_with = "de::duration")]
    pub session_max: Duration,
    pub max_pending_logins: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    Open,
    Sso,
}

#[derive(Debug, Deserialize)]
pub struct OidcConfig {
    pub issuer: Option<Url>,
    pub client_id: Option<String>,
    pub client_secret: Option<ClientSecret>,
    pub provider_name: Option<String>,
    #[serde(deserialize_with = "de::list")]
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CaptureConfig {
    #[serde(deserialize_with = "de::duration")]
    pub retention: Duration,
    pub max_captures: u32,
    #[serde(deserialize_with = "de::byte_size")]
    pub max_body_bytes: ByteSize,
    #[serde(deserialize_with = "de::byte_size")]
    pub body_memory_budget: ByteSize,
    pub max_delayed: u32,
    pub ring_mode_allowed: bool,
    #[serde(deserialize_with = "de::byte_size")]
    pub storage_cap_bytes: ByteSize,
}

#[derive(Debug, Deserialize)]
pub struct RateConfig {
    pub capture_ip: RateSpec,
    pub capture_endpoint: RateSpec,
    pub api_ip: RateSpec,
    pub create_ip: RateSpec,
}

#[derive(Debug, Deserialize)]
pub struct ForwardConfig {
    pub enabled: bool,
    pub concurrency: u32,
    pub max_attempts: u32,
    #[serde(deserialize_with = "de::list")]
    pub allow_cidrs: Vec<IpNet>,
}

#[derive(Debug, Deserialize)]
pub struct ShutdownConfig {
    #[serde(deserialize_with = "de::duration")]
    pub drain: Duration,
    #[serde(deserialize_with = "de::duration")]
    pub grace: Duration,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct DatabaseUrl(Url);

impl DatabaseUrl {
    pub fn unpack(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseUrl(<redacted>)")
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct ClientSecret(String);

impl ClientSecret {
    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientSecret(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    MissingRequired(&'static str),
    InvalidRequired(&'static str),
    MissingForSso(&'static str),
    PoolSizeTooSmall,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequired(key) => write!(f, "{ENV_PREFIX}{key} is required"),
            Self::InvalidRequired(key) => write!(f, "{ENV_PREFIX}{key} is not a valid URL"),
            Self::MissingForSso(key) => write!(
                f,
                "{ENV_PREFIX}{key} is required when {ENV_PREFIX}AUTH_MODE is sso"
            ),
            Self::PoolSizeTooSmall => write!(
                f,
                "{ENV_PREFIX}DB_POOL_SIZE must be at least {MIN_DB_POOL_SIZE}"
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config file {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration: {0}")]
    Load(Box<figment::Error>),
    #[error("invalid configuration: {}", join_problems(.problems))]
    Invalid { problems: Vec<Problem> },
}

fn check_required<T: DeserializeOwned>(
    figment: &Figment,
    key: &'static str,
    path: &str,
    problems: &mut Vec<Problem>,
) {
    if !figment.contains(path) {
        problems.push(Problem::MissingRequired(key));
    } else if figment.extract_inner::<T>(path).is_err() {
        problems.push(Problem::InvalidRequired(key));
    }
}

fn figment_problems(figment: &Figment) -> Vec<Problem> {
    let mut problems = Vec::new();
    check_required::<Url>(figment, "PUBLIC_URL", "public_url", &mut problems);
    check_required::<DatabaseUrl>(figment, "DATABASE_URL", "database.url", &mut problems);

    let sso = figment
        .extract_inner::<AuthMode>("auth.mode")
        .is_ok_and(|mode| mode == AuthMode::Sso);
    if sso {
        problems.extend(sso_problems(
            figment.contains("auth.oidc.issuer"),
            figment.contains("auth.oidc.client_id"),
            figment.contains("auth.oidc.client_secret"),
        ));
    }

    if let Ok(pool_size) = figment.extract_inner::<u32>("database.pool_size") {
        problems.extend(pool_size_problem(pool_size));
    }
    problems
}

fn sso_problems(issuer: bool, client_id: bool, client_secret: bool) -> Vec<Problem> {
    [
        (issuer, "OIDC_ISSUER"),
        (client_id, "OIDC_CLIENT_ID"),
        (client_secret, "OIDC_CLIENT_SECRET"),
    ]
    .into_iter()
    .filter(|(present, _)| !present)
    .map(|(_, key)| Problem::MissingForSso(key))
    .collect()
}

fn pool_size_problem(pool_size: u32) -> Option<Problem> {
    (pool_size < MIN_DB_POOL_SIZE).then_some(Problem::PoolSizeTooSmall)
}

fn join_problems(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let mut figment = Figment::from(Toml::string(DEFAULTS));

        if let Some(path) = config_file_path() {
            let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::ReadFile {
                path: path.clone(),
                source,
            })?;
            figment = figment.merge(Toml::string(&text));
        }

        let env = Env::prefixed(ENV_PREFIX).map(|key| {
            ENV_KEYS
                .iter()
                .find(|(name, _)| *name == key.as_str())
                .map_or_else(
                    || key.as_str().to_ascii_lowercase(),
                    |(_, path)| (*path).to_owned(),
                )
                .into()
        });
        figment = figment.merge(env);

        let problems = figment_problems(&figment);
        if !problems.is_empty() {
            return Err(ConfigError::Invalid { problems });
        }

        let config: Self = figment
            .extract()
            .map_err(|error| ConfigError::Load(Box::new(error)))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut problems = Vec::new();

        if self.auth.mode == AuthMode::Sso {
            let oidc = &self.auth.oidc;
            problems.extend(sso_problems(
                oidc.issuer.is_some(),
                oidc.client_id.is_some(),
                oidc.client_secret.is_some(),
            ));
        }
        problems.extend(pool_size_problem(self.database.pool_size));

        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid { problems })
        }
    }
}

fn config_file_path() -> Option<PathBuf> {
    std::env::var_os(FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
