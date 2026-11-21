use url::{Host, Url};
use whl_core::endpoint_id::{ENDPOINT_ID_LEN, EndpointId};
use whl_forward::{GuardConfig, RefusalReason, TargetUrl, validate_target_url};

use crate::error::{FieldCode, FieldViolation};

const FIELD: &str = "url";
const MAX_URL_BYTES: usize = 2048;

#[derive(Debug)]
pub(crate) struct LocalTarget {
    pub host: String,
    pub path_and_query: String,
}

#[derive(Debug)]
pub(crate) enum Destination {
    Local(LocalTarget),
    External(TargetUrl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicOrigin {
    host: Host<String>,
    port: u16,
    authority: String,
}

impl PublicOrigin {
    pub fn parse(public_url: &str) -> Option<Self> {
        let url = Url::parse(public_url).ok()?;
        let host = normalized_host(&url)?;
        let port = url.port_or_known_default()?;
        let authority = match url.port() {
            Some(explicit) => format!("{}:{explicit}", host_text(&host)),
            None => host_text(&host),
        };
        Some(Self {
            host,
            port,
            authority,
        })
    }

    fn matches(&self, url: &Url) -> bool {
        normalized_host(url).is_some_and(|host| host == self.host)
            && url.port_or_known_default() == Some(self.port)
    }
}

fn normalized_host(url: &Url) -> Option<Host<String>> {
    match url.host()? {
        Host::Domain(domain) => Some(Host::Domain(
            domain.trim_end_matches('.').to_ascii_lowercase(),
        )),
        Host::Ipv4(address) => Some(Host::Ipv4(address)),
        Host::Ipv6(address) => Some(Host::Ipv6(address)),
    }
}

fn host_text(host: &Host<String>) -> String {
    match host {
        Host::Domain(domain) => domain.clone(),
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => format!("[{address}]"),
    }
}

pub(crate) fn parse_url(text: &str) -> Result<Url, FieldViolation> {
    if text.is_empty() {
        return Err(violation(FieldCode::Required, "is required"));
    }
    if text.len() > MAX_URL_BYTES {
        return Err(violation(FieldCode::TooLong, "is longer than 2048 bytes"));
    }
    if text.chars().any(char::is_control) {
        return Err(violation(
            FieldCode::InvalidFormat,
            "must not contain control characters",
        ));
    }
    Url::parse(text).map_err(|_| {
        violation(
            FieldCode::InvalidFormat,
            "must be an absolute http or https URL",
        )
    })
}

pub(crate) fn classify(
    url: &Url,
    origin: Option<&PublicOrigin>,
    guard: &GuardConfig,
) -> Result<Destination, FieldViolation> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(violation(
            FieldCode::InvalidFormat,
            "must be an absolute http or https URL",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(violation(
            FieldCode::TargetRefused,
            "must not carry user information",
        ));
    }
    if let Some(origin) = origin.filter(|origin| origin.matches(url)) {
        return local(url, origin);
    }
    validate_target_url(url.as_str(), guard)
        .map(Destination::External)
        .map_err(|error| {
            let reason = error.refusal().unwrap_or(RefusalReason::InvalidUrl);
            violation(refusal_code(reason), reason.to_string())
        })
}

fn local(url: &Url, origin: &PublicOrigin) -> Result<Destination, FieldViolation> {
    let first = url
        .path()
        .strip_prefix('/')
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default();
    let boundary_ok = first.len() == ENDPOINT_ID_LEN && EndpointId::parse(first).is_ok();
    if !boundary_ok {
        return Err(violation(
            FieldCode::TargetRefused,
            "this instance can only be called at a capture URL",
        ));
    }
    let path_and_query = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    Ok(Destination::Local(LocalTarget {
        host: origin.authority.clone(),
        path_and_query,
    }))
}

fn refusal_code(reason: RefusalReason) -> FieldCode {
    match reason {
        RefusalReason::InvalidUrl | RefusalReason::MissingHost | RefusalReason::Scheme => {
            FieldCode::InvalidFormat
        }
        RefusalReason::UrlTooLong => FieldCode::TooLong,
        RefusalReason::Userinfo
        | RefusalReason::ForbiddenHost
        | RefusalReason::ForbiddenPort
        | RefusalReason::RefusedAddress
        | RefusalReason::HttpsDowngrade => FieldCode::TargetRefused,
    }
}

fn violation(code: FieldCode, message: impl Into<String>) -> FieldViolation {
    FieldViolation::new(FIELD, code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

    fn origin() -> PublicOrigin {
        PublicOrigin::parse("https://whl.test").expect("the public URL parses")
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("the test URL parses")
    }

    fn destination(text: &str) -> Result<Destination, FieldViolation> {
        classify(&url(text), Some(&origin()), &GuardConfig::default())
    }

    #[test]
    fn the_public_host_with_a_capture_path_is_local_whatever_the_spelling() {
        for text in [
            format!("https://whl.test/{ID}"),
            format!("https://WHL.test./{ID}/a/b?x=1"),
            format!("https://whl.test:443/{ID}"),
            format!("http://whl.test:443/{ID}"),
        ] {
            assert!(
                matches!(destination(&text), Ok(Destination::Local(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn the_public_host_outside_a_capture_path_is_refused_not_sent() {
        for text in [
            "https://whl.test/".to_owned(),
            "https://whl.test/api/v1/instance".to_owned(),
            format!("https://whl.test/{ID}/../api/v1/instance"),
            "https://whl.test/%37k2m9q4xw8c1d5f3h6j0n2p4".to_owned(),
            "https://whl.test/short".to_owned(),
        ] {
            let refused = destination(&text).expect_err(&text);
            assert_eq!(refused.code(), FieldCode::TargetRefused, "{text}");
        }
    }

    #[test]
    fn other_hosts_go_through_the_guard() {
        assert!(matches!(
            destination("https://example.com/x"),
            Ok(Destination::External(_))
        ));
        for text in [
            "http://127.0.0.1/x",
            "http://169.254.169.254/latest",
            "http://localhost/x",
            "http://[::1]/x",
            "http://10.0.0.5:8080/",
            "ftp://example.com/x",
            "https://user:pw@example.com/x",
        ] {
            assert!(destination(text).is_err(), "{text}");
        }
    }
}
