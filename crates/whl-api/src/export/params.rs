use serde::Deserialize;
use whl_core::capture_id::CaptureId;
use whl_export::ExportFormat;
use whl_store::{ExportSelection, Uuid};

use crate::captures::query::{ListParams, export_filter};
use crate::error::{ApiError, FieldCode, FieldViolation};

pub const MAX_SELECTION: usize = 500;

#[derive(Debug, Deserialize)]
pub struct ExportParams {
    format: Option<String>,
    #[serde(flatten)]
    list: ListParams,
}

impl ExportParams {
    pub fn parse(&self) -> Result<(ExportFormat, ExportSelection), ApiError> {
        let format = parse_format(self.format.as_deref())?;
        let filter = export_filter(&self.list)?;
        Ok((format, ExportSelection::Filter(filter)))
    }
}

#[derive(Deserialize)]
struct SelectionRequest {
    format: Option<String>,
    capture_ids: Option<Vec<String>>,
}

pub fn parse_selection(bytes: &[u8]) -> Result<(ExportFormat, ExportSelection), ApiError> {
    let request: SelectionRequest = serde_json::from_slice(bytes).map_err(|_| {
        ApiError::InvalidJson("the request body must be a JSON object with format and capture_ids")
    })?;
    let format = parse_format(request.format.as_deref())?;
    let ids = parse_ids(request.capture_ids)?;
    Ok((format, ExportSelection::Ids(ids)))
}

fn parse_format(text: Option<&str>) -> Result<ExportFormat, ApiError> {
    match text {
        None => Err(ApiError::Validation(vec![FieldViolation::new(
            "format",
            FieldCode::Required,
            "is required",
        )])),
        Some(text) => ExportFormat::parse(text).ok_or_else(|| {
            ApiError::Validation(vec![FieldViolation::new(
                "format",
                FieldCode::InvalidFormat,
                "must be json, ndjson, csv or zip",
            )])
        }),
    }
}

fn parse_ids(raw: Option<Vec<String>>) -> Result<Vec<Uuid>, ApiError> {
    let raw = raw.unwrap_or_default();
    let mut violations = Vec::new();
    if raw.is_empty() {
        violations.push(FieldViolation::new(
            "capture_ids",
            FieldCode::Required,
            "must name at least one capture",
        ));
    }
    if raw.len() > MAX_SELECTION {
        violations.push(FieldViolation::new(
            "capture_ids",
            FieldCode::OutOfRange,
            "must name at most 500 captures",
        ));
    }
    let mut ids = Vec::with_capacity(raw.len());
    for text in &raw {
        match CaptureId::parse(text) {
            Ok(id) => ids.push(id.unpack()),
            Err(_) => {
                violations.push(FieldViolation::new(
                    "capture_ids",
                    FieldCode::InvalidFormat,
                    "must be capture ids",
                ));
                break;
            }
        }
    }
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}
