use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{
    ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG,
    IF_NONE_MATCH, REFERRER_POLICY,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::route::{Target, classify};
use crate::shell::Shell;
use crate::source::{Asset, AssetSource, Dist, ETag};

pub fn router() -> Router {
    site(Box::new(Dist))
}

pub(crate) fn site(source: Box<dyn AssetSource>) -> Router {
    let shell = Shell::load(source.as_ref());
    Router::new()
        .fallback(handle)
        .with_state(Arc::new(Site { source, shell }))
}

struct Site {
    source: Box<dyn AssetSource>,
    shell: Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheRule {
    Immutable,
    NoCache,
}

impl CacheRule {
    fn header(self) -> HeaderValue {
        HeaderValue::from_static(match self {
            CacheRule::Immutable => "public, max-age=31536000, immutable",
            CacheRule::NoCache => "no-cache",
        })
    }
}

async fn handle(
    State(site): State<Arc<Site>>,
    method: Method,
    uri: Uri,
    request: HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }
    let head = method == Method::HEAD;
    match classify(uri.path()) {
        Some(Target::Hashed(key)) => match site.source.get(key) {
            Some(asset) => deliver(&asset, key, CacheRule::Immutable, None, &request, head),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Some(Target::Shell) => deliver(
            &site.shell.document,
            "index.html",
            CacheRule::NoCache,
            Some(&site.shell.csp),
            &request,
            head,
        ),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn deliver(
    asset: &Asset,
    key: &str,
    cache: CacheRule,
    csp: Option<&HeaderValue>,
    request: &HeaderMap,
    head: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, cache.header());
    headers.insert(ETAG, asset.etag.unpack().clone());
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    if let Some(csp) = csp {
        headers.insert(CONTENT_SECURITY_POLICY, csp.clone());
        headers.insert(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        );
        headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
        headers.insert(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "camera=(), microphone=(), geolocation=(), payment=(), usb=(), clipboard-write=(self)",
            ),
        );
    }
    if not_modified(request, &asset.etag) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }
    headers.insert(CONTENT_TYPE, content_type(key));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(asset.body.len()));
    let body = if head {
        Body::empty()
    } else {
        Body::from(asset.body.clone())
    };
    let mut response = Response::new(body);
    *response.headers_mut() = headers;
    response
}

fn not_modified(request: &HeaderMap, etag: &ETag) -> bool {
    request
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag.unpack()
        })
}

fn content_type(key: &str) -> HeaderValue {
    let mime = mime_guess::from_path(key).first_or_octet_stream();
    let value = if mime.essence_str().starts_with("text/") {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    };
    HeaderValue::from_str(&value)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
}
