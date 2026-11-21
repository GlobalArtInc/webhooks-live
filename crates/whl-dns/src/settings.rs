use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hickory_proto::rr::Name;
use whl_core::client_ip::TrustedProxies;
use whl_core::config::Config;
use whl_core::endpoint_id::ENDPOINT_ID_LEN;
use whl_core::rate::{RatePeriod, RateSpec};
use whl_store::dns::DnsCapturePolicy;

use crate::error::DnsError;

const MAX_NAME_TEXT_BYTES: usize = 253;
const MAX_LABEL_BYTES: usize = 63;
const MAX_ZONE_TEXT_BYTES: usize = MAX_NAME_TEXT_BYTES - ENDPOINT_ID_LEN - 1;

pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(10);
pub const DEFAULT_STALE_TTL: Duration = Duration::from_secs(300);
pub const DEFAULT_TCP_PER_ADDRESS: usize = 8;
pub const DEFAULT_CACHE_ENTRIES: usize = 50_000;
pub const DEFAULT_MAX_CONCURRENT_QUERIES: usize = 256;
pub const DEFAULT_MAX_CONCURRENT_STORES: usize = 8;
pub const DEFAULT_MAX_CONNECTIONS: usize = 256;
pub const DEFAULT_TCP_READ_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_TCP_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_TCP_LIFETIME: Duration = Duration::from_secs(30);
pub const DEFAULT_TCP_MAX_QUERIES: usize = 16;
const DEFAULT_MAX_CAPTURES: u32 = 500;
const DEFAULT_RETENTION: Duration = Duration::from_secs(7 * 86_400);
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsLimits {
    pub cache_ttl: Duration,
    pub stale_ttl: Duration,
    pub cache_entries: usize,
    pub lookup_rate: RateSpec,
    pub max_concurrent_queries: usize,
    pub max_concurrent_stores: usize,
    pub max_connections: usize,
    pub tcp_per_address: usize,
    pub tcp_read_timeout: Duration,
    pub tcp_write_timeout: Duration,
    pub tcp_lifetime: Duration,
    pub tcp_max_queries: usize,
}

impl Default for DnsLimits {
    fn default() -> Self {
        Self {
            cache_ttl: DEFAULT_CACHE_TTL,
            stale_ttl: DEFAULT_STALE_TTL,
            cache_entries: DEFAULT_CACHE_ENTRIES,
            lookup_rate: RateSpec {
                count: 200,
                period: RatePeriod::Second,
                burst: 200,
            },
            max_concurrent_queries: DEFAULT_MAX_CONCURRENT_QUERIES,
            max_concurrent_stores: DEFAULT_MAX_CONCURRENT_STORES,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            tcp_per_address: DEFAULT_TCP_PER_ADDRESS,
            tcp_read_timeout: DEFAULT_TCP_READ_TIMEOUT,
            tcp_write_timeout: DEFAULT_TCP_WRITE_TIMEOUT,
            tcp_lifetime: DEFAULT_TCP_LIFETIME,
            tcp_max_queries: DEFAULT_TCP_MAX_QUERIES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DnsSettings {
    pub listen_addr: SocketAddr,
    pub zone: Name,
    pub ns_name: Name,
    pub soa_rname: Name,
    pub answer_a: Option<Ipv4Addr>,
    pub answer_aaaa: Option<Ipv6Addr>,
    pub trusted_proxies: TrustedProxies,
    pub rate_source: RateSpec,
    pub rate_global: RateSpec,
    pub rate_capture_endpoint: RateSpec,
    pub policy: DnsCapturePolicy,
    pub serial: u32,
    pub limits: DnsLimits,
}

impl DnsSettings {
    pub fn new(
        listen_addr: SocketAddr,
        zone: &str,
        ns_name: &str,
        soa_email: &str,
    ) -> Result<Self, DnsError> {
        Ok(Self {
            listen_addr,
            zone: parse_zone(zone)?,
            ns_name: parse_host(ns_name).map_err(|reason| DnsError::NameServer {
                name: ns_name.to_owned(),
                reason,
            })?,
            soa_rname: soa_rname(soa_email)?,
            answer_a: None,
            answer_aaaa: None,
            trusted_proxies: TrustedProxies::default(),
            rate_source: RateSpec {
                count: 20,
                period: RatePeriod::Second,
                burst: 20,
            },
            rate_global: RateSpec {
                count: 2000,
                period: RatePeriod::Second,
                burst: 2000,
            },
            rate_capture_endpoint: RateSpec {
                count: 60,
                period: RatePeriod::Minute,
                burst: 60,
            },
            policy: DnsCapturePolicy {
                max_captures: DEFAULT_MAX_CAPTURES,
                retention: DEFAULT_RETENTION,
            },
            serial: serial_from_clock(),
            limits: DnsLimits::default(),
        })
    }

    pub fn from_config(config: &Config) -> Result<Option<Self>, DnsError> {
        let dns = &config.dns;
        let (Some(listen_addr), Some(zone), Some(ns_name), Some(email)) = (
            dns.listen_addr,
            dns.zone.as_deref(),
            dns.ns_name.as_deref(),
            dns.soa_email.as_deref(),
        ) else {
            return Ok(None);
        };
        let mut settings = Self::new(listen_addr, zone, ns_name, email)?;
        settings.answer_a = dns.answer_a;
        settings.answer_aaaa = dns.answer_aaaa;
        settings.trusted_proxies = TrustedProxies::new(dns.trusted_proxies.clone());
        settings.rate_source = dns.rate_source;
        settings.rate_global = dns.rate_global;
        settings.rate_capture_endpoint = dns.rate_capture_endpoint;
        settings.policy = DnsCapturePolicy {
            max_captures: config.capture.max_captures,
            retention: config.capture.retention,
        };
        Ok(Some(settings))
    }

    pub fn zone_text(&self) -> String {
        let text = self.zone.to_ascii();
        text.strip_suffix('.').unwrap_or(&text).to_owned()
    }
}

fn serial_from_clock() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u32::try_from(elapsed.as_secs()).ok())
        .unwrap_or(1)
}

fn parse_zone(text: &str) -> Result<Name, DnsError> {
    let invalid = |reason| DnsError::Zone {
        name: text.to_owned(),
        reason,
    };
    if text.len() > MAX_ZONE_TEXT_BYTES {
        return Err(invalid("it is too long to hold an endpoint label below it"));
    }
    parse_host(text).map_err(invalid)
}

fn parse_host(text: &str) -> Result<Name, &'static str> {
    let trimmed = text.strip_suffix('.').unwrap_or(text);
    if trimmed.is_empty() {
        return Err("it is empty");
    }
    if trimmed.len() > MAX_NAME_TEXT_BYTES {
        return Err("it is longer than 253 bytes");
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > MAX_LABEL_BYTES {
            return Err("a label is empty or longer than 63 bytes");
        }
        if !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return Err("a label has a character outside letters, digits and hyphen");
        }
    }
    let mut name =
        Name::from_ascii(trimmed.to_ascii_lowercase()).map_err(|_| "it cannot be encoded")?;
    name.set_fqdn(true);
    Ok(name)
}

fn soa_rname(email: &str) -> Result<Name, DnsError> {
    let invalid = || DnsError::SoaEmail {
        email: email.to_owned(),
    };
    let (local, domain) = email.rsplit_once('@').ok_or_else(invalid)?;
    if local.is_empty() || local.len() > MAX_LABEL_BYTES || !local.is_ascii() {
        return Err(invalid());
    }
    let domain = parse_host(domain).map_err(|_| invalid())?;
    let mut labels: Vec<Vec<u8>> = vec![local.to_ascii_lowercase().into_bytes()];
    labels.extend(domain.iter().map(<[u8]>::to_vec));
    let mut name = Name::from_labels(labels).map_err(|_| invalid())?;
    name.set_fqdn(true);
    Ok(name)
}
