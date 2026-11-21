use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

const API_PREFIX: &str = "/api/v1";
const ALLOW_ORIGIN: HeaderValue = HeaderValue::from_static("*");
const ALLOW_METHODS: HeaderValue = HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE");
const ALLOW_HEADERS: HeaderValue =
    HeaderValue::from_static("Authorization, Content-Type, Last-Event-ID, X-Webhooks-Live-Grant");
const MAX_AGE: HeaderValue = HeaderValue::from_static("600");
const EXPOSE_HEADERS: HeaderValue =
    HeaderValue::from_static("Location, RateLimit-Limit, RateLimit-Remaining, Retry-After");
const REQUEST_METHOD: HeaderName = HeaderName::from_static("access-control-request-method");

fn is_api(path: &str) -> bool {
    path.strip_prefix(API_PREFIX)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn is_preflight(method: &Method, headers: &HeaderMap) -> bool {
    *method == Method::OPTIONS
        && headers.contains_key(header::ORIGIN)
        && headers.contains_key(REQUEST_METHOD)
}

pub async fn api_cors(request: Request, next: Next) -> Response {
    if !is_api(request.uri().path()) {
        return next.run(request).await;
    }
    if is_preflight(request.method(), request.headers()) {
        let mut response = Response::new(axum::body::Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        let headers = response.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, ALLOW_ORIGIN);
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, ALLOW_METHODS);
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, ALLOW_HEADERS);
        headers.insert(header::ACCESS_CONTROL_MAX_AGE, MAX_AGE);
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, ALLOW_ORIGIN);
    headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, EXPOSE_HEADERS);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_api_paths_are_covered() {
        assert!(is_api("/api/v1"));
        assert!(is_api("/api/v1/endpoints"));
        assert!(!is_api("/api/v10"));
        assert!(!is_api("/auth/login"));
        assert!(!is_api("/api"));
        assert!(!is_api("/e/abc"));
    }

    #[test]
    fn a_preflight_needs_the_origin_and_the_requested_method() {
        let mut headers = HeaderMap::new();
        assert!(!is_preflight(&Method::OPTIONS, &headers));
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://x.test"));
        assert!(!is_preflight(&Method::OPTIONS, &headers));
        headers.insert(REQUEST_METHOD, HeaderValue::from_static("POST"));
        assert!(is_preflight(&Method::OPTIONS, &headers));
        assert!(!is_preflight(&Method::GET, &headers));
    }
}
