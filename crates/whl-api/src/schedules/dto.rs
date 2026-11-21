use serde::Serialize;
use whl_store::schedules::{ScheduleHeader, ScheduleRecord, ScheduleRequest, ScheduleRunRecord};

use crate::dto::timestamp;

#[derive(Serialize)]
struct HeaderDto {
    name: String,
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

#[derive(Serialize)]
struct RequestDto {
    method: String,
    url: String,
    headers: Vec<HeaderDto>,
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

#[derive(Serialize)]
pub struct ScheduleDto {
    id: String,
    name: String,
    cron: String,
    timezone: String,
    request: RequestDto,
    enabled: bool,
    paused_reason: Option<String>,
    next_run_at: Option<String>,
    last_run_at: Option<String>,
    consecutive_failures: i16,
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

#[derive(Serialize)]
pub struct ScheduleListDto {
    pub items: Vec<ScheduleDto>,
}

#[derive(Serialize)]
pub struct RunDto {
    id: String,
    started_at: String,
    duration_ms: i32,
    outcome: &'static str,
    status: Option<i16>,
    error: Option<String>,
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

#[derive(Serialize)]
pub struct RunListDto {
    pub items: Vec<RunDto>,
}

#[derive(Serialize)]
pub struct QueuedDto {
    pub schedule_id: String,
    pub queued_at: String,
}
