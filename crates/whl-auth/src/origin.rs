use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::http::{HeaderMap, Method, header};
use axum::response::{IntoResponse, Response};
use tower::{Layer, Service};
use url::Url;

use crate::cookies::CookiePolicy;
use crate::error::AuthRejection;

const GUARDED_PREFIXES: [&str; 2] = ["/api", "/auth"];

#[derive(Debug)]
struct Policy {
    public_origin: String,
    cookies: CookiePolicy,
}

#[derive(Debug, Clone)]
pub struct OriginCheck {
    policy: Arc<Policy>,
}

impl OriginCheck {
    pub fn new(public_origin: &str, cookies: CookiePolicy) -> Self {
        Self {
            policy: Arc::new(Policy {
                public_origin: public_origin.to_owned(),
                cookies,
            }),
        }
    }

    pub fn permits(&self, method: &Method, path: &str, headers: &HeaderMap) -> bool {
        if is_safe(method) || !is_guarded(path) {
            return true;
        }
        if headers.get_all(header::ORIGIN).iter().count() > 1 {
            return false;
        }
        if !self.policy.cookies.has_session_cookie(headers) {
            return true;
        }
        request_origin(headers).is_some_and(|origin| origin == self.policy.public_origin)
    }
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn is_guarded(path: &str) -> bool {
    GUARDED_PREFIXES.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

fn request_origin(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        return origin.to_str().ok().map(str::to_owned);
    }
    let referer = headers.get(header::REFERER)?.to_str().ok()?;
    let url = Url::parse(referer).ok()?;
    url.origin()
        .is_tuple()
        .then(|| url.origin().ascii_serialization())
}

impl<S> Layer<S> for OriginCheck {
    type Service = OriginCheckService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        OriginCheckService {
            check: self.clone(),
            inner,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OriginCheckService<S> {
    check: OriginCheck,
    inner: S,
}

type ServiceFuture = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

impl<S> Service<Request> for OriginCheckService<S>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = ServiceFuture;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if !self
            .check
            .permits(request.method(), request.uri().path(), request.headers())
        {
            tracing::warn!(
                method = %request.method(),
                path = request.uri().path(),
                "request refused by the origin check"
            );
            let response = AuthRejection::CsrfRejected.into_response();
            return Box::pin(std::future::ready(Ok(response)));
        }
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move { inner.call(request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const ORIGIN: &str = "https://whl.example";

    fn check() -> OriginCheck {
        OriginCheck::new(ORIGIN, CookiePolicy::new(true))
    }

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name.clone(), HeaderValue::from_str(value).expect("value"));
        }
        map
    }

    fn with_cookie(extra: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut map = headers(extra);
        map.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-whl_session=anything"),
        );
        map
    }

    #[test]
    fn origin_table() {
        let check = check();
        let cases: [(&str, HeaderMap, bool); 9] = [
            (
                "same origin",
                with_cookie(&[(header::ORIGIN, ORIGIN)]),
                true,
            ),
            (
                "other origin",
                with_cookie(&[(header::ORIGIN, "https://evil.example")]),
                false,
            ),
            (
                "null origin",
                with_cookie(&[(header::ORIGIN, "null")]),
                false,
            ),
            (
                "other scheme",
                with_cookie(&[(header::ORIGIN, "http://whl.example")]),
                false,
            ),
            (
                "other port",
                with_cookie(&[(header::ORIGIN, "https://whl.example:8443")]),
                false,
            ),
            (
                "same-origin referer",
                with_cookie(&[(header::REFERER, "https://whl.example/e/abc?x=1")]),
                true,
            ),
            (
                "other referer",
                with_cookie(&[(header::REFERER, "https://evil.example/e/abc")]),
                false,
            ),
            ("neither header", with_cookie(&[]), false),
            (
                "origin wins over referer",
                with_cookie(&[
                    (header::ORIGIN, "https://evil.example"),
                    (header::REFERER, "https://whl.example/"),
                ]),
                false,
            ),
        ];
        for (name, headers, expected) in cases {
            assert_eq!(
                check.permits(&Method::POST, "/api/v1/endpoints", &headers),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn every_mutating_method_is_checked_and_reads_are_not() {
        let check = check();
        let foreign = with_cookie(&[(header::ORIGIN, "https://evil.example")]);
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(!check.permits(&method, "/api/v1/endpoints/x", &foreign));
        }
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(check.permits(&method, "/api/v1/endpoints/x", &foreign));
        }
    }

    #[test]
    fn every_method_that_is_not_known_safe_is_checked() {
        let check = check();
        let foreign = with_cookie(&[(header::ORIGIN, "https://evil.example")]);
        let same = with_cookie(&[(header::ORIGIN, ORIGIN)]);
        for name in [
            "post", "get", "delete", "PROPFIND", "PURGE", "X-CUSTOM", "TRACE",
        ] {
            let method = Method::from_bytes(name.as_bytes()).expect("a valid method token");
            assert!(
                !check.permits(&method, "/api/v1/endpoints/x", &foreign),
                "{name}"
            );
            assert!(
                check.permits(&method, "/api/v1/endpoints/x", &same),
                "{name}"
            );
        }
        for method in [Method::CONNECT, Method::TRACE] {
            assert!(!check.permits(&method, "/api/v1/endpoints/x", &foreign));
        }
    }

    #[test]
    fn more_than_one_origin_header_is_refused_even_when_each_value_is_right() {
        let check = check();
        let mut doubled = with_cookie(&[(header::ORIGIN, ORIGIN)]);
        doubled.append(header::ORIGIN, HeaderValue::from_static(ORIGIN));
        assert!(!check.permits(&Method::POST, "/api/v1/endpoints", &doubled));
        let mut without_session = HeaderMap::new();
        without_session.append(header::ORIGIN, HeaderValue::from_static(ORIGIN));
        without_session.append(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!check.permits(&Method::POST, "/api/v1/endpoints", &without_session));
        assert!(check.permits(&Method::GET, "/api/v1/endpoints", &doubled));
    }

    #[test]
    fn requests_without_a_session_cookie_are_not_checked() {
        let check = check();
        let foreign = headers(&[(header::ORIGIN, "https://evil.example")]);
        assert!(check.permits(&Method::POST, "/api/v1/endpoints", &foreign));
    }

    #[test]
    fn only_api_and_auth_paths_are_guarded() {
        let check = check();
        let foreign = with_cookie(&[(header::ORIGIN, "https://evil.example")]);
        assert!(!check.permits(&Method::POST, "/auth/logout", &foreign));
        assert!(!check.permits(&Method::POST, "/api", &foreign));
        assert!(check.permits(&Method::POST, "/apix/foo", &foreign));
        assert!(check.permits(&Method::POST, "/0123456789abcdefghjkmnpq/hook", &foreign));
    }
}
