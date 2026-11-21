use axum::body::Body;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use whl_core::status::HttpStatus;
use whl_store::captures::{Endpoint, StoredCapture};

pub fn build_response(
    endpoint: &Endpoint,
    capture: &StoredCapture,
    status_override: Option<HttpStatus>,
) -> Response {
    let status = status_override.unwrap_or(endpoint.status);
    let Ok(code) = StatusCode::from_u16(status.unpack()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let (Ok(request_id), Ok(endpoint_id)) = (
        HeaderValue::from_str(&capture.id.to_string()),
        HeaderValue::from_str(endpoint.id.unpack()),
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = code;
    let headers = response.headers_mut();
    headers.insert("x-request-id", request_id);
    headers.insert("x-endpoint-id", endpoint_id);
    response
}
