use std::net::IpAddr;
use std::sync::Arc;

use serde_json::{Value, json};
use time::OffsetDateTime;
use url::{Host, Url};
use whl_store::notify::MAX_PUSH_ENDPOINT_BYTES;

use crate::adapter::{AdapterInput, Destination, PrepareError, Prepared};
use crate::notice::{Notice, truncate};
use crate::push_encrypt::{SubscriptionKeys, encrypt};
use crate::vapid::Vapid;

pub const MAX_PAYLOAD_BYTES: usize = 3072;
pub const PUSH_TTL_SECONDS: u32 = 3600;
pub const MAX_EXTRA_PUSH_HOSTS: usize = 32;

const MAX_PAYLOAD_PATH_CHARS: usize = 512;
const MAX_PAYLOAD_METHOD_CHARS: usize = 16;
const MAX_HOST_BYTES: usize = 253;
const MAX_LABEL_BYTES: usize = 63;
const MIN_PATTERN_LABELS: usize = 2;
const WILDCARD_PREFIX: &str = "*.";
const VENDOR_EXACT: [&str; 2] = ["fcm.googleapis.com", "updates.push.services.mozilla.com"];
const VENDOR_SUFFIXES: [&str; 2] = ["push.apple.com", "notify.windows.com"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostPatternError {
    #[error("the push host pattern {0:?} is not a DNS name or a *.suffix pattern")]
    Invalid(String),
    #[error("at most {MAX_EXTRA_PUSH_HOSTS} extra push hosts are allowed")]
    TooMany,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PushUrlRefusal {
    #[error("the endpoint is longer than {MAX_PUSH_ENDPOINT_BYTES} bytes")]
    TooLong,
    #[error("the endpoint is not a valid URL")]
    Invalid,
    #[error(
        "the endpoint must be an https URL with a host name, the default port, no credentials and no fragment"
    )]
    Shape,
    #[error("the host {0:?} is not an allowed push service")]
    Host(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushHosts {
    exact: Vec<String>,
    suffixes: Vec<String>,
}

impl Default for PushHosts {
    fn default() -> Self {
        Self::vendors()
    }
}

impl PushHosts {
    pub fn vendors() -> Self {
        Self {
            exact: VENDOR_EXACT.iter().map(|host| (*host).to_owned()).collect(),
            suffixes: VENDOR_SUFFIXES
                .iter()
                .map(|suffix| (*suffix).to_owned())
                .collect(),
        }
    }

    pub fn with_extras(mut self, extras: &[String]) -> Result<Self, HostPatternError> {
        if extras.len() > MAX_EXTRA_PUSH_HOSTS {
            return Err(HostPatternError::TooMany);
        }
        for pattern in extras {
            let lowered = pattern.trim().to_ascii_lowercase();
            match lowered.strip_prefix(WILDCARD_PREFIX) {
                Some(base) if valid_name(base) => self.suffixes.push(base.to_owned()),
                None if valid_name(&lowered) => self.exact.push(lowered),
                _ => return Err(HostPatternError::Invalid(pattern.clone())),
            }
        }
        Ok(self)
    }

    pub fn allows(&self, host: &str) -> bool {
        if host.len() > MAX_HOST_BYTES {
            return false;
        }
        self.exact.iter().any(|exact| exact == host)
            || self.suffixes.iter().any(|suffix| {
                host.strip_suffix(suffix.as_str())
                    .and_then(|prefix| prefix.strip_suffix('.'))
                    .is_some_and(|prefix| prefix.split('.').all(valid_label))
            })
    }

    pub fn check_url(&self, url: &str) -> Result<Url, PushUrlRefusal> {
        if url.len() > MAX_PUSH_ENDPOINT_BYTES {
            return Err(PushUrlRefusal::TooLong);
        }
        if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(PushUrlRefusal::Invalid);
        }
        let parsed = Url::parse(url).map_err(|_| PushUrlRefusal::Invalid)?;
        if parsed.as_str().len() > MAX_PUSH_ENDPOINT_BYTES {
            return Err(PushUrlRefusal::TooLong);
        }
        if parsed.scheme() != "https"
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(PushUrlRefusal::Shape);
        }
        let Some(Host::Domain(host)) = parsed.host() else {
            return Err(PushUrlRefusal::Shape);
        };
        if self.allows(host) {
            Ok(parsed)
        } else {
            Err(PushUrlRefusal::Host(host.to_owned()))
        }
    }
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_name(name: &str) -> bool {
    name.len() <= MAX_HOST_BYTES
        && name.split('.').count() >= MIN_PATTERN_LABELS
        && name.split('.').all(valid_label)
        && name.parse::<IpAddr>().is_err()
        && name
            .rsplit('.')
            .next()
            .is_some_and(|last| !last.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, Clone)]
pub struct PushService {
    vapid: Arc<Vapid>,
    hosts: PushHosts,
}

impl PushService {
    pub fn new(vapid: Vapid, hosts: PushHosts) -> Self {
        Self {
            vapid: Arc::new(vapid),
            hosts,
        }
    }

    pub fn public_key(&self) -> &str {
        self.vapid.public_key()
    }

    pub fn hosts(&self) -> &PushHosts {
        &self.hosts
    }

    pub(crate) fn vapid(&self) -> &Vapid {
        &self.vapid
    }
}

pub struct PushTarget<'a> {
    pub endpoint: &'a str,
    pub p256dh: &'a [u8],
    pub auth: &'a [u8],
    pub vapid: &'a Vapid,
}

pub(crate) fn payload(notice: &Notice) -> Result<Vec<u8>, PrepareError> {
    let capture = notice.capture.as_ref();
    let text = |name: &str| {
        capture
            .and_then(|capture| capture.get(name))
            .and_then(Value::as_str)
    };
    let capture_id = text("id").map(str::to_owned);
    let kind = text("kind")
        .unwrap_or(notice.delivery_kind.as_str())
        .to_owned();
    let method = text("method").map(|method| truncate(method, MAX_PAYLOAD_METHOD_CHARS));
    let mut path = text("path").map(|path| truncate(path, MAX_PAYLOAD_PATH_CHARS));
    loop {
        let body = serde_json::to_vec(&json!({
            "endpoint_id": notice.endpoint_id,
            "capture_id": capture_id,
            "kind": kind,
            "method": method,
            "path": path,
        }))
        .map_err(|_| PrepareError::Encode)?;
        if body.len() <= MAX_PAYLOAD_BYTES {
            return Ok(body);
        }
        match path.as_mut() {
            Some(path) if !path.is_empty() => {
                let keep = path.chars().count() / 2;
                *path = path.chars().take(keep).collect();
            }
            _ => return Err(PrepareError::Encode),
        }
    }
}

pub(crate) fn prepare(input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
    let target = input.push.ok_or(PrepareError::Config)?;
    let endpoint = Url::parse(target.endpoint).map_err(|_| PrepareError::Config)?;
    let audience = endpoint.origin().ascii_serialization();
    let keys =
        SubscriptionKeys::parse(target.p256dh, target.auth).map_err(|_| PrepareError::Config)?;
    let body = encrypt(&keys, &payload(input.notice)?).map_err(|_| PrepareError::Encode)?;
    let now = u64::try_from(OffsetDateTime::now_utc().unix_timestamp())
        .map_err(|_| PrepareError::Encode)?;
    let authorization = target
        .vapid
        .authorization(&audience, now)
        .map_err(|_| PrepareError::Encode)?;
    Ok(Prepared {
        destination: Destination::Push(target.endpoint.to_owned()),
        body,
        delivery_header: None,
        push: Some(PushHeaders {
            authorization,
            ttl_seconds: PUSH_TTL_SECONDS,
        }),
    })
}

#[derive(Clone, PartialEq, Eq)]
pub struct PushHeaders {
    pub authorization: String,
    pub ttl_seconds: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> PushHosts {
        PushHosts::vendors()
    }

    #[test]
    fn the_vendor_list_admits_the_four_push_services_and_nothing_near_them() {
        for good in [
            "https://fcm.googleapis.com/fcm/send/abc",
            "https://updates.push.services.mozilla.com/wpush/v2/abc",
            "https://web.push.apple.com/abc",
            "https://api.sandbox.push.apple.com/abc",
            "https://wns2-par02p.notify.windows.com/w/?token=abc",
        ] {
            assert!(hosts().check_url(good).is_ok(), "{good}");
        }
        for bad in [
            "https://evil.test/fcm/send/abc",
            "https://fcm.googleapis.com.evil.test/abc",
            "https://evilfcm.googleapis.com/abc",
            "https://push.apple.com/abc",
            "https://notify.windows.com/abc",
            "https://x.push.apple.com.evil.test/abc",
            "https://fcm.googleapis.com./abc",
            "https://a b.push.apple.com/abc",
            "http://fcm.googleapis.com/abc",
            "https://fcm.googleapis.com:8443/abc",
            "https://user@fcm.googleapis.com/abc",
            "https://fcm.googleapis.com/abc#frag",
            "https://127.0.0.1/abc",
            "https://[::1]/abc",
            "ftp://fcm.googleapis.com/abc",
            "not a url",
        ] {
            assert!(hosts().check_url(bad).is_err(), "{bad}");
        }
        let long = format!(
            "https://fcm.googleapis.com/{}",
            "a".repeat(MAX_PUSH_ENDPOINT_BYTES)
        );
        assert_eq!(hosts().check_url(&long), Err(PushUrlRefusal::TooLong));
    }

    #[test]
    fn equivalent_spellings_normalise_to_one_url_and_embedded_control_characters_are_refused() {
        let canonical = hosts()
            .check_url("https://fcm.googleapis.com/fcm/send/x")
            .expect("a valid endpoint");
        for variant in [
            "https:fcm.googleapis.com/fcm/send/x",
            "https:////fcm.googleapis.com/fcm/send/x",
            "HTTPS://FCM.GoogleAPIs.com/fcm/send/x",
            "https://fcm.googleapis.com:443/fcm/send/x",
            "https://\u{ff46}\u{ff43}\u{ff4d}.googleapis.com/fcm/send/x",
        ] {
            let parsed = hosts().check_url(variant).expect(variant);
            assert_eq!(parsed.as_str(), canonical.as_str(), "{variant}");
        }
        for bad in [
            "https://fcm.googleapis.com/fcm/\tsend/x",
            "https://fcm.googleapis.com/fcm/send/x\n",
            "https://fcm.googleapis.com/fcm/se\rnd/x",
            " https://fcm.googleapis.com/fcm/send/x",
            "https://fcm.googleapis.com/fcm/send/\u{7}x",
        ] {
            assert_eq!(
                hosts().check_url(bad),
                Err(PushUrlRefusal::Invalid),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn configured_extras_extend_the_list_and_bad_patterns_are_refused() {
        let extended = hosts()
            .with_extras(&[
                "Push.Example.Test".to_owned(),
                "*.relay.example.test".to_owned(),
            ])
            .expect("valid extras");
        assert!(extended.check_url("https://push.example.test/x").is_ok());
        assert!(
            extended
                .check_url("https://a.b.relay.example.test/x")
                .is_ok()
        );
        assert!(extended.check_url("https://relay.example.test/x").is_err());
        assert!(extended.check_url("https://other.example.test/x").is_err());
        for bad in [
            "*",
            "*.com",
            "com",
            "127.0.0.1",
            "a..b",
            "*.*.example.test",
            "-a.example.test",
            "a b.example.test",
        ] {
            assert!(hosts().with_extras(&[bad.to_owned()]).is_err(), "{bad}");
        }
        let many: Vec<String> = (0..=MAX_EXTRA_PUSH_HOSTS)
            .map(|index| format!("h{index}.example.test"))
            .collect();
        assert_eq!(hosts().with_extras(&many), Err(HostPatternError::TooMany));
    }

    #[test]
    fn the_payload_is_the_summary_only_and_stays_within_three_kib() {
        let notice = Notice {
            endpoint_id: "7k2m9q4xw8c1d5f3h6j0n2p4".to_owned(),
            text: "ignored".to_owned(),
            capture: Some(json!({
                "id": "0192e5a0-0000-7000-8000-000000000001",
                "kind": "http",
                "method": "POST",
                "path": "/a\u{0001}b".repeat(2000),
                "client_ip": "203.0.113.1",
            })),
            delivery_kind: whl_store::notify::DeliveryKind::Single,
        };
        let body = payload(&notice).expect("a payload");
        assert!(body.len() <= MAX_PAYLOAD_BYTES);
        let value: Value = serde_json::from_slice(&body).expect("JSON");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["capture_id", "endpoint_id", "kind", "method", "path"]
        );
        assert_eq!(value["kind"], "http");
        assert_eq!(value["method"], "POST");
        let test = Notice {
            capture: None,
            delivery_kind: whl_store::notify::DeliveryKind::Test,
            ..notice
        };
        let value: Value =
            serde_json::from_slice(&payload(&test).expect("a payload")).expect("JSON");
        assert_eq!(value["kind"], "test");
        assert!(value["capture_id"].is_null());
        assert!(value["path"].is_null());
    }
}
