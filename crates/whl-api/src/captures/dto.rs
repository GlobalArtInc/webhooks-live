use serde::Serialize;
use utoipa::ToSchema;
use whl_store::forward::ForwardJob;
use whl_store::notify::Delivery;
use whl_store::rule_exec::RunRecord;
use whl_store::stream::CaptureSummary;
use whl_store::{CaptureDetail, CaptureForm, NameValue};

use crate::dto::{BodyDto, NameValueDto};
use crate::forward::JobDto;
use crate::notify::DeliveryDto;
use crate::rules::RuleRunDto;

#[derive(Serialize, ToSchema)]
#[schema(as = CaptureSummary)]
pub struct SummaryDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    seq: i64,
    #[schema(
        pattern = "^[0-9a-hjkmnp-tv-z]{24}$",
        example = "7k2m9q4xw8c1d5f3h6j0n2p4"
    )]
    endpoint_id: String,
    #[schema(format = DateTime)]
    received_at: String,
    method: String,
    path: String,
    query_string: String,
    client_ip: String,
    content_type: Option<String>,
    body_size: i32,
    status_override: Option<i16>,
    response_status: i16,
    #[schema(pattern = "^(pending|succeeded|failed)$")]
    forward_state: Option<&'static str>,
    #[schema(pattern = "^(http|email|dns|schedule)$")]
    kind: &'static str,
    starred: bool,
    has_note: bool,
    #[schema(pattern = "^(valid|invalid|missing)$")]
    verification: Option<&'static str>,
    imported: bool,
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
            kind: summary.kind.as_str(),
            starred: summary.starred,
            has_note: summary.has_note,
            verification: summary
                .verification
                .map(|verification| verification.as_str()),
            imported: summary.imported,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = CaptureList)]
pub struct ListDto {
    pub items: Vec<SummaryDto>,
    pub next_cursor: Option<String>,
    pub total: i64,
}

#[derive(Serialize, ToSchema)]
#[schema(as = Capture)]
pub struct CaptureDto {
    #[serde(flatten)]
    summary: SummaryDto,
    url: String,
    host: String,
    #[schema(value_type = Vec<NameValueDto>)]
    headers: Vec<NameValue>,
    #[schema(value_type = Vec<NameValueDto>)]
    query: Vec<NameValue>,
    #[schema(value_type = Option<CaptureFormDoc>)]
    form: Option<CaptureForm>,
    note: Option<String>,
    body: BodyDto,
    forwards: Vec<JobDto>,
    notifications: Vec<DeliveryDto>,
    rule_runs: Vec<RuleRunDto>,
    #[schema(value_type = Object)]
    meta: serde_json::Value,
}

impl CaptureDto {
    pub fn new(
        detail: CaptureDetail,
        forwards: &[ForwardJob],
        notifications: &[Delivery],
        rule_runs: &[RunRecord],
    ) -> Self {
        Self {
            summary: SummaryDto::from(&detail.summary),
            url: detail.url,
            host: detail.host,
            headers: detail.headers,
            query: detail.query,
            form: detail.form,
            note: detail.note,
            body: BodyDto::from_prefix(&detail.body.prefix, detail.body.truncated),
            forwards: forwards.iter().map(JobDto::from).collect(),
            notifications: notifications.iter().map(DeliveryDto::from).collect(),
            rule_runs: rule_runs.iter().map(RuleRunDto::from).collect(),
            meta: detail.meta,
        }
    }
}

#[derive(ToSchema)]
#[schema(as = FormFile)]
#[allow(dead_code)]
pub struct FormFileDoc {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    size: i64,
}

#[derive(ToSchema)]
#[schema(as = CaptureForm)]
#[allow(dead_code)]
pub struct CaptureFormDoc {
    fields: Vec<NameValueDto>,
    files: Vec<FormFileDoc>,
}

#[derive(ToSchema)]
#[schema(as = CaptureInput)]
#[allow(dead_code)]
pub struct CaptureInputDoc {
    #[schema(max_length = 2000)]
    note: Option<String>,
    #[schema(nullable = false)]
    starred: Option<bool>,
}

#[derive(ToSchema)]
#[schema(as = ForwardRequest)]
#[allow(dead_code)]
pub struct ForwardRequestDoc {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    rule_id: Option<String>,
    #[schema(max_length = 2048)]
    target_url: Option<String>,
    #[schema(max_length = 16)]
    method: Option<String>,
    content_type: Option<String>,
    #[schema(max_items = 50)]
    header_allowlist: Option<Vec<String>>,
}
