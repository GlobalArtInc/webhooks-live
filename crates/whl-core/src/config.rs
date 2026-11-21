mod de;

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use bytesize::ByteSize;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use ipnet::IpNet;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;
use zeroize::Zeroizing;

use crate::rate::RateSpec;

const FILE_ENV: &str = "WEBHOOKS_LIVE_CONFIG";
const ENV_PREFIX: &str = "WEBHOOKS_LIVE_";

pub const MIN_DB_POOL_SIZE: u32 = 2;
pub const MIN_SCHEDULE_INTERVAL: Duration = Duration::from_secs(60);
pub const MAX_RULES_SYNC_BUDGET: Duration = Duration::from_secs(10);
pub const MAX_SCRIPT_THREADS: u32 = 64;
pub const MIN_RULES_CACHE: ByteSize = ByteSize::mib(16);
pub const MIN_SCRIPT_MEMORY: ByteSize = ByteSize::mib(1);
pub const MAX_SCRIPT_MEMORY: ByteSize = ByteSize::gib(1);

const DEFAULTS: &str = r#"
listen_addr = "0.0.0.0:8080"
admin_addr = "0.0.0.0:9090"
log = "info"
trusted_proxies = []
api_cors = true

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

[dns]
trusted_proxies = []
rate_source = "20/s:20"
rate_global = "2000/s:2000"
rate_capture_endpoint = "60/m:60"

[export]
concurrency = 2
[schedules]
enabled = true
max_per_user = 5
min_interval = "5m"
runs_per_hour = 120
workers = 8

[composer]
enabled = true
endpoint_per_hour = 200
network_per_day = 1000
user_per_hour = 2000
max_in_flight = 32

[rules]
enabled = true
sync_budget = "3s"
sync_concurrency = 256
cache_bytes = "128MiB"

[script]
threads = 2
memory = "32MiB"

[notify]
enabled = true
concurrency = 8
max_attempts = 3
endpoint_per_hour = 30
owner_per_hour = 300
network_per_hour = 60
global_per_hour = 600

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
    ("API_CORS", "api_cors"),
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
    ("EXPORT_CONCURRENCY", "export.concurrency"),
    ("COMPOSER_ENABLED", "composer.enabled"),
    ("COMPOSER_ENDPOINT_PER_HOUR", "composer.endpoint_per_hour"),
    ("COMPOSER_NETWORK_PER_DAY", "composer.network_per_day"),
    ("COMPOSER_USER_PER_HOUR", "composer.user_per_hour"),
    ("COMPOSER_MAX_IN_FLIGHT", "composer.max_in_flight"),
    ("SCHEDULES_ENABLED", "schedules.enabled"),
    ("SCHEDULES_MAX_PER_USER", "schedules.max_per_user"),
    ("SCHEDULES_MIN_INTERVAL", "schedules.min_interval"),
    ("SCHEDULES_RUNS_PER_HOUR", "schedules.runs_per_hour"),
    ("SCHEDULES_WORKERS", "schedules.workers"),
    ("RULES_ENABLED", "rules.enabled"),
    ("RULES_SYNC_BUDGET", "rules.sync_budget"),
    ("RULES_SYNC_CONCURRENCY", "rules.sync_concurrency"),
    ("RULES_CACHE_BYTES", "rules.cache_bytes"),
    ("SCRIPT_THREADS", "script.threads"),
    ("SCRIPT_MEMORY", "script.memory"),
    ("SHUTDOWN_DRAIN", "shutdown.drain"),
    ("SHUTDOWN_GRACE", "shutdown.grace"),
    ("NOTIFY_ENABLED", "notify.enabled"),
    ("NOTIFY_CONCURRENCY", "notify.concurrency"),
    ("NOTIFY_MAX_ATTEMPTS", "notify.max_attempts"),
    ("NOTIFY_ENDPOINT_PER_HOUR", "notify.endpoint_per_hour"),
    ("NOTIFY_OWNER_PER_HOUR", "notify.owner_per_hour"),
    ("NOTIFY_NETWORK_PER_HOUR", "notify.network_per_hour"),
    ("NOTIFY_GLOBAL_PER_HOUR", "notify.global_per_hour"),
    ("VENDOR_PROXY", "notify.vendor_proxy"),
    ("SECRETS_KEYS", "secrets.keys"),
    ("DNS_LISTEN_ADDR", "dns.listen_addr"),
    ("DNS_ZONE", "dns.zone"),
    ("DNS_TRUSTED_PROXIES", "dns.trusted_proxies"),
    ("DNS_ANSWER_A", "dns.answer_a"),
    ("DNS_ANSWER_AAAA", "dns.answer_aaaa"),
    ("DNS_NS_NAME", "dns.ns_name"),
    ("DNS_SOA_EMAIL", "dns.soa_email"),
    ("DNS_RATE_SOURCE", "dns.rate_source"),
    ("DNS_RATE_GLOBAL", "dns.rate_global"),
    ("DNS_RATE_CAPTURE_ENDPOINT", "dns.rate_capture_endpoint"),
    ("MAIL_LISTEN_ADDR", "mail.listen_addr"),
    ("MAIL_DOMAIN", "mail.domain"),
    ("MAIL_TRUSTED_PROXIES", "mail.trusted_proxies"),
    ("MAIL_TLS_CERT", "mail.tls_cert"),
    ("MAIL_TLS_KEY", "mail.tls_key"),
    ("LOG", "log"),
];

#[derive(Debug, Deserialize)]
pub struct Config {
    pub public_url: Url,
    pub listen_addr: SocketAddr,
    pub admin_addr: SocketAddr,
    pub log: String,
    pub api_cors: bool,
    #[serde(deserialize_with = "de::list")]
    pub trusted_proxies: Vec<IpNet>,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub capture: CaptureConfig,
    pub rate: RateConfig,
    pub forward: ForwardConfig,
    pub export: ExportConfig,
    pub composer: ComposerConfig,
    pub schedules: SchedulesConfig,
    pub notify: NotifyConfig,
    pub rules: RulesConfig,
    pub script: ScriptConfig,
    #[serde(default)]
    pub secrets: SecretsConfig,
    pub dns: DnsConfig,
    #[serde(default)]
    pub mail: MailConfig,
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
    Mixed,
}

impl AuthMode {
    pub fn needs_oidc(self) -> bool {
        matches!(self, Self::Sso | Self::Mixed)
    }

    pub fn needs_secrets(self) -> bool {
        matches!(self, Self::Sso | Self::Mixed)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Sso => "sso",
            Self::Mixed => "mixed",
        }
    }
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
pub struct ExportConfig {
    pub concurrency: u32,
}

#[derive(Debug, Deserialize)]
pub struct ComposerConfig {
    pub enabled: bool,
    pub endpoint_per_hour: u32,
    pub network_per_day: u32,
    pub user_per_hour: u32,
    pub max_in_flight: u32,
}

#[derive(Debug, Deserialize)]
pub struct SchedulesConfig {
    pub enabled: bool,
    pub max_per_user: u32,
    #[serde(deserialize_with = "de::duration")]
    pub min_interval: Duration,
    pub runs_per_hour: u32,
    pub workers: u32,
}

#[derive(Debug, Deserialize)]
pub struct RulesConfig {
    pub enabled: bool,
    #[serde(deserialize_with = "de::duration")]
    pub sync_budget: Duration,
    pub sync_concurrency: u32,
    #[serde(deserialize_with = "de::byte_size")]
    pub cache_bytes: ByteSize,
}

#[derive(Debug, Deserialize)]
pub struct ScriptConfig {
    pub threads: u32,
    #[serde(deserialize_with = "de::byte_size")]
    pub memory: ByteSize,
}

#[derive(Debug, Deserialize)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub concurrency: u32,
    pub max_attempts: u32,
    pub endpoint_per_hour: u32,
    pub owner_per_hour: u32,
    pub network_per_hour: u32,
    pub global_per_hour: u32,
    pub vendor_proxy: Option<ProxyUrl>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MailConfig {
    pub listen_addr: Option<SocketAddr>,
    pub domain: Option<String>,
    #[serde(default, deserialize_with = "de::list")]
    pub trusted_proxies: Vec<IpNet>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

pub const MAX_MAIL_DOMAIN_LEN: usize = 253;
const MAX_DOMAIN_LABEL_LEN: usize = 63;

pub fn is_valid_mail_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= MAX_MAIL_DOMAIN_LEN
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= MAX_DOMAIN_LABEL_LEN
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[derive(Debug, Default, Deserialize)]
pub struct SecretsConfig {
    pub keys: Option<SecretKeys>,
}

impl SecretsConfig {
    pub fn keys(&self) -> Option<&SecretKeys> {
        self.keys.as_ref().filter(|keys| !keys.is_blank())
    }
}

#[derive(Debug, Deserialize)]
pub struct DnsConfig {
    #[serde(default, deserialize_with = "de::optional")]
    pub listen_addr: Option<SocketAddr>,
    #[serde(default, deserialize_with = "de::optional")]
    pub zone: Option<String>,
    #[serde(deserialize_with = "de::list")]
    pub trusted_proxies: Vec<IpNet>,
    #[serde(default, deserialize_with = "de::optional")]
    pub answer_a: Option<Ipv4Addr>,
    #[serde(default, deserialize_with = "de::optional")]
    pub answer_aaaa: Option<Ipv6Addr>,
    #[serde(default, deserialize_with = "de::optional")]
    pub ns_name: Option<String>,
    #[serde(default, deserialize_with = "de::optional")]
    pub soa_email: Option<String>,
    pub rate_source: RateSpec,
    pub rate_global: RateSpec,
    pub rate_capture_endpoint: RateSpec,
}

impl DnsConfig {
    pub fn enabled(&self) -> bool {
        self.listen_addr.is_some()
    }

    fn problems(&self) -> Vec<Problem> {
        if self.listen_addr.is_none() && self.zone.is_none() {
            return Vec::new();
        }
        [
            (self.listen_addr.is_some(), "DNS_LISTEN_ADDR"),
            (self.zone.is_some(), "DNS_ZONE"),
            (self.ns_name.is_some(), "DNS_NS_NAME"),
            (self.soa_email.is_some(), "DNS_SOA_EMAIL"),
        ]
        .into_iter()
        .filter(|(present, _)| !present)
        .map(|(_, key)| Problem::MissingForDns(key))
        .collect()
    }
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

#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct ProxyUrl(Url);

impl ProxyUrl {
    pub fn unpack(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProxyUrl(<redacted>)")
    }
}

#[derive(Deserialize)]
#[serde(from = "String")]
pub struct SecretKeys(Zeroizing<String>);

impl SecretKeys {
    pub fn unpack(&self) -> &str {
        &self.0
    }

    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl From<String> for SecretKeys {
    fn from(text: String) -> Self {
        Self(Zeroizing::new(text))
    }
}

impl fmt::Debug for SecretKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKeys(<redacted>)")
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
    MissingForMixed(&'static str),
    MissingForDns(&'static str),
    PoolSizeTooSmall,
    ScheduleIntervalTooShort,
    ScheduleRunsPerHourZero,
    MailNeedsDomain,
    MailInvalidDomain,
    MailTlsIncomplete,
    RulesSyncBudgetOutOfRange,
    RulesSyncConcurrencyZero,
    RulesCacheTooSmall,
    ScriptThreadsOutOfRange,
    ScriptMemoryOutOfRange,
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
            Self::MissingForMixed(key) => write!(
                f,
                "{ENV_PREFIX}{key} is required when {ENV_PREFIX}AUTH_MODE is mixed"
            ),
            Self::MissingForDns(key) => write!(
                f,
                "{ENV_PREFIX}{key} is required when DNS capture is configured"
            ),
            Self::ScheduleIntervalTooShort => write!(
                f,
                "{ENV_PREFIX}SCHEDULES_MIN_INTERVAL must be at least {}s",
                MIN_SCHEDULE_INTERVAL.as_secs()
            ),
            Self::ScheduleRunsPerHourZero => {
                write!(f, "{ENV_PREFIX}SCHEDULES_RUNS_PER_HOUR must be at least 1")
            }
            Self::PoolSizeTooSmall => write!(
                f,
                "{ENV_PREFIX}DB_POOL_SIZE must be at least {MIN_DB_POOL_SIZE}"
            ),
            Self::MailNeedsDomain => write!(
                f,
                "{ENV_PREFIX}MAIL_DOMAIN is required when {ENV_PREFIX}MAIL_LISTEN_ADDR is set"
            ),
            Self::MailInvalidDomain => {
                write!(f, "{ENV_PREFIX}MAIL_DOMAIN is not a valid domain name")
            }
            Self::MailTlsIncomplete => write!(
                f,
                "{ENV_PREFIX}MAIL_TLS_CERT and {ENV_PREFIX}MAIL_TLS_KEY must be set together"
            ),
            Self::RulesSyncBudgetOutOfRange => write!(
                f,
                "{ENV_PREFIX}RULES_SYNC_BUDGET must be above zero and at most {}s",
                MAX_RULES_SYNC_BUDGET.as_secs()
            ),
            Self::RulesSyncConcurrencyZero => {
                write!(f, "{ENV_PREFIX}RULES_SYNC_CONCURRENCY must be at least 1")
            }
            Self::RulesCacheTooSmall => write!(
                f,
                "{ENV_PREFIX}RULES_CACHE_BYTES must be at least {MIN_RULES_CACHE}"
            ),
            Self::ScriptThreadsOutOfRange => write!(
                f,
                "{ENV_PREFIX}SCRIPT_THREADS must be between 1 and {MAX_SCRIPT_THREADS}"
            ),
            Self::ScriptMemoryOutOfRange => write!(
                f,
                "{ENV_PREFIX}SCRIPT_MEMORY must be between {MIN_SCRIPT_MEMORY} and {MAX_SCRIPT_MEMORY}"
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

    if let Ok(mode) = figment.extract_inner::<AuthMode>("auth.mode") {
        problems.extend(oidc_problems(
            mode,
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

fn oidc_problems(
    mode: AuthMode,
    issuer: bool,
    client_id: bool,
    client_secret: bool,
) -> Vec<Problem> {
    let missing = match mode {
        AuthMode::Open => return Vec::new(),
        AuthMode::Sso => Problem::MissingForSso,
        AuthMode::Mixed => Problem::MissingForMixed,
    };
    [
        (issuer, "OIDC_ISSUER"),
        (client_id, "OIDC_CLIENT_ID"),
        (client_secret, "OIDC_CLIENT_SECRET"),
    ]
    .into_iter()
    .filter(|(present, _)| !present)
    .map(|(_, key)| missing(key))
    .collect()
}

fn pool_size_problem(pool_size: u32) -> Option<Problem> {
    (pool_size < MIN_DB_POOL_SIZE).then_some(Problem::PoolSizeTooSmall)
}

fn mail_problems(mail: &MailConfig) -> Vec<Problem> {
    let mut problems = Vec::new();
    match (&mail.domain, mail.listen_addr) {
        (None, Some(_)) => problems.push(Problem::MailNeedsDomain),
        (Some(domain), _) if !is_valid_mail_domain(domain) => {
            problems.push(Problem::MailInvalidDomain);
        }
        _ => {}
    }
    if mail.tls_cert.is_some() != mail.tls_key.is_some() {
        problems.push(Problem::MailTlsIncomplete);
    }
    problems
}

fn rules_problems(rules: &RulesConfig, script: &ScriptConfig) -> Vec<Problem> {
    let mut problems = Vec::new();
    if rules.sync_budget.is_zero() || rules.sync_budget > MAX_RULES_SYNC_BUDGET {
        problems.push(Problem::RulesSyncBudgetOutOfRange);
    }
    if rules.sync_concurrency == 0 {
        problems.push(Problem::RulesSyncConcurrencyZero);
    }
    if rules.cache_bytes < MIN_RULES_CACHE {
        problems.push(Problem::RulesCacheTooSmall);
    }
    if script.threads == 0 || script.threads > MAX_SCRIPT_THREADS {
        problems.push(Problem::ScriptThreadsOutOfRange);
    }
    if script.memory < MIN_SCRIPT_MEMORY || script.memory > MAX_SCRIPT_MEMORY {
        problems.push(Problem::ScriptMemoryOutOfRange);
    }
    problems
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

        let oidc = &self.auth.oidc;
        problems.extend(oidc_problems(
            self.auth.mode,
            oidc.issuer.is_some(),
            oidc.client_id.is_some(),
            oidc.client_secret.is_some(),
        ));
        problems.extend(pool_size_problem(self.database.pool_size));
        problems.extend(self.dns.problems());
        if self.schedules.min_interval < MIN_SCHEDULE_INTERVAL {
            problems.push(Problem::ScheduleIntervalTooShort);
        }
        if self.schedules.runs_per_hour == 0 {
            problems.push(Problem::ScheduleRunsPerHourZero);
        }
        problems.extend(mail_problems(&self.mail));
        problems.extend(rules_problems(&self.rules, &self.script));

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
