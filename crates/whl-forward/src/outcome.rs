use std::time::Duration;

use http::header::CONTENT_TYPE;
use whl_store::Uuid;
use whl_store::forward::AttemptRecord;

use crate::error::{ForwardError, RefusalReason};
use crate::message::OutboundResponse;
use crate::request::RequestError;

pub const RESPONSE_BODY_BYTES: usize = 4096;
const MAX_DETAIL_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Success,
    Retryable,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    HttpError,
    Timeout,
    ConnectError,
    Blocked,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::HttpError => "http_error",
            Self::Timeout => "timeout",
            Self::ConnectError => "connect_error",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub record: AttemptRecord,
    pub verdict: Verdict,
    pub outcome: Outcome,
}

pub fn from_response(
    job_id: Uuid,
    attempt: i16,
    elapsed: Duration,
    response: &OutboundResponse,
) -> Finished {
    let status = response.status.as_u16();
    let (verdict, outcome) = match status {
        200..=299 => (Verdict::Success, Outcome::Success),
        408 | 429 | 500..=599 => (Verdict::Retryable, Outcome::HttpError),
        _ => (Verdict::Final, Outcome::HttpError),
    };
    let content_type = response
        .headers
        .iter()
        .find(|header| header.name == CONTENT_TYPE)
        .map(|header| header.value.clone());
    let kept = response.body.len().min(RESPONSE_BODY_BYTES);
    Finished {
        record: AttemptRecord {
            job_id,
            attempt,
            duration_ms: millis(elapsed),
            status: i16::try_from(status).ok(),
            error: None,
            final_url: Some(response.final_url.unpack().to_string()),
            response_content_type: content_type,
            response_body: Some(response.body[..kept].to_vec()),
            response_truncated: response.truncated || response.body.len() > RESPONSE_BODY_BYTES,
        },
        verdict,
        outcome,
    }
}

pub fn from_error(job_id: Uuid, attempt: i16, elapsed: Duration, error: &ForwardError) -> Finished {
    let (text, verdict, outcome) = classify_error(error);
    failure(job_id, attempt, elapsed, text, verdict, outcome)
}

pub fn from_request_error(
    job_id: Uuid,
    attempt: i16,
    elapsed: Duration,
    error: &RequestError,
) -> Finished {
    match error {
        RequestError::Target(inner) => from_error(job_id, attempt, elapsed, inner),
        RequestError::Method => failure(
            job_id,
            attempt,
            elapsed,
            "invalid_request: the job method is not a valid HTTP method".to_owned(),
            Verdict::Final,
            Outcome::Blocked,
        ),
    }
}

fn failure(
    job_id: Uuid,
    attempt: i16,
    elapsed: Duration,
    text: String,
    verdict: Verdict,
    outcome: Outcome,
) -> Finished {
    Finished {
        record: AttemptRecord {
            job_id,
            attempt,
            duration_ms: millis(elapsed),
            status: None,
            error: Some(text),
            final_url: None,
            response_content_type: None,
            response_body: None,
            response_truncated: false,
        },
        verdict,
        outcome,
    }
}

fn classify_error(error: &ForwardError) -> (String, Verdict, Outcome) {
    match error {
        ForwardError::TargetRefused(reason) => (
            format!("blocked_address: {}", reason.code()),
            Verdict::Final,
            Outcome::Blocked,
        ),
        ForwardError::RedirectRefused(reason) => {
            let code = match reason {
                RefusalReason::RefusedAddress
                | RefusalReason::ForbiddenHost
                | RefusalReason::ForbiddenPort => "blocked_address",
                _ => "invalid_redirect",
            };
            (
                format!("{code}: {}", reason.code()),
                Verdict::Final,
                Outcome::Blocked,
            )
        }
        ForwardError::DnsFailure(detail) => (
            format!("connect_error: dns lookup failed: {}", bounded(detail)),
            Verdict::Retryable,
            Outcome::ConnectError,
        ),
        ForwardError::ConnectTimeout | ForwardError::HopTimeout | ForwardError::TotalTimeout => (
            format!("timeout: {error}"),
            Verdict::Retryable,
            Outcome::Timeout,
        ),
        ForwardError::TooManyRedirects { limit } => (
            format!("too_many_redirects: limit {limit}"),
            Verdict::Final,
            Outcome::HttpError,
        ),
        ForwardError::Transport(detail) => {
            let code = if is_tls(detail) {
                "tls_error"
            } else {
                "connect_error"
            };
            (
                format!("{code}: {}", bounded(detail)),
                Verdict::Retryable,
                Outcome::ConnectError,
            )
        }
        ForwardError::Configuration(detail) => (
            format!("connect_error: {}", bounded(detail)),
            Verdict::Final,
            Outcome::ConnectError,
        ),
    }
}

fn is_tls(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    ["tls", "certificate", "handshake", "ssl"]
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn bounded(detail: &str) -> String {
    detail.chars().take(MAX_DETAIL_CHARS).collect()
}

fn millis(elapsed: Duration) -> i32 {
    i32::try_from(elapsed.as_millis()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(error: &ForwardError) -> Finished {
        from_error(Uuid::nil(), 1, Duration::from_millis(5), error)
    }

    #[test]
    fn refusals_are_final_and_carry_the_refusal_code() {
        let result = finished(&ForwardError::TargetRefused(RefusalReason::RefusedAddress));
        assert_eq!(result.verdict, Verdict::Final);
        assert_eq!(result.outcome, Outcome::Blocked);
        assert_eq!(
            result.record.error.as_deref(),
            Some("blocked_address: refused_address")
        );
        assert_eq!(result.record.status, None);
        assert_eq!(result.record.response_body, None);
    }

    #[test]
    fn redirect_refusals_use_the_documented_codes() {
        let downgrade = finished(&ForwardError::RedirectRefused(
            RefusalReason::HttpsDowngrade,
        ));
        assert_eq!(
            downgrade.record.error.as_deref(),
            Some("invalid_redirect: https_downgrade")
        );
        let address = finished(&ForwardError::RedirectRefused(
            RefusalReason::RefusedAddress,
        ));
        assert_eq!(
            address.record.error.as_deref(),
            Some("blocked_address: refused_address")
        );
        let loops = finished(&ForwardError::TooManyRedirects { limit: 5 });
        assert_eq!(loops.verdict, Verdict::Final);
    }

    #[test]
    fn transient_transport_failures_are_retryable() {
        for error in [
            ForwardError::ConnectTimeout,
            ForwardError::HopTimeout,
            ForwardError::TotalTimeout,
            ForwardError::DnsFailure("nope".to_owned()),
            ForwardError::Transport("connection refused".to_owned()),
        ] {
            assert_eq!(finished(&error).verdict, Verdict::Retryable, "{error}");
        }
        let tls = finished(&ForwardError::Transport(
            "invalid peer certificate".to_owned(),
        ));
        assert!(tls.record.error.unwrap().starts_with("tls_error: "));
    }
}
