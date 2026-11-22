use serde::Serialize;
use utoipa::ToSchema;
use whl_store::forward::{ForwardAttempt, ForwardJob, ForwardRule};

use crate::dto::BodyDto;

#[derive(Serialize, ToSchema)]
#[schema(as = ForwardRule)]
pub struct RuleDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = true)]
    enabled: bool,
    #[schema(example = "https://hooks.example.com/incoming")]
    target_url: String,
    #[schema(example = "PUT")]
    method: Option<String>,
    #[schema(example = "application/json")]
    content_type: Option<String>,
    #[schema(example = json!(["x-signature"]))]
    header_allowlist: Vec<String>,
    #[schema(example = false)]
    append_path: bool,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = ForwardRuleList)]
pub struct RuleListDto {
    pub items: Vec<RuleDto>,
}

#[derive(ToSchema)]
#[schema(as = ForwardRuleInput)]
#[allow(dead_code)]
pub struct ForwardRuleInputDoc {
    #[schema(default = true, example = true)]
    enabled: Option<bool>,
    #[schema(max_length = 2048, example = "https://hooks.example.com/incoming")]
    target_url: Option<String>,
    #[schema(max_length = 16, example = "PUT")]
    method: Option<String>,
    #[schema(example = "application/json")]
    content_type: Option<String>,
    #[schema(max_items = 50, example = json!(["x-signature"]))]
    header_allowlist: Option<Vec<String>>,
    #[schema(default = false, example = false)]
    append_path: Option<bool>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = ForwardAttempt)]
pub struct AttemptDto {
    attempt: i16,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = ForwardJob)]
pub struct JobDto {
    id: String,
    #[schema(pattern = "^(rule|manual)$")]
    kind: &'static str,
    rule_id: Option<String>,
    target_url: String,
    method: String,
    #[schema(pattern = "^(pending|running|succeeded|failed)$")]
    state: &'static str,
    attempts: Vec<AttemptDto>,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
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
