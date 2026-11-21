use std::net::IpAddr;

use ipnet::IpNet;
use url::{Host, Url};

use crate::config::GuardConfig;
use crate::error::{ForwardError, RefusalReason};
use crate::ranges::is_refused;

pub const MAX_URL_BYTES: usize = 2048;

const FORBIDDEN_DOMAINS: [&str; 3] = ["localhost", "internal", "local"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetUrl(Url);

impl TargetUrl {
    pub fn unpack(&self) -> &Url {
        &self.0
    }

    pub(crate) fn from_checked(url: Url) -> Self {
        Self(url)
    }
}

pub fn validate_target_url(input: &str, config: &GuardConfig) -> Result<TargetUrl, ForwardError> {
    parse_url(input)
        .and_then(|url| check_url(url, &config.allow))
        .map(TargetUrl)
        .map_err(ForwardError::TargetRefused)
}

pub(crate) fn parse_url(input: &str) -> Result<Url, RefusalReason> {
    if input.len() > MAX_URL_BYTES {
        return Err(RefusalReason::UrlTooLong);
    }
    if input.chars().any(char::is_control) {
        return Err(RefusalReason::InvalidUrl);
    }
    Url::parse(input).map_err(|_| RefusalReason::InvalidUrl)
}

pub(crate) fn check_url(mut url: Url, allow: &[IpNet]) -> Result<Url, RefusalReason> {
    if url.as_str().len() > MAX_URL_BYTES {
        return Err(RefusalReason::UrlTooLong);
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(RefusalReason::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(RefusalReason::Userinfo);
    }
    match url.host() {
        None => return Err(RefusalReason::MissingHost),
        Some(Host::Ipv4(address)) => check_address(IpAddr::V4(address), allow)?,
        Some(Host::Ipv6(address)) => check_address(IpAddr::V6(address), allow)?,
        Some(Host::Domain(domain)) => check_domain(domain, allow)?,
    }
    url.set_fragment(None);
    Ok(url)
}

fn check_address(address: IpAddr, allow: &[IpNet]) -> Result<(), RefusalReason> {
    if is_refused(address, allow) {
        Err(RefusalReason::RefusedAddress)
    } else {
        Ok(())
    }
}

fn check_domain(domain: &str, allow: &[IpNet]) -> Result<(), RefusalReason> {
    let name = domain.trim_end_matches('.');
    if name.is_empty() {
        return Err(RefusalReason::MissingHost);
    }
    if let Ok(address) = name.parse::<IpAddr>() {
        return check_address(address, allow);
    }
    if !allow.is_empty() {
        return Ok(());
    }
    let forbidden = FORBIDDEN_DOMAINS.iter().any(|forbidden| {
        name == *forbidden
            || name
                .strip_suffix(*forbidden)
                .is_some_and(|rest| rest.ends_with('.'))
    });
    if forbidden {
        Err(RefusalReason::ForbiddenHost)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loopback_allowed() -> GuardConfig {
        GuardConfig {
            allow: vec!["127.0.0.1/32".parse().unwrap()],
            ..GuardConfig::default()
        }
    }

    fn refusal(input: &str) -> Option<RefusalReason> {
        validate_target_url(input, &GuardConfig::default())
            .err()
            .and_then(|error| error.refusal())
    }

    #[test]
    fn accepts_http_and_https_host_names_and_public_literals() {
        for input in [
            "http://example.com/hook",
            "https://example.com:8443/hook?a=1",
            "https://8.8.8.8/",
            "https://[2606:4700:4700::1111]/",
            "http://hooks.example.org./",
        ] {
            assert!(
                validate_target_url(input, &GuardConfig::default()).is_ok(),
                "{input}"
            );
        }
    }

    #[test]
    fn strips_the_fragment() {
        let target =
            validate_target_url("https://example.com/a?b=c#secret", &GuardConfig::default())
                .unwrap();
        assert_eq!(target.unpack().as_str(), "https://example.com/a?b=c");
    }

    #[test]
    fn refuses_other_schemes() {
        for input in [
            "ftp://example.com/",
            "file:///etc/passwd",
            "gopher://example.com/",
            "javascript:alert(1)",
            "data:text/plain,x",
        ] {
            assert_eq!(refusal(input), Some(RefusalReason::Scheme), "{input}");
        }
    }

    #[test]
    fn refuses_user_information() {
        for input in [
            "http://user:pass@example.com/",
            "http://user@example.com/",
            "http://:pass@example.com/",
        ] {
            assert_eq!(refusal(input), Some(RefusalReason::Userinfo), "{input}");
        }
    }

    #[test]
    fn refuses_missing_host_and_unparsable_input() {
        assert_eq!(refusal("http://"), Some(RefusalReason::InvalidUrl));
        assert_eq!(refusal("not a url"), Some(RefusalReason::InvalidUrl));
        assert_eq!(
            refusal("http://exa\nmple.com/"),
            Some(RefusalReason::InvalidUrl)
        );
        assert_eq!(
            refusal("http://example.com/\u{0}"),
            Some(RefusalReason::InvalidUrl)
        );
        assert_eq!(refusal(""), Some(RefusalReason::InvalidUrl));
    }

    #[test]
    fn refuses_urls_longer_than_the_limit() {
        let long = format!("http://example.com/{}", "a".repeat(MAX_URL_BYTES));
        assert_eq!(refusal(&long), Some(RefusalReason::UrlTooLong));
        let exact = format!(
            "http://example.com/{}",
            "a".repeat(MAX_URL_BYTES - "http://example.com/".len())
        );
        assert_eq!(exact.len(), MAX_URL_BYTES);
        assert!(validate_target_url(&exact, &GuardConfig::default()).is_ok());
    }

    #[test]
    fn refuses_literal_addresses_in_refused_ranges_however_they_are_written() {
        for input in [
            "http://127.0.0.1/",
            "http://10.1.2.3:8080/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:7f00:1]/",
            "http://[64:ff9b::a00:1]/",
            "http://[2002:a9fe:a9fe::]/",
            "http://[fe80::1]/",
            "http://0x7f.1/",
            "http://2130706433/",
            "http://0177.0.0.1/",
            "http://127.1/",
            "http://%31%32%37.0.0.1/",
            "http://0.0.0.0/",
        ] {
            assert_eq!(
                refusal(input),
                Some(RefusalReason::RefusedAddress),
                "{input}"
            );
        }
    }

    #[test]
    fn refuses_local_host_names_when_the_allow_list_is_empty() {
        for input in [
            "http://localhost/",
            "http://LOCALHOST:8080/",
            "http://localhost./",
            "http://app.localhost/",
            "http://metadata.google.internal/",
            "http://db.svc.cluster.local/",
            "http://printer.local/",
            "http://internal/",
        ] {
            assert_eq!(
                refusal(input),
                Some(RefusalReason::ForbiddenHost),
                "{input}"
            );
        }
        assert_eq!(refusal("http://notlocalhost.example.com/"), None);
        assert_eq!(refusal("http://localhost.example.com/"), None);
        assert_eq!(refusal("http://myinternal.example.com/"), None);
    }

    #[test]
    fn non_empty_allow_list_opens_literal_addresses_and_defers_names_to_the_resolver() {
        let config = loopback_allowed();
        assert!(validate_target_url("http://127.0.0.1:9000/", &config).is_ok());
        assert!(validate_target_url("http://127.0.0.2:9000/", &config).is_err());
        for input in [
            "http://localhost:9000/",
            "http://app.localhost/",
            "http://svc.internal/",
            "http://printer.local/",
        ] {
            assert!(validate_target_url(input, &config).is_ok(), "{input}");
        }
    }
}
