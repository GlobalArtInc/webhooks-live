use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Map, Value};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::{AnnotateOutcome, CaptureAnnotation};

use super::doc::CaptureIdParam;
use super::dto::{CaptureInputDoc, SummaryDto};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
    ValidationFailed,
};
use crate::state::ApiState;

const NOTE_MAX_CHARS: usize = 2000;
const ANNOTATION_KEYS: &[&str] = &["note", "starred"];

#[utoipa::path(
    patch,
    path = "/endpoints/{id}/captures/{capture_id}",
    operation_id = "captures_update",
    tag = "Captures",
    summary = "Set the note or the star of a capture",
    description = "Absent fields are unchanged. `note` null or an empty string removes the note; more than 2000 characters answers 422 `too_long`. `starred` must be a boolean. Unknown fields answer 422 `not_allowed`. Session only: a Bearer token is refused with 403 `session_required`. Needs an editor on the endpoint.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        CaptureIdParam,
    ),
    request_body(content = CaptureInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The updated summary", body = SummaryDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: the role is below editor; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`capture_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn update(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<SummaryDto>, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let capture = CaptureId::parse(&raw_capture).map_err(|_| ApiError::NotFound)?;
    let annotation = parse_annotation(&body)?;
    match state
        .store
        .annotate_capture(&endpoint, &capture, &annotation, state.settings.retention)
        .await?
    {
        AnnotateOutcome::Updated(summary) => Ok(Json(SummaryDto::from(summary.as_ref()))),
        AnnotateOutcome::CaptureNotFound => Err(ApiError::CaptureNotFound),
        AnnotateOutcome::EndpointGone => Err(ApiError::EndpointGone),
    }
}

fn parse_annotation(bytes: &[u8]) -> Result<CaptureAnnotation, ApiError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut violations = Vec::new();
    for key in object.keys() {
        if !ANNOTATION_KEYS.contains(&key.as_str()) {
            violations.push(FieldViolation::new(
                key.as_str(),
                FieldCode::NotAllowed,
                "is not a known field",
            ));
        }
    }
    let note = note_of(&object, &mut violations);
    let starred = starred_of(&object, &mut violations);
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    Ok(CaptureAnnotation { note, starred })
}

fn note_of(
    object: &Map<String, Value>,
    violations: &mut Vec<FieldViolation>,
) -> Option<Option<String>> {
    match object.get("note")? {
        Value::Null => Some(None),
        Value::String(text) if text.is_empty() => Some(None),
        Value::String(text) if text.chars().count() <= NOTE_MAX_CHARS => Some(Some(text.clone())),
        Value::String(_) => {
            violations.push(FieldViolation::new(
                "note",
                FieldCode::TooLong,
                "must be at most 2000 characters",
            ));
            None
        }
        _ => {
            violations.push(FieldViolation::new(
                "note",
                FieldCode::InvalidFormat,
                "must be a string or null",
            ));
            None
        }
    }
}

fn starred_of(object: &Map<String, Value>, violations: &mut Vec<FieldViolation>) -> Option<bool> {
    match object.get("starred")? {
        Value::Bool(starred) => Some(*starred),
        _ => {
            violations.push(FieldViolation::new(
                "starred",
                FieldCode::InvalidFormat,
                "must be a boolean",
            ));
            None
        }
    }
}
