use serde::Serialize;
use whl_store::stream::CaptureSummary;
use whl_store::{CaptureDetail, CaptureForm, NameValue};

use crate::dto::BodyDto;

#[derive(Serialize)]
pub struct SummaryDto {
    id: String,
    seq: i64,
    endpoint_id: String,
    received_at: String,
    method: String,
    path: String,
    query_string: String,
    client_ip: String,
    content_type: Option<String>,
    body_size: i32,
    status_override: Option<i16>,
    response_status: i16,
    forward_state: Option<&'static str>,
}

impl From<&CaptureSummary> for SummaryDto {
    fn from(summary: &CaptureSummary) -> Self {
        Self {
            id: summary.id.clone(),
            seq: summary.seq,
            endpoint_id: summary.endpoint_id.clone(),
            received_at: summary.received_at.clone(),
            method: summary.method.clone(),
            path: summary.path.clone(),
            query_string: summary.query_string.clone(),
            client_ip: summary.client_ip.clone(),
            content_type: summary.content_type.clone(),
            body_size: summary.body_size,
            status_override: summary.status_override,
            response_status: summary.response_status,
            forward_state: summary.forward_state.map(|state| state.as_str()),
        }
    }
}

#[derive(Serialize)]
pub struct ListDto {
    pub items: Vec<SummaryDto>,
    pub next_cursor: Option<String>,
    pub total: i64,
}

#[derive(Serialize)]
pub struct CaptureDto {
    #[serde(flatten)]
    summary: SummaryDto,
    url: String,
    host: String,
    headers: Vec<NameValue>,
    query: Vec<NameValue>,
    form: Option<CaptureForm>,
    body: BodyDto,
    forwards: Vec<serde_json::Value>,
}

impl From<CaptureDetail> for CaptureDto {
    fn from(detail: CaptureDetail) -> Self {
        Self {
            summary: SummaryDto::from(&detail.summary),
            url: detail.url,
            host: detail.host,
            headers: detail.headers,
            query: detail.query,
            form: detail.form,
            body: BodyDto::from_prefix(&detail.body.prefix, detail.body.truncated),
            forwards: Vec::new(),
        }
    }
}
