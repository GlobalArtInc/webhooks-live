use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use whl_core::endpoint_id::EndpointId;

use crate::state::CaptureState;

const ERROR_HEADER: &str = "x-webhooks-live-error";
const ENDPOINT_HEADER: &str = "x-endpoint-id";
const STORE_RETRY_SECONDS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Malformed,
    Gone(EndpointId),
    Full(EndpointId),
    TooLarge,
    RateLimited { retry_after: Duration },
    StoreUnavailable,
    StorageFull,
    BudgetExhausted,
    ReadTimeout,
    BodyAborted,
    Internal,
}

#[derive(Debug)]
pub struct Failure {
    pub refusal: Refusal,
    pub close: bool,
}

impl Failure {
    pub fn new(refusal: Refusal, body_unread: bool) -> Self {
        Self {
            refusal,
            close: body_unread,
        }
    }
}

struct Parts {
    status: StatusCode,
    text: String,
    error: Option<&'static str>,
    endpoint: Option<EndpointId>,
    retry_after: Option<u64>,
}

impl Failure {
    pub fn into_response(self, state: &CaptureState) -> Response {
        let parts = describe(self.refusal, state);
        let mut response = Response::new(Body::from(parts.text));
        *response.status_mut() = parts.status;
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("sandbox"),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(error) = parts.error {
            headers.insert(ERROR_HEADER, HeaderValue::from_static(error));
        }
        if let Some(endpoint) = parts.endpoint
            && let Ok(value) = HeaderValue::from_str(endpoint.unpack())
        {
            headers.insert(ENDPOINT_HEADER, value);
        }
        if let Some(seconds) = parts.retry_after {
            headers.insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        if self.close {
            headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }
}

fn describe(refusal: Refusal, state: &CaptureState) -> Parts {
    let plain = |status, text: &str| Parts {
        status,
        text: text.to_owned(),
        error: None,
        endpoint: None,
        retry_after: None,
    };
    match refusal {
        Refusal::Malformed => Parts {
            error: Some("malformed_id"),
            ..plain(StatusCode::NOT_FOUND, "Not found.")
        },
        Refusal::Gone(id) => Parts {
            error: Some("endpoint_gone"),
            endpoint: Some(id),
            ..plain(
                StatusCode::GONE,
                &format!(
                    "This endpoint does not exist or has expired. Create a new one at {}.",
                    state.public_url()
                ),
            )
        },
        Refusal::Full(id) => Parts {
            error: Some("endpoint_full"),
            endpoint: Some(id),
            ..plain(
                StatusCode::GONE,
                "This endpoint holds the maximum number of captures. Delete captures to receive more.",
            )
        },
        Refusal::TooLarge => Parts {
            error: Some("too_large"),
            ..plain(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!(
                    "Request body exceeds {} bytes.",
                    state.limits().max_body_bytes
                ),
            )
        },
        Refusal::RateLimited { retry_after } => Parts {
            error: Some("rate_limited"),
            retry_after: Some(retry_after.as_secs().max(1)),
            ..plain(StatusCode::TOO_MANY_REQUESTS, "Too many requests.")
        },
        Refusal::StoreUnavailable => Parts {
            error: Some("store_unavailable"),
            retry_after: Some(STORE_RETRY_SECONDS),
            ..plain(StatusCode::SERVICE_UNAVAILABLE, "Temporarily unavailable.")
        },
        Refusal::StorageFull => Parts {
            error: Some("storage_full"),
            ..plain(
                StatusCode::INSUFFICIENT_STORAGE,
                "Storage is full. Try again later.",
            )
        },
        Refusal::BudgetExhausted => Parts {
            retry_after: Some(STORE_RETRY_SECONDS),
            ..plain(StatusCode::SERVICE_UNAVAILABLE, "Temporarily unavailable.")
        },
        Refusal::ReadTimeout => plain(StatusCode::REQUEST_TIMEOUT, "Request timed out."),
        Refusal::BodyAborted => plain(StatusCode::BAD_REQUEST, "Request body could not be read."),
        Refusal::Internal => plain(StatusCode::INTERNAL_SERVER_ERROR, "Internal error."),
    }
}
