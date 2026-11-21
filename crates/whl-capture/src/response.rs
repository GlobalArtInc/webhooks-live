use std::convert::Infallible;

use axum::body::Body;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_EXPOSE_HEADERS, ACCESS_CONTROL_MAX_AGE, ACCESS_CONTROL_REQUEST_HEADERS,
    ACCESS_CONTROL_REQUEST_METHOD, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY,
    CONTENT_TYPE, ORIGIN, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use whl_core::content_type::ContentType;
use whl_core::header::{HeaderInput, MAX_HEADER_NAME_BYTES, MAX_HEADERS, is_token};
use whl_core::response::{ResponseConfig, ResponseConfigInput, compose_response};
use whl_core::status::HttpStatus;
use whl_core::validation::ValidationError;
use whl_rules::{FailureCode, ResponseDraft};
use whl_store::captures::{Endpoint, StoredCapture};

use crate::template::{FallbackCode, RenderedParts};

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const ENDPOINT_ID: HeaderName = HeaderName::from_static("x-endpoint-id");
const SUBSTITUTED_STATUS: HeaderName =
    HeaderName::from_static("x-webhooks-live-substituted-status");
const TEMPLATE_STATE: HeaderName = HeaderName::from_static("x-webhooks-live-template");
const TEMPLATE_ERROR: HeaderName = HeaderName::from_static("x-webhooks-live-template-error");
const TEMPLATE_STATE_ERROR: &str = "error";
const RULES_STATE: HeaderName = HeaderName::from_static("x-webhooks-live-rules");
const RULES_ERROR: HeaderName = HeaderName::from_static("x-webhooks-live-rule-error");
const RULES_STATE_ERROR: &str = "error";

const RESERVED_PREFIX: &str = "x-webhooks-live-";
const SANDBOX: &str = "sandbox";
const NOSNIFF: &str = "nosniff";
const NO_STORE: &str = "no-store";
const ANY_ORIGIN: &str = "*";
const EXPOSED_HEADERS: &str = "X-Request-Id, X-Endpoint-Id";
const PREFLIGHT_MAX_AGE: &str = "600";
const PREFLIGHT_MAX_HEADER_NAMES: usize = MAX_HEADERS;
const SUBSTITUTE_STATUS: StatusCode = StatusCode::OK;

#[derive(Debug, Clone, Copy)]
pub struct ReplyRequest<'a> {
    pub method: &'a Method,
    pub headers: &'a HeaderMap,
}

#[derive(Debug, thiserror::Error)]
enum ResponseError {
    #[error("the stored response of endpoint {endpoint} is invalid: {source}")]
    Config {
        endpoint: String,
        source: ValidationError,
    },
    #[error("the value of header {name} cannot be sent")]
    HeaderValue { name: String },
    #[error("the header name {name} cannot be sent")]
    HeaderName { name: String },
    #[error("the status {0} cannot be sent")]
    Status(u16),
    #[error("the stored content type of endpoint {endpoint} is invalid")]
    ContentType { endpoint: String },
}

#[derive(Debug)]
pub(crate) enum Content {
    Configured,
    Rendered(RenderedParts),
    Fallback(FallbackCode),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Rules<'a> {
    None,
    Applied(&'a ResponseDraft),
    Failed(FailureCode),
}

struct Parts {
    status: u16,
    headers: Vec<(String, String)>,
    content_type: String,
    body: Bytes,
}

struct Preflight {
    method: String,
    headers: Option<String>,
}

pub fn build_response(
    endpoint: &Endpoint,
    capture: &StoredCapture,
    status_override: Option<whl_core::status::HttpStatus>,
    request: &ReplyRequest<'_>,
) -> Response {
    build_content(
        endpoint,
        capture,
        status_override,
        request,
        Content::Configured,
    )
}

pub(crate) fn is_preflight(endpoint: &Endpoint, request: &ReplyRequest<'_>) -> bool {
    endpoint.cors && preflight_of(request).is_some()
}

pub(crate) fn build_content(
    endpoint: &Endpoint,
    capture: &StoredCapture,
    status_override: Option<whl_core::status::HttpStatus>,
    request: &ReplyRequest<'_>,
    content: Content,
) -> Response {
    build_ruled(
        endpoint,
        capture,
        status_override,
        request,
        content,
        Rules::None,
    )
}

pub(crate) fn build_ruled(
    endpoint: &Endpoint,
    capture: &StoredCapture,
    status_override: Option<whl_core::status::HttpStatus>,
    request: &ReplyRequest<'_>,
    content: Content,
    rules: Rules<'_>,
) -> Response {
    match compose(endpoint, capture, status_override, request, content, rules) {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, endpoint_id = endpoint.id.unpack(), "the configured response cannot be served");
            failure(endpoint, capture)
        }
    }
}

fn compose(
    endpoint: &Endpoint,
    capture: &StoredCapture,
    status_override: Option<whl_core::status::HttpStatus>,
    request: &ReplyRequest<'_>,
    content: Content,
    rules: Rules<'_>,
) -> Result<Response, ResponseError> {
    let fallback = match &content {
        Content::Fallback(code) => Some(*code),
        Content::Configured | Content::Rendered(_) => None,
    };
    let preflight = if endpoint.cors {
        preflight_of(request)
    } else {
        None
    };
    let mut headers = HeaderMap::new();
    let (status, body) = match &preflight {
        Some(_) => (StatusCode::NO_CONTENT, Bytes::new()),
        None => configured(
            endpoint,
            status_override,
            request,
            &mut headers,
            content,
            rules,
        )?,
    };
    insert_mandatory(&mut headers, endpoint, capture)?;
    if let Some(code) = fallback {
        insert_fallback(&mut headers, code)?;
    }
    if let Rules::Failed(code) = rules
        && preflight.is_none()
    {
        insert_rule_failure(&mut headers, code)?;
    }
    if endpoint.cors {
        insert_cors(&mut headers);
    }
    if let Some(preflight) = &preflight {
        insert_preflight(&mut headers, preflight)?;
    }
    let payload = match status {
        StatusCode::RESET_CONTENT => {
            headers.insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
            Body::empty()
        }
        _ if has_no_content(status) => unsized_empty_body(),
        _ => Body::from(body),
    };
    let mut response = Response::new(payload);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}

fn configured(
    endpoint: &Endpoint,
    status_override: Option<whl_core::status::HttpStatus>,
    request: &ReplyRequest<'_>,
    headers: &mut HeaderMap,
    content: Content,
    rules: Rules<'_>,
) -> Result<(StatusCode, Bytes), ResponseError> {
    if let Rules::Applied(draft) = rules {
        let base = base_parts(endpoint, status_override, content)?;
        let Parts {
            status,
            headers: pairs,
            content_type,
            body,
        } = overlay(base, draft, status_override);
        return finish(
            headers,
            status,
            pairs
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
            &content_type,
            || body,
            request.method,
        );
    }
    match content {
        Content::Configured => {
            let config = rebuild_config(endpoint)?;
            let composed = compose_response(&config, status_override);
            finish(
                headers,
                composed.status.unpack(),
                composed
                    .headers
                    .iter()
                    .map(|header| (header.name.unpack(), header.value.unpack())),
                composed.content_type.unpack(),
                || Bytes::copy_from_slice(config.body()),
                request.method,
            )
        }
        Content::Rendered(parts) => {
            let content_type = stored_content_type(endpoint)?;
            let requested = status_override
                .or(parts.status)
                .unwrap_or(endpoint.status)
                .unpack();
            let RenderedParts {
                headers: rendered,
                body,
                ..
            } = parts;
            finish(
                headers,
                requested,
                rendered
                    .iter()
                    .map(|header| (header.name.as_str(), header.value.as_str())),
                content_type.unpack(),
                || Bytes::from(body),
                request.method,
            )
        }
        Content::Fallback(_) => {
            let content_type = stored_content_type(endpoint)?;
            let requested = status_override.unwrap_or(endpoint.status).unpack();
            finish(
                headers,
                requested,
                std::iter::empty(),
                content_type.unpack(),
                Bytes::new,
                request.method,
            )
        }
    }
}

fn base_parts(
    endpoint: &Endpoint,
    status_override: Option<HttpStatus>,
    content: Content,
) -> Result<Parts, ResponseError> {
    match content {
        Content::Configured => {
            let config = rebuild_config(endpoint)?;
            let composed = compose_response(&config, status_override);
            Ok(Parts {
                status: composed.status.unpack(),
                headers: composed
                    .headers
                    .iter()
                    .map(|header| {
                        (
                            header.name.unpack().to_owned(),
                            header.value.unpack().to_owned(),
                        )
                    })
                    .collect(),
                content_type: composed.content_type.unpack().to_owned(),
                body: Bytes::copy_from_slice(config.body()),
            })
        }
        Content::Rendered(parts) => {
            let content_type = stored_content_type(endpoint)?;
            let status = status_override
                .or(parts.status)
                .unwrap_or(endpoint.status)
                .unpack();
            Ok(Parts {
                status,
                headers: parts
                    .headers
                    .into_iter()
                    .map(|header| (header.name, header.value))
                    .collect(),
                content_type: content_type.unpack().to_owned(),
                body: Bytes::from(parts.body),
            })
        }
        Content::Fallback(_) => {
            let content_type = stored_content_type(endpoint)?;
            Ok(Parts {
                status: status_override.unwrap_or(endpoint.status).unpack(),
                headers: Vec::new(),
                content_type: content_type.unpack().to_owned(),
                body: Bytes::new(),
            })
        }
    }
}

fn overlay(base: Parts, draft: &ResponseDraft, status_override: Option<HttpStatus>) -> Parts {
    Parts {
        status: status_override
            .map(|status| status.unpack())
            .or(draft.status)
            .unwrap_or(base.status),
        content_type: draft.content_type.clone().unwrap_or(base.content_type),
        headers: draft.apply_headers(base.headers),
        body: draft
            .body
            .as_ref()
            .map_or(base.body, |body| Bytes::copy_from_slice(body)),
    }
}

fn stored_content_type(endpoint: &Endpoint) -> Result<ContentType, ResponseError> {
    ContentType::parse(&endpoint.content_type).map_err(|_| ResponseError::ContentType {
        endpoint: endpoint.id.unpack().to_owned(),
    })
}

fn finish<'a>(
    headers: &mut HeaderMap,
    requested: u16,
    configured: impl Iterator<Item = (&'a str, &'a str)>,
    content_type: &str,
    body: impl FnOnce() -> Bytes,
    method: &Method,
) -> Result<(StatusCode, Bytes), ResponseError> {
    let requested_code =
        StatusCode::from_u16(requested).map_err(|_| ResponseError::Status(requested))?;

    for (name, value) in configured {
        if name.to_ascii_lowercase().starts_with(RESERVED_PREFIX) {
            continue;
        }
        let parsed_name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| ResponseError::HeaderName {
                name: name.to_owned(),
            })?;
        let value = HeaderValue::from_str(value).map_err(|_| ResponseError::HeaderValue {
            name: name.to_owned(),
        })?;
        headers.append(parsed_name, value);
    }

    let status = if requested_code.is_informational() {
        headers.insert(
            SUBSTITUTED_STATUS,
            HeaderValue::from_str(&requested.to_string()).map_err(|_| {
                ResponseError::HeaderValue {
                    name: SUBSTITUTED_STATUS.as_str().to_owned(),
                }
            })?,
        );
        SUBSTITUTE_STATUS
    } else {
        requested_code
    };

    if has_no_content(status) {
        return Ok((status, Bytes::new()));
    }

    let payload = body();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).map_err(|_| ResponseError::HeaderValue {
            name: CONTENT_TYPE.as_str().to_owned(),
        })?,
    );
    headers.insert(CONTENT_LENGTH, HeaderValue::from(payload.len()));
    if method == Method::HEAD {
        return Ok((status, Bytes::new()));
    }
    Ok((status, payload))
}

fn insert_fallback(headers: &mut HeaderMap, code: FallbackCode) -> Result<(), ResponseError> {
    headers.insert(
        TEMPLATE_STATE,
        HeaderValue::from_static(TEMPLATE_STATE_ERROR),
    );
    headers.insert(
        TEMPLATE_ERROR,
        HeaderValue::from_str(code.as_str()).map_err(|_| ResponseError::HeaderValue {
            name: TEMPLATE_ERROR.as_str().to_owned(),
        })?,
    );
    Ok(())
}

fn insert_rule_failure(headers: &mut HeaderMap, code: FailureCode) -> Result<(), ResponseError> {
    headers.insert(RULES_STATE, HeaderValue::from_static(RULES_STATE_ERROR));
    headers.insert(
        RULES_ERROR,
        HeaderValue::from_str(code.as_str()).map_err(|_| ResponseError::HeaderValue {
            name: RULES_ERROR.as_str().to_owned(),
        })?,
    );
    Ok(())
}

fn unsized_empty_body() -> Body {
    Body::from_stream(futures_util::stream::empty::<Result<Bytes, Infallible>>())
}

fn has_no_content(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
    )
}

fn rebuild_config(endpoint: &Endpoint) -> Result<ResponseConfig, ResponseError> {
    ResponseConfig::validate(ResponseConfigInput {
        status: i64::from(endpoint.status.unpack()),
        content_type: endpoint.content_type.clone(),
        headers: endpoint
            .headers
            .iter()
            .map(|pair| HeaderInput {
                name: pair.name.clone(),
                value: pair.value.clone(),
            })
            .collect(),
        body: endpoint.body.clone(),
        delay_ms: i64::from(endpoint.delay_ms),
    })
    .map_err(|source| ResponseError::Config {
        endpoint: endpoint.id.unpack().to_owned(),
        source,
    })
}

fn insert_mandatory(
    headers: &mut HeaderMap,
    endpoint: &Endpoint,
    capture: &StoredCapture,
) -> Result<(), ResponseError> {
    let request_id = capture.id.to_string();
    headers.insert(
        REQUEST_ID,
        HeaderValue::from_str(&request_id).map_err(|_| ResponseError::HeaderValue {
            name: REQUEST_ID.as_str().to_owned(),
        })?,
    );
    headers.insert(
        ENDPOINT_ID,
        HeaderValue::from_str(endpoint.id.unpack()).map_err(|_| ResponseError::HeaderValue {
            name: ENDPOINT_ID.as_str().to_owned(),
        })?,
    );
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(SANDBOX));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static(NOSNIFF));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    Ok(())
}

fn insert_cors(headers: &mut HeaderMap) {
    headers.insert(
        ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static(ANY_ORIGIN),
    );
    headers.insert(
        ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(EXPOSED_HEADERS),
    );
}

fn insert_preflight(headers: &mut HeaderMap, preflight: &Preflight) -> Result<(), ResponseError> {
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(&preflight.method).map_err(|_| ResponseError::HeaderValue {
            name: ACCESS_CONTROL_ALLOW_METHODS.as_str().to_owned(),
        })?,
    );
    if let Some(names) = &preflight.headers {
        headers.insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_str(names).map_err(|_| ResponseError::HeaderValue {
                name: ACCESS_CONTROL_ALLOW_HEADERS.as_str().to_owned(),
            })?,
        );
    }
    headers.insert(
        ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static(PREFLIGHT_MAX_AGE),
    );
    Ok(())
}

fn preflight_of(request: &ReplyRequest<'_>) -> Option<Preflight> {
    if request.method != Method::OPTIONS || !request.headers.contains_key(ORIGIN) {
        return None;
    }
    let method = request
        .headers
        .get(ACCESS_CONTROL_REQUEST_METHOD)?
        .to_str()
        .ok()?
        .trim();
    if !is_header_token(method) {
        return None;
    }
    Some(Preflight {
        method: method.to_owned(),
        headers: requested_headers(request.headers),
    })
}

fn requested_headers(headers: &HeaderMap) -> Option<String> {
    let names: Vec<&str> = headers
        .get_all(ACCESS_CONTROL_REQUEST_HEADERS)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim_matches([' ', '\t']))
        .filter(|name| is_header_token(name))
        .take(PREFLIGHT_MAX_HEADER_NAMES)
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

fn is_header_token(text: &str) -> bool {
    text.len() <= MAX_HEADER_NAME_BYTES && is_token(text)
}

fn failure(endpoint: &Endpoint, capture: &StoredCapture) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    let headers = response.headers_mut();
    if insert_mandatory(headers, endpoint, capture).is_err() {
        headers.clear();
        headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(SANDBOX));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static(NOSNIFF));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    }
    response
}
