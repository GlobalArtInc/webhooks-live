use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::Response;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::filename::safe_filename;
use whl_mail::message::{ParseFailure, PartFailure, PartView, decode_part_bounded};

use super::body::{BodyParams, PLAIN_TEXT, response};
use crate::error::ApiError;
use crate::state::ApiState;

const OCTET_STREAM: &str = "application/octet-stream";
const FALLBACK_EXTENSION: &str = "bin";

pub async fn download(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture, raw_index)): Path<(String, String, String)>,
    Query(params): Query<BodyParams>,
) -> Result<Response, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let capture = CaptureId::parse(&raw_capture).map_err(|_| ApiError::NotFound)?;
    let index: usize = raw_index.parse().map_err(|_| ApiError::PartNotFound)?;
    let raw = state
        .store
        .capture_body(&endpoint, &capture)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    let view = if params.content_type() == PLAIN_TEXT {
        PartView::Text
    } else {
        PartView::Decoded
    };
    let part = decode_part_bounded(raw, index, view)
        .await
        .map_err(part_error)?;
    let content_type = if view == PartView::Text && part.is_text {
        PLAIN_TEXT
    } else {
        OCTET_STREAM
    };
    let disposition = if params.download() {
        let name = match &part.filename {
            Some(name) => safe_filename(name, ""),
            None => safe_filename(&format!("part-{index}"), FALLBACK_EXTENSION),
        };
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .map_err(|_| ApiError::PartNotFound)?
    } else {
        HeaderValue::from_static("inline")
    };
    Ok(response(Bytes::from(part.data), content_type, disposition))
}

fn part_error(failure: PartFailure) -> ApiError {
    match failure {
        PartFailure::NotFound => ApiError::PartNotFound,
        PartFailure::Parse(ParseFailure::Busy | ParseFailure::Timeout | ParseFailure::Aborted) => {
            ApiError::StoreUnavailable
        }
        PartFailure::Parse(_) => ApiError::PartNotFound,
    }
}
