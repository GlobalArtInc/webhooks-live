use bytes::Bytes;
use http::header::{
    AUTHORIZATION, CONNECTION, CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LENGTH,
    CONTENT_LOCATION, CONTENT_TYPE, COOKIE, EXPECT, HOST, PROXY_AUTHORIZATION, TE, TRAILER,
    TRANSFER_ENCODING, UPGRADE,
};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use ipnet::IpNet;

use crate::config::Hardening;
use crate::error::RefusalReason;
use crate::message::OutboundRequest;
use crate::target::{MAX_URL_BYTES, TargetUrl, check_url};

const COOKIE2: HeaderName = HeaderName::from_static("cookie2");
const KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive");
const PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");

const MANAGED_HEADERS: [HeaderName; 11] = [
    HOST,
    CONTENT_LENGTH,
    TRANSFER_ENCODING,
    CONNECTION,
    KEEP_ALIVE,
    PROXY_CONNECTION,
    PROXY_AUTHORIZATION,
    TE,
    TRAILER,
    UPGRADE,
    EXPECT,
];

const SENSITIVE_HEADERS: [HeaderName; 4] = [AUTHORIZATION, COOKIE, COOKIE2, PROXY_AUTHORIZATION];

const BODY_HEADERS: [HeaderName; 5] = [
    CONTENT_TYPE,
    CONTENT_ENCODING,
    CONTENT_LANGUAGE,
    CONTENT_LOCATION,
    CONTENT_LENGTH,
];

pub(crate) fn is_redirect_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

pub(crate) fn strip_managed_headers(headers: &mut HeaderMap) {
    for name in MANAGED_HEADERS {
        headers.remove(name);
    }
}

fn becomes_get(status: StatusCode, method: &Method) -> bool {
    match status.as_u16() {
        301..=303 => method != Method::HEAD,
        _ => false,
    }
}

pub(crate) fn plan_redirect(
    current: OutboundRequest,
    status: StatusCode,
    location: &HeaderValue,
    allow: &[IpNet],
    hardening: &Hardening,
) -> Result<OutboundRequest, RefusalReason> {
    let raw = std::str::from_utf8(location.as_bytes()).map_err(|_| RefusalReason::InvalidUrl)?;
    if raw.len() > MAX_URL_BYTES {
        return Err(RefusalReason::UrlTooLong);
    }
    if raw.chars().any(char::is_control) {
        return Err(RefusalReason::InvalidUrl);
    }
    let joined = current
        .url
        .unpack()
        .join(raw)
        .map_err(|_| RefusalReason::InvalidUrl)?;
    let next = check_url(joined, allow, hardening)?;
    if current.url.unpack().scheme() == "https" && next.scheme() == "http" {
        return Err(RefusalReason::HttpsDowngrade);
    }

    let drop_body = becomes_get(status, &current.method);
    let method = if drop_body {
        Method::GET
    } else {
        current.method
    };
    let body = if drop_body {
        Bytes::new()
    } else {
        current.body
    };
    let mut headers = current.headers;
    if drop_body {
        for name in BODY_HEADERS {
            headers.remove(name);
        }
    }
    if current.url.unpack().origin() != next.origin() {
        for name in SENSITIVE_HEADERS {
            headers.remove(name);
        }
    }
    Ok(OutboundRequest {
        method,
        url: TargetUrl::from_checked(next),
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GuardConfig;
    use crate::target::validate_target_url;

    fn request(method: Method, url: &str) -> OutboundRequest {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        headers.insert(COOKIE, HeaderValue::from_static("session=1"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-trace", HeaderValue::from_static("kept"));
        OutboundRequest::new(
            method,
            validate_target_url(url, &GuardConfig::default()).unwrap(),
            headers,
            Bytes::from_static(b"{\"a\":1}"),
        )
    }

    fn plan(
        current: OutboundRequest,
        status: u16,
        location: &str,
    ) -> Result<OutboundRequest, RefusalReason> {
        plan_redirect(
            current,
            StatusCode::from_u16(status).unwrap(),
            &HeaderValue::from_str(location).unwrap(),
            &[],
            &Hardening::default(),
        )
    }

    #[test]
    fn resolves_relative_and_absolute_locations_and_strips_the_fragment() {
        let cases = [
            ("../c?x=1#frag", "http://example.com/c?x=1"),
            ("/root", "http://example.com/root"),
            ("sibling", "http://example.com/a/sibling"),
            ("//other.example.org/p", "http://other.example.org/p"),
            ("https://example.com/secure", "https://example.com/secure"),
        ];
        for (location, expected) in cases {
            let next = plan(
                request(Method::GET, "http://example.com/a/b"),
                302,
                location,
            )
            .unwrap();
            assert_eq!(next.url.unpack().as_str(), expected, "{location}");
        }
    }

    #[test]
    fn refuses_redirects_to_other_schemes() {
        for location in [
            "ftp://example.com/",
            "file:///etc/passwd",
            "gopher://example.com/",
            "javascript:alert(1)",
            "data:text/plain,x",
        ] {
            for current in ["http://example.com/", "https://example.com/"] {
                let error = plan(request(Method::GET, current), 302, location).unwrap_err();
                assert_eq!(error, RefusalReason::Scheme, "{current} -> {location}");
            }
        }
    }

    #[test]
    fn refuses_downgrade_from_https_to_http_but_not_the_other_directions() {
        let error = plan(
            request(Method::GET, "https://example.com/"),
            302,
            "http://example.com/",
        )
        .unwrap_err();
        assert_eq!(error, RefusalReason::HttpsDowngrade);
        assert!(
            plan(
                request(Method::GET, "http://example.com/"),
                302,
                "https://example.com/"
            )
            .is_ok()
        );
        assert!(
            plan(
                request(Method::GET, "https://example.com/"),
                302,
                "https://other.example.org/"
            )
            .is_ok()
        );
        assert!(
            plan(
                request(Method::GET, "http://example.com/"),
                302,
                "http://other.example.org/"
            )
            .is_ok()
        );
    }

    #[test]
    fn refuses_redirects_to_refused_addresses_and_unsafe_locations() {
        let cases = [
            (
                "http://169.254.169.254/latest/meta-data/",
                RefusalReason::RefusedAddress,
            ),
            ("http://[::1]:8080/", RefusalReason::RefusedAddress),
            ("//10.0.0.1/", RefusalReason::RefusedAddress),
            (
                "http://[::ffff:192.168.1.1]/",
                RefusalReason::RefusedAddress,
            ),
            ("http://localhost/", RefusalReason::ForbiddenHost),
            ("http://user:pw@example.org/", RefusalReason::Userinfo),
        ];
        for (location, reason) in cases {
            let error =
                plan(request(Method::GET, "http://example.com/"), 307, location).unwrap_err();
            assert_eq!(error, reason, "{location}");
        }
        let long = format!("/{}", "a".repeat(MAX_URL_BYTES));
        let error = plan(request(Method::GET, "http://example.com/"), 307, &long).unwrap_err();
        assert_eq!(error, RefusalReason::UrlTooLong);
        let invalid = HeaderValue::from_bytes(&[0xff, 0xfe, b'x']).unwrap();
        let error = plan_redirect(
            request(Method::GET, "http://example.com/"),
            StatusCode::FOUND,
            &invalid,
            &[],
            &Hardening::default(),
        )
        .unwrap_err();
        assert_eq!(error, RefusalReason::InvalidUrl);
    }

    #[test]
    fn allow_list_applies_to_redirect_targets() {
        let allow = ["127.0.0.1/32".parse().unwrap()];
        let current = request(Method::GET, "http://example.com/");
        let status = StatusCode::FOUND;
        let location = HeaderValue::from_static("http://127.0.0.1:9000/");
        assert!(plan_redirect(current, status, &location, &allow, &Hardening::default()).is_ok());
    }

    #[test]
    fn rewrites_the_method_on_301_302_303_and_keeps_it_on_307_308() {
        let cases = [
            (301, Method::POST, Method::GET, false),
            (302, Method::PUT, Method::GET, false),
            (303, Method::DELETE, Method::GET, false),
            (301, Method::PATCH, Method::GET, false),
            (302, Method::GET, Method::GET, false),
            (301, Method::HEAD, Method::HEAD, true),
            (303, Method::HEAD, Method::HEAD, true),
            (307, Method::POST, Method::POST, true),
            (308, Method::PUT, Method::PUT, true),
            (308, Method::DELETE, Method::DELETE, true),
        ];
        for (status, method, expected, keeps_body) in cases {
            let next = plan(
                request(method.clone(), "http://example.com/"),
                status,
                "/next",
            )
            .unwrap();
            assert_eq!(next.method, expected, "{status} {method}");
            assert_eq!(!next.body.is_empty(), keeps_body, "{status} {method}");
            assert_eq!(
                next.headers.contains_key(CONTENT_TYPE),
                keeps_body,
                "{status} {method}"
            );
        }
    }

    #[test]
    fn drops_sensitive_headers_only_when_the_origin_changes() {
        let kept = plan(request(Method::GET, "http://example.com/a"), 302, "/b").unwrap();
        assert!(kept.headers.contains_key(AUTHORIZATION));
        assert!(kept.headers.contains_key(COOKIE));

        for location in [
            "http://example.com:8080/b",
            "http://other.example.com/b",
            "https://example.com/b",
        ] {
            let next = plan(request(Method::GET, "http://example.com/a"), 302, location).unwrap();
            assert!(!next.headers.contains_key(AUTHORIZATION), "{location}");
            assert!(!next.headers.contains_key(COOKIE), "{location}");
            assert!(
                !next.headers.contains_key(PROXY_AUTHORIZATION),
                "{location}"
            );
            assert!(next.headers.contains_key("x-trace"), "{location}");
        }
    }

    #[test]
    fn strips_headers_the_client_manages() {
        let mut headers = HeaderMap::new();
        for name in [
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "keep-alive",
            "proxy-connection",
            "proxy-authorization",
            "te",
            "trailer",
            "upgrade",
            "expect",
        ] {
            headers.insert(HeaderName::from_static(name), HeaderValue::from_static("x"));
        }
        headers.insert("x-keep", HeaderValue::from_static("x"));
        strip_managed_headers(&mut headers);
        assert_eq!(headers.len(), 1);
        assert!(headers.contains_key("x-keep"));
    }
}
