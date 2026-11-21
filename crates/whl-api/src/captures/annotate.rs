use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Map, Value};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::{AnnotateOutcome, CaptureAnnotation};

use super::dto::SummaryDto;
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const NOTE_MAX_CHARS: usize = 2000;
const ANNOTATION_KEYS: &[&str] = &["note", "starred"];

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
