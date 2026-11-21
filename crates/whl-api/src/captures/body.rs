use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;
use serde::Deserialize;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::filename::safe_filename;
use whl_store::EndpointError;

use crate::error::ApiError;
use crate::state::ApiState;

const CROSS_ORIGIN_RESOURCE_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-resource-policy");
const OCTET_STREAM: &str = "application/octet-stream";
pub(super) const PLAIN_TEXT: &str = "text/plain; charset=utf-8";
const SANDBOX_NO_SOURCES: &str = "sandbox; default-src 'none'";
const NOSNIFF: &str = "nosniff";
const SAME_ORIGIN: &str = "same-origin";
const NO_STORE: &str = "no-store";
const INLINE: &str = "inline";
const FILE_EXTENSION: &str = "bin";
const TEXT_VIEW: &str = "text";
const DOWNLOAD_FLAG: &str = "1";

#[derive(Debug, Deserialize)]
pub struct BodyParams {
    view: Option<String>,
    download: Option<String>,
}

impl BodyParams {
    pub(super) fn content_type(&self) -> &'static str {
        match self.view.as_deref() {
            Some(TEXT_VIEW) => PLAIN_TEXT,
            _ => OCTET_STREAM,
        }
    }

    pub(super) fn download(&self) -> bool {
        self.download.as_deref() == Some(DOWNLOAD_FLAG)
    }
}

pub async fn download(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
    Query(params): Query<BodyParams>,
) -> Result<Response, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let capture = CaptureId::parse(&raw_capture).map_err(|_| ApiError::NotFound)?;
    let data = state
        .store
        .capture_body(&endpoint, &capture)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    let disposition = disposition(&params, &capture, &endpoint)?;
    Ok(response(
        Bytes::from(data),
        params.content_type(),
        disposition,
    ))
}

fn disposition(
    params: &BodyParams,
    capture: &CaptureId,
    endpoint: &EndpointId,
) -> Result<HeaderValue, ApiError> {
    if !params.download() {
        return Ok(HeaderValue::from_static(INLINE));
    }
    let name = safe_filename(&capture.to_string(), FILE_EXTENSION);
    HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")).map_err(|error| {
        ApiError::Internal(EndpointError::Invalid {
            id: endpoint.unpack().to_owned(),
            reason: format!("the download file name cannot be sent: {error}"),
        })
    })
}

pub(super) fn response(
    data: Bytes,
    content_type: &'static str,
    disposition: HeaderValue,
) -> Response {
    let length = data.len();
    let mut response = Response::new(Body::from(data));
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
    headers.insert(CONTENT_DISPOSITION, disposition);
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(SANDBOX_NO_SOURCES),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static(NOSNIFF));
    headers.insert(
        CROSS_ORIGIN_RESOURCE_POLICY,
        HeaderValue::from_static(SAME_ORIGIN),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    response
}
