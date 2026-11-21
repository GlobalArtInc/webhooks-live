use std::borrow::Cow;
use std::collections::BTreeMap;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use crate::serve::site;
use crate::source::{Asset, AssetSource};

const INLINE_SCRIPT: &str = "(function () { var theme = 'dark'; })();";
const INDEX: &str = "<!doctype html><html><head><script>(function () { var theme = 'dark'; })();</script></head><body><div id=\"root\"></div></body></html>";
const JS: &str = "console.log(1);";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NO_INLINE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob:; worker-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl AssetSource for Memory {
    fn get(&self, path: &str) -> Option<Asset> {
        self.0
            .get(path)
            .map(|body| Asset::new(Cow::Owned(body.clone()), Sha256::digest(body).into()))
    }
}

fn app(files: &[(&str, &str)]) -> Router {
    let map = files
        .iter()
        .map(|(path, body)| (path.to_string(), body.as_bytes().to_vec()))
        .collect();
    site(Box::new(Memory(map)))
}

fn built() -> Router {
    app(&[
        ("index.html", INDEX),
        ("assets/index-abc123.js", JS),
        ("assets/index-abc123.css", "body{}"),
    ])
}

async fn send(app: Router, method: Method, uri: &str, headers: &[(&str, &str)]) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn get(app: Router, uri: &str) -> Response {
    send(app, Method::GET, uri, &[]).await
}

fn header(response: &Response, name: &str) -> String {
    response
        .headers()
        .get(name)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn hashed_asset_is_cached_as_immutable() {
    let response = get(built(), "/assets/index-abc123.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "cache-control"), IMMUTABLE);
}

#[tokio::test]
async fn index_is_served_with_no_cache() {
    let response = get(built(), "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "cache-control"), "no-cache");
}

#[tokio::test]
async fn every_ui_route_serves_index() {
    let routes = [
        "/",
        "/endpoints",
        "/signin",
        "/settings",
        "/e/abc123",
        "/e/abc123/c/def456",
        "/e/abc123/settings",
        "/e/abc123/forwarding",
        "/e/abc123/diff/def456/ghi789",
    ];
    for uri in routes {
        let response = get(built(), uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(text(response).await, INDEX, "{uri}");
    }
}

#[tokio::test]
async fn missing_file_with_extension_is_not_found() {
    assert_eq!(
        get(built(), "/missing.css").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(built(), "/e/abc123.js").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn missing_asset_under_assets_is_not_found() {
    assert_eq!(
        get(built(), "/assets/missing-000.js").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn paths_outside_the_route_table_are_not_found() {
    assert_eq!(
        get(built(), "/unknown").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(built(), "/e/abc123/diff/def456").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn dot_segments_are_not_found() {
    assert_eq!(
        get(built(), "/assets/../index.html").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn post_is_method_not_allowed() {
    let response = send(built(), Method::POST, "/", &[]).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header(&response, "allow"), "GET, HEAD");
}

#[tokio::test]
async fn head_returns_headers_without_body() {
    let response = send(built(), Method::HEAD, "/assets/index-abc123.js", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "cache-control"), IMMUTABLE);
    assert_eq!(header(&response, "content-length"), JS.len().to_string());
    assert!(text(response).await.is_empty());
}

#[tokio::test]
async fn matching_etag_returns_not_modified() {
    let etag = header(&get(built(), "/assets/index-abc123.js").await, "etag");
    let response = send(
        built(),
        Method::GET,
        "/assets/index-abc123.js",
        &[("if-none-match", etag.as_str())],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(header(&response, "etag"), etag);
    assert!(text(response).await.is_empty());
}

#[tokio::test]
async fn index_csp_carries_hash_of_inline_script() {
    let response = get(built(), "/").await;
    let hash = STANDARD.encode(Sha256::digest(INLINE_SCRIPT));
    assert_eq!(
        header(&response, "content-security-policy"),
        format!(
            "default-src 'none'; script-src 'self' 'sha256-{hash}'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob:; worker-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
        )
    );
}

#[tokio::test]
async fn csp_has_no_inline_permission_when_hash_cannot_be_computed() {
    let broken =
        "<!doctype html><html><head><script>var theme = 'dark';</head><body></body></html>";
    let response = get(app(&[("index.html", broken)]), "/").await;
    assert_eq!(header(&response, "content-security-policy"), NO_INLINE_CSP);
}

#[tokio::test]
async fn ui_security_headers_accompany_index() {
    let response = get(built(), "/").await;
    assert_eq!(header(&response, "x-frame-options"), "DENY");
    assert_eq!(header(&response, "referrer-policy"), "no-referrer");
    assert_eq!(header(&response, "x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn permissions_policy_accompanies_index() {
    let response = get(built(), "/").await;
    assert_eq!(
        header(&response, "permissions-policy"),
        "camera=(), microphone=(), geolocation=(), payment=(), usb=(), clipboard-write=(self)"
    );
}

#[tokio::test]
async fn static_assets_carry_nosniff() {
    let response = get(built(), "/assets/index-abc123.js").await;
    assert_eq!(header(&response, "x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn content_types_follow_extension() {
    let cases = [
        ("assets/app.js", "text/javascript; charset=utf-8"),
        ("assets/app.css", "text/css; charset=utf-8"),
        ("assets/font.woff2", "font/woff2"),
        ("assets/logo.svg", "image/svg+xml"),
        ("assets/logo.png", "image/png"),
        ("assets/icon.ico", "image/x-icon"),
        ("assets/data.json", "application/json"),
    ];
    let files: Vec<(&str, &str)> = cases.iter().map(|(path, _)| (*path, "x")).collect();
    for (path, expected) in cases {
        let response = get(app(&files), &format!("/{path}")).await;
        assert_eq!(header(&response, "content-type"), expected, "{path}");
    }
}

#[tokio::test]
async fn placeholder_page_when_source_is_empty() {
    let response = get(app(&[]), "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "cache-control"), "no-cache");
    assert!(header(&response, "content-security-policy").contains("script-src 'self';"));
    assert!(text(response).await.contains("not built"));
}
