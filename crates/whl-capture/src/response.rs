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
use whl_core::header::{HeaderInput, MAX_HEADER_NAME_BYTES, MAX_HEADERS, is_token};
use whl_core::response::{ResponseConfig, ResponseConfigInput, compose_response};
use whl_core::validation::ValidationError;
use whl_store::captures::{Endpoint, StoredCapture};

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const ENDPOINT_ID: HeaderName = HeaderName::from_static("x-endpoint-id");
const SUBSTITUTED_STATUS: HeaderName =
    HeaderName::from_static("x-webhooks-live-substituted-status");

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
    match compose(endpoint, capture, status_override, request) {
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
) -> Result<Response, ResponseError> {
    let preflight = if endpoint.cors {
        preflight_of(request)
    } else {
        None
    };
    let mut headers = HeaderMap::new();
    let (status, body) = match &preflight {
        Some(_) => (StatusCode::NO_CONTENT, Bytes::new()),
        None => configured(endpoint, status_override, request, &mut headers)?,
    };
    insert_mandatory(&mut headers, endpoint, capture)?;
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
) -> Result<(StatusCode, Bytes), ResponseError> {
    let config = rebuild_config(endpoint)?;
    let composed = compose_response(&config, status_override);
    let requested = composed.status.unpack();
    let requested_code =
        StatusCode::from_u16(requested).map_err(|_| ResponseError::Status(requested))?;

    for header in composed.headers {
        let name = header.name.unpack();
        if name.to_ascii_lowercase().starts_with(RESERVED_PREFIX) {
            continue;
        }
        let parsed_name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| ResponseError::HeaderName {
                name: name.to_owned(),
            })?;
        let value = HeaderValue::from_str(header.value.unpack()).map_err(|_| {
            ResponseError::HeaderValue {
                name: name.to_owned(),
            }
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

    let payload = Bytes::copy_from_slice(config.body());
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(composed.content_type.unpack()).map_err(|_| {
            ResponseError::HeaderValue {
                name: CONTENT_TYPE.as_str().to_owned(),
            }
        })?,
    );
    headers.insert(CONTENT_LENGTH, HeaderValue::from(payload.len()));
    if request.method == Method::HEAD {
        return Ok((status, Bytes::new()));
    }
    Ok((status, payload))
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
