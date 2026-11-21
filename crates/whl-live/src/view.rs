use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use whl_store::stream::{
    EndpointView, ForwardAttemptView, ForwardJobView, ForwardRuleView, HeaderView,
};

use crate::event::JsonText;

#[derive(Serialize)]
struct BodyData {
    encoding: &'static str,
    data: String,
    truncated: bool,
}

impl BodyData {
    fn from_bytes(bytes: &[u8], truncated: bool) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self {
                encoding: "utf8",
                data: text.to_owned(),
                truncated,
            },
            Err(_) => Self {
                encoding: "base64",
                data: STANDARD.encode(bytes),
                truncated,
            },
        }
    }
}

#[derive(Serialize)]
struct ResponseData<'a> {
    status: i16,
    templated: bool,
    status_template: Option<&'a str>,
    content_type: &'a str,
    headers: &'a [HeaderView],
    body: BodyData,
    delay_ms: i32,
}

#[derive(Serialize)]
struct RuleData<'a> {
    id: &'a str,
    enabled: bool,
    target_url: &'a str,
    method: Option<&'a str>,
    content_type: Option<&'a str>,
    header_allowlist: &'a [String],
    append_path: bool,
    created_at: &'a str,
}

#[derive(Serialize)]
struct EndpointData<'a> {
    id: &'a str,
    url: String,
    title: Option<&'a str>,
    response: ResponseData<'a>,
    cors: bool,
    overflow: &'a str,
    forward_rules: Vec<RuleData<'a>>,
    max_captures: u32,
    capture_count: i32,
    bytes_used: i64,
    last_seq: i64,
    created_at: &'a str,
    last_active_at: &'a str,
    expires_at: Option<&'a str>,
    retention: RetentionData,
    capture_ttl_seconds: Option<i32>,
}

#[derive(Serialize)]
struct RetentionData {
    mode: &'static str,
    seconds: Option<i32>,
}

#[derive(Serialize)]
struct AttemptData<'a> {
    attempt: i16,
    started_at: &'a str,
    duration_ms: i32,
    status: Option<i16>,
    error: Option<&'a str>,
    final_url: Option<&'a str>,
    response_content_type: Option<&'a str>,
    response_body: Option<BodyData>,
}

#[derive(Serialize)]
struct JobData<'a> {
    id: &'a str,
    kind: &'a str,
    rule_id: Option<&'a str>,
    target_url: &'a str,
    method: &'a str,
    state: &'a str,
    attempts: Vec<AttemptData<'a>>,
    created_at: &'a str,
    finished_at: Option<&'a str>,
}

#[derive(Serialize)]
struct ForwardResultData<'a> {
    capture_id: &'a str,
    job: JobData<'a>,
}

fn rule_data(rule: &ForwardRuleView) -> RuleData<'_> {
    RuleData {
        id: &rule.id,
        enabled: rule.enabled,
        target_url: &rule.target_url,
        method: rule.method.as_deref(),
        content_type: rule.content_type.as_deref(),
        header_allowlist: &rule.header_allowlist,
        append_path: rule.append_path,
        created_at: &rule.created_at,
    }
}

fn attempt_data(attempt: &ForwardAttemptView) -> AttemptData<'_> {
    AttemptData {
        attempt: attempt.attempt,
        started_at: &attempt.started_at,
        duration_ms: attempt.duration_ms,
        status: attempt.status,
        error: attempt.error.as_deref(),
        final_url: attempt.final_url.as_deref(),
        response_content_type: attempt.response_content_type.as_deref(),
        response_body: attempt
            .response_body
            .as_deref()
            .map(|body| BodyData::from_bytes(body, attempt.response_truncated)),
    }
}

pub fn endpoint_json(
    view: &EndpointView,
    public_url: &str,
    max_captures: u32,
) -> Result<JsonText, serde_json::Error> {
    let data = EndpointData {
        id: &view.id,
        url: format!("{}/{}", public_url.trim_end_matches('/'), view.id),
        title: view.title.as_deref(),
        response: ResponseData {
            status: view.response_status,
            templated: view.response_templated,
            status_template: view.response_status_template.as_deref(),
            content_type: &view.response_content_type,
            headers: &view.response_headers,
            body: BodyData::from_bytes(&view.response_body, false),
            delay_ms: view.response_delay_ms,
        },
        cors: view.cors,
        overflow: &view.overflow,
        forward_rules: view.forward_rules.iter().map(rule_data).collect(),
        max_captures: view
            .max_captures
            .and_then(|own| u32::try_from(own).ok())
            .unwrap_or(max_captures),
        capture_count: view.capture_count,
        bytes_used: view.bytes_used,
        last_seq: view.last_seq,
        created_at: &view.created_at,
        last_active_at: &view.last_active_at,
        expires_at: view.expires_at.as_deref(),
        retention: RetentionData {
            mode: if view.never_expires {
                "never"
            } else {
                "sliding"
            },
            seconds: view.retention_seconds,
        },
        capture_ttl_seconds: view.capture_ttl_seconds,
    };
    serde_json::to_string(&data).map(JsonText::from_serialized)
}

pub fn forward_result_json(job: &ForwardJobView) -> Result<JsonText, serde_json::Error> {
    let data = ForwardResultData {
        capture_id: &job.capture_id,
        job: JobData {
            id: &job.id,
            kind: &job.kind,
            rule_id: job.rule_id.as_deref(),
            target_url: &job.target_url,
            method: &job.method,
            state: &job.state,
            attempts: job.attempts.iter().map(attempt_data).collect(),
            created_at: &job.created_at,
            finished_at: job.finished_at.as_deref(),
        },
    };
    serde_json::to_string(&data).map(JsonText::from_serialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(body: Option<Vec<u8>>, truncated: bool) -> ForwardJobView {
        ForwardJobView {
            id: "j".to_owned(),
            capture_id: "c".to_owned(),
            rule_id: None,
            kind: "manual".to_owned(),
            target_url: "http://t".to_owned(),
            method: "POST".to_owned(),
            state: "failed".to_owned(),
            attempts: vec![ForwardAttemptView {
                attempt: 1,
                started_at: "2026-11-21T10:15:30.123Z".to_owned(),
                duration_ms: 5,
                status: Some(502),
                error: None,
                final_url: None,
                response_content_type: Some("text/plain".to_owned()),
                response_body: body,
                response_truncated: truncated,
            }],
            created_at: "2026-11-21T10:15:30.000Z".to_owned(),
            finished_at: None,
        }
    }

    fn parsed(text: JsonText) -> serde_json::Value {
        serde_json::from_str(text.unpack()).expect("the view is JSON")
    }

    #[test]
    fn maps_a_forward_job_with_utf8_and_binary_response_heads() {
        let utf8 = parsed(forward_result_json(&job(Some(b"hi".to_vec()), false)).expect("encodes"));
        assert_eq!(utf8["capture_id"], "c");
        assert_eq!(utf8["job"]["state"], "failed");
        assert_eq!(
            utf8["job"]["attempts"][0]["response_body"],
            serde_json::json!({"encoding": "utf8", "data": "hi", "truncated": false})
        );

        let binary =
            parsed(forward_result_json(&job(Some(vec![0xff, 0xfe]), false)).expect("encodes"));
        assert_eq!(
            binary["job"]["attempts"][0]["response_body"]["encoding"],
            "base64"
        );

        let none = parsed(forward_result_json(&job(None, false)).expect("encodes"));
        assert_eq!(
            none["job"]["attempts"][0]["response_body"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn carries_the_truncated_flag_and_content_type_of_the_response() {
        let cut = parsed(forward_result_json(&job(Some(vec![b'x'; 8]), true)).expect("encodes"));
        let attempt = &cut["job"]["attempts"][0];
        assert_eq!(attempt["response_body"]["truncated"], true);
        assert_eq!(attempt["response_content_type"], "text/plain");
    }
}
