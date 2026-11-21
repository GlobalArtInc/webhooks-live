use std::sync::OnceLock;

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::error::ApiError;

const CACHE_CONTROL: HeaderValue = HeaderValue::from_static("no-cache");
const JSON: HeaderValue = HeaderValue::from_static("application/json");
const WEAK_PREFIX: &str = "W/";
const DIGEST_BYTES: usize = 4;

struct Rendered {
    body: Bytes,
    etag: HeaderValue,
}

static RENDERED: OnceLock<Option<Rendered>> = OnceLock::new();

fn rendered() -> Option<&'static Rendered> {
    RENDERED
        .get_or_init(|| {
            let body = serde_json::to_vec(&super::document())
                .inspect_err(
                    |error| tracing::error!(%error, "the OpenAPI document could not be rendered"),
                )
                .ok()?;
            let etag = HeaderValue::from_str(&entity_tag(&body)).ok()?;
            Some(Rendered {
                body: Bytes::from(body),
                etag,
            })
        })
        .as_ref()
}

fn entity_tag(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    let hex: String = digest
        .iter()
        .take(DIGEST_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("\"{}-{hex}\"", env!("CARGO_PKG_VERSION"))
}

fn matches(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let current = etag.to_str().unwrap_or_default();
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate.strip_prefix(WEAK_PREFIX).unwrap_or(candidate) == current
        })
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    operation_id = "openapi_get",
    tag = "Meta",
    summary = "The OpenAPI 3.1 document of this API",
    description = "Public in every access mode. The `ETag` is the build version plus a digest of the document; send it back in `If-None-Match` to get 304.",
    security(()),
    responses(
        (status = 200, description = "The document", content_type = "application/json", body = Object),
        (status = 304, description = "The document is unchanged since the `ETag` in `If-None-Match`")
    )
)]
pub(super) async fn show(headers: HeaderMap) -> Result<Response, ApiError> {
    let rendered = rendered().ok_or(ApiError::InternalFault(
        "the OpenAPI document could not be rendered",
    ))?;
    let mut response = if matches(&headers, &rendered.etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let mut response = rendered.body.clone().into_response();
        response.headers_mut().insert(header::CONTENT_TYPE, JSON);
        response
    };
    let headers = response.headers_mut();
    headers.insert(header::ETAG, rendered.etag.clone());
    headers.insert(header::CACHE_CONTROL, CACHE_CONTROL);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entity_tag_carries_the_version_and_changes_with_the_document() {
        let tag = entity_tag(b"{\"paths\":{}}");
        let digest = tag
            .strip_prefix(&format!("\"{}-", env!("CARGO_PKG_VERSION")))
            .and_then(|rest| rest.strip_suffix('"'))
            .expect("version, dash, digest");
        assert_eq!(digest.len(), 2 * DIGEST_BYTES);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(tag, entity_tag(b"{\"paths\":{}}"));
        assert_ne!(tag, entity_tag(b"{\"paths\":{\"/usage\":{}}}"));
    }

    #[test]
    fn a_stale_or_foreign_tag_does_not_match_and_a_listed_weak_or_star_tag_does() {
        let etag = HeaderValue::from_static("\"1-aaaaaaaa\"");
        let with = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static(value));
            matches(&headers, &etag)
        };
        assert!(with("\"1-aaaaaaaa\""));
        assert!(with("\"x\", W/\"1-aaaaaaaa\""));
        assert!(with("*"));
        assert!(!with("\"1-bbbbbbbb\""));
        assert!(!matches(&HeaderMap::new(), &etag));
    }
}
