use serde::Serialize;
use whl_store::forward::{ForwardAttempt, ForwardJob, ForwardRule};

use crate::dto::BodyDto;

#[derive(Serialize)]
pub struct RuleDto {
    id: String,
    enabled: bool,
    target_url: String,
    method: Option<String>,
    content_type: Option<String>,
    header_allowlist: Vec<String>,
    append_path: bool,
    created_at: String,
}

impl From<&ForwardRule> for RuleDto {
    fn from(rule: &ForwardRule) -> Self {
        Self {
            id: rule.id.hyphenated().to_string(),
            enabled: rule.enabled,
            target_url: rule.target_url.clone(),
            method: rule.method.clone(),
            content_type: rule.content_type.clone(),
            header_allowlist: rule.header_allowlist.clone(),
            append_path: rule.append_path,
            created_at: rule.created_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct RuleListDto {
    pub items: Vec<RuleDto>,
}

#[derive(Serialize)]
pub struct AttemptDto {
    attempt: i16,
    started_at: String,
    duration_ms: i32,
    status: Option<i16>,
    error: Option<String>,
    final_url: Option<String>,
    response_content_type: Option<String>,
    response_body: Option<BodyDto>,
}

impl From<&ForwardAttempt> for AttemptDto {
    fn from(attempt: &ForwardAttempt) -> Self {
        Self {
            attempt: attempt.attempt,
            started_at: attempt.started_at.clone(),
            duration_ms: attempt.duration_ms,
            status: attempt.status,
            error: attempt.error.clone(),
            final_url: attempt.final_url.clone(),
            response_content_type: attempt.response_content_type.clone(),
            response_body: attempt
                .response_body
                .as_deref()
                .map(|bytes| BodyDto::from_prefix(bytes, attempt.response_truncated)),
        }
    }
}

#[derive(Serialize)]
pub struct JobDto {
    id: String,
    kind: &'static str,
    rule_id: Option<String>,
    target_url: String,
    method: String,
    state: &'static str,
    attempts: Vec<AttemptDto>,
    created_at: String,
    finished_at: Option<String>,
}

impl From<&ForwardJob> for JobDto {
    fn from(job: &ForwardJob) -> Self {
        Self {
            id: job.id.hyphenated().to_string(),
            kind: job.kind.as_str(),
            rule_id: job.rule_id.map(|id| id.hyphenated().to_string()),
            target_url: job.target_url.clone(),
            method: job.method.clone(),
            state: job.state.as_str(),
            attempts: job.attempts.iter().map(AttemptDto::from).collect(),
            created_at: job.created_at.clone(),
            finished_at: job.finished_at.clone(),
        }
    }
}
