use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::Response;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::filename::safe_filename;
use whl_mail::message::{ParseFailure, PartFailure, PartView, decode_part_bounded};

use super::body::{BodyParams, PLAIN_TEXT, response};
use super::doc::CaptureIdParam;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::StoreUnavailable;
use crate::state::ApiState;

const OCTET_STREAM: &str = "application/octet-stream";
const FALLBACK_EXTENSION: &str = "bin";

#[utoipa::path(
    get,
    path = "/endpoints/{id}/captures/{capture_id}/parts/{index}",
    operation_id = "captures_part",
    tag = "Captures",
    summary = "One decoded MIME part of an email capture",
    description = "The part at `index` of `meta.parts` (see `GET .../captures/{capture_id}`), with its transfer encoding removed. `view=text` answers `text/plain; charset=utf-8` for a text part; other parts are `application/octet-stream`, the stored type; the captured Content-Type is never used. `filename` comes from the part and is sanitised; `download=1` sends it as an attachment. An unknown or non-numeric `index` answers 404 `part_not_found`. A parser that is busy or times out answers 503. Needs a viewer on the endpoint; a token needs the `read` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
        ("index" = u64, Path, minimum = 0, description = "Position of the part in `meta.parts`"),
        ("view" = Option<String>, Query, description = "`text` for a text part as `text/plain; charset=utf-8`"),
        ("download" = Option<String>, Query, description = "`1` for an attachment; any other value is inline"),
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of a view-protected anonymous endpoint")
    ),
    responses(
        (
            status = 200,
            description = "The decoded part as stored; the media type is `application/octet-stream` or `text/plain; charset=utf-8` for a text part viewed with `view=text`",
            body = String,
            content_type = "application/octet-stream",
            headers(
                ("Content-Disposition" = String, description = "`inline`, or `attachment; filename=\"<name>\"` with `download=1`"),
                ("Content-Length" = u64, description = "Bytes in the part"),
                ("Content-Security-Policy" = String, description = "Always `sandbox; default-src 'none'`"),
                ("X-Content-Type-Options" = String, description = "Always `nosniff`"),
                ("Cross-Origin-Resource-Policy" = String, description = "Always `same-origin`"),
                ("Cache-Control" = String, description = "Always `no-store`")
            )
        ),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`part_not_found`: no such part or a bad index; `capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
