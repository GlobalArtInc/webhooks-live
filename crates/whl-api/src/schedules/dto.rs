use serde::Serialize;
use utoipa::ToSchema;
use whl_store::schedules::{ScheduleHeader, ScheduleRecord, ScheduleRequest, ScheduleRunRecord};

use crate::dto::timestamp;

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleHeader)]
pub struct HeaderDto {
    #[schema(example = "Content-Type")]
    name: String,
    #[schema(example = "application/json")]
    value: String,
}

impl From<&ScheduleHeader> for HeaderDto {
    fn from(header: &ScheduleHeader) -> Self {
        Self {
            name: header.name.clone(),
            value: header.value.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleRequest)]
pub struct RequestDto {
    #[schema(example = "POST")]
    method: String,
    #[schema(example = "https://hooks.example.com/nightly")]
    url: String,
    headers: Vec<HeaderDto>,
    #[schema(example = "{\"ping\": true}")]
    body: Option<String>,
}

impl From<&ScheduleRequest> for RequestDto {
    fn from(request: &ScheduleRequest) -> Self {
        Self {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.iter().map(HeaderDto::from).collect(),
            body: request.body.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = Schedule)]
pub struct ScheduleDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = "Nightly ping")]
    name: String,
    #[schema(example = "0 3 * * *")]
    cron: String,
    #[schema(example = "UTC")]
    timezone: String,
    request: RequestDto,
    enabled: bool,
    #[schema(example = "3 consecutive failures")]
    paused_reason: Option<String>,
    #[schema(format = DateTime)]
    next_run_at: Option<String>,
    #[schema(format = DateTime)]
    last_run_at: Option<String>,
    #[schema(example = 0)]
    consecutive_failures: i16,
    #[schema(format = DateTime)]
    created_at: String,
}

impl From<&ScheduleRecord> for ScheduleDto {
    fn from(record: &ScheduleRecord) -> Self {
        Self {
            id: record.id.hyphenated().to_string(),
            name: record.name.clone(),
            cron: record.cron.clone(),
            timezone: record.timezone.clone(),
            request: RequestDto::from(&record.request),
            enabled: record.enabled,
            paused_reason: record.paused_reason.clone(),
            next_run_at: record.next_run_at.map(timestamp),
            last_run_at: record.last_run_at.map(timestamp),
            consecutive_failures: record.consecutive_failures,
            created_at: timestamp(record.created_at),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleList)]
pub struct ScheduleListDto {
    pub items: Vec<ScheduleDto>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleRun)]
pub struct RunDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(format = DateTime)]
    started_at: String,
    #[schema(example = 182)]
    duration_ms: i32,
    #[schema(example = "succeeded")]
    outcome: &'static str,
    #[schema(example = 200)]
    status: Option<i16>,
    #[schema(example = "connection refused")]
    error: Option<String>,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d71"
    )]
    capture_id: Option<String>,
}

impl From<&ScheduleRunRecord> for RunDto {
    fn from(run: &ScheduleRunRecord) -> Self {
        Self {
            id: run.id.hyphenated().to_string(),
            started_at: timestamp(run.started_at),
            duration_ms: run.duration_ms,
            outcome: run.outcome.as_str(),
            status: run.status,
            error: run.error.clone(),
            capture_id: run.capture_id.map(|id| id.hyphenated().to_string()),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleRunList)]
pub struct RunListDto {
    pub items: Vec<RunDto>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScheduleRunQueued)]
pub struct QueuedDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    pub schedule_id: String,
    #[schema(format = DateTime)]
    pub queued_at: String,
}
