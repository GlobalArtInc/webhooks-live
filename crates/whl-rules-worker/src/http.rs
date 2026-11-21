use std::time::{Duration, Instant};

use bytes::Bytes;
use http::header::USER_AGENT;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde_json::{Value, json};
use whl_forward::{
    ForwardError, HOP_HEADER, OutboundRequest, OutboundResponse, validate_target_url,
};
use whl_rules::{
    Failure, FailureCode, HttpCall, MAX_STEP_BODY_BYTES, MAX_STEP_HEADERS, OutboundDone, Outcome,
    bound_text,
};
use whl_template::{NameValue, StepData};

use crate::ids::delivery_header;
use crate::outbound::{Ctx, StepAttempt, charge, count, failed};

pub const DELIVERY_HEADER: &str = "x-webhooks-live-delivery";

const PURPOSE: &str = "rule";
const SEND_GRACE: Duration = Duration::from_secs(1);
const LOGGED_BODY_BYTES: usize = 512;
const LOGGED_HEADERS: usize = 8;
const RESERVED_HEADERS: [&str; 5] = [
    HOP_HEADER,
    DELIVERY_HEADER,
    "user-agent",
    "host",
    "content-length",
];

pub(crate) fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

pub(crate) fn is_retryable_error(error: &ForwardError) -> bool {
    matches!(
        error,
        ForwardError::DnsFailure(_)
            | ForwardError::ConnectTimeout
            | ForwardError::HopTimeout
            | ForwardError::TotalTimeout
            | ForwardError::Transport(_)
    )
}

fn invalid(detail: &str) -> Failure {
    Failure::new(FailureCode::Http, detail)
}

fn build(ctx: &Ctx<'_>, call: &HttpCall, position: usize) -> Result<OutboundRequest, Failure> {
    let method = Method::from_bytes(call.method.as_bytes())
        .map_err(|_| invalid("the method is not a valid HTTP method"))?;
    if method == Method::CONNECT || method == Method::TRACE {
        return Err(invalid("the method is not allowed"));
    }
    let target = validate_target_url(&call.url, ctx.client.config())
        .map_err(|error| Failure::new(FailureCode::Http, ctx.scrub.text(&describe(&error))))?;
    let mut headers = HeaderMap::new();
    for (name, value) in &call.headers {
        let lowered = name.to_ascii_lowercase();
        if RESERVED_HEADERS.contains(&lowered.as_str()) {
            continue;
        }
        let name = HeaderName::from_bytes(lowered.as_bytes())
            .map_err(|_| invalid("a header name is not valid"))?;
        let mut value =
            HeaderValue::from_str(value).map_err(|_| invalid("a header value is not valid"))?;
        value.set_sensitive(true);
        headers.append(name, value);
    }
    headers.insert(
        HeaderName::from_static(HOP_HEADER),
        HeaderValue::from(ctx.hop.saturating_add(1)),
    );
    let delivery = HeaderValue::from_str(&delivery_header(ctx.run.id, position))
        .map_err(|_| invalid("the delivery header cannot be built"))?;
    headers.insert(HeaderName::from_static(DELIVERY_HEADER), delivery);
    let agent = HeaderValue::from_str(&ctx.client.config().user_agent)
        .map_err(|_| invalid("the user agent cannot be built"))?;
    headers.insert(USER_AGENT, agent);
    let body = call
        .body
        .as_ref()
        .map_or_else(Bytes::new, |text| Bytes::from(text.clone().into_bytes()));
    Ok(OutboundRequest::new(method, target, headers, body))
}

pub(crate) fn describe(error: &ForwardError) -> String {
    format!("{}: {error}", error.code())
}

fn kept_headers(ctx: &Ctx<'_>, response: &OutboundResponse) -> Vec<NameValue> {
    response
        .headers
        .iter()
        .take(MAX_STEP_HEADERS)
        .map(|header| NameValue::new(header.name.as_str(), ctx.scrub.text(&header.value)))
        .collect()
}

fn done(ctx: &Ctx<'_>, response: &OutboundResponse, attempt: u32) -> OutboundDone {
    let status = response.status.as_u16();
    let text = String::from_utf8_lossy(&response.body);
    let redacted = ctx.scrub.text(&text);
    let body = bound_text(&redacted, MAX_STEP_BODY_BYTES);
    let headers = kept_headers(ctx, response);
    let host = response
        .final_url
        .unpack()
        .host_str()
        .map(|host| ctx.scrub.text(host));
    let logged_headers: Vec<Value> = headers
        .iter()
        .take(LOGGED_HEADERS)
        .map(|header| json!({"name": header.name, "value": header.value}))
        .collect();
    let output = json!({
        "status": status,
        "host": host,
        "attempts": attempt,
        "duration_ms": u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX),
        "truncated": response.truncated,
        "headers": logged_headers,
        "body": bound_text(&body, LOGGED_BODY_BYTES),
    });
    let detail = (!(200..300).contains(&status))
        .then(|| format!("the target answered with status {status}"));
    OutboundDone {
        outcome: Outcome::Ok,
        detail,
        output,
        data: Some(StepData {
            status: Some(status),
            headers,
            body,
            duration_ms: 0,
            error: None,
        }),
    }
}

pub(crate) async fn execute(
    ctx: &Ctx<'_>,
    call: &HttpCall,
    position: usize,
    attempt: u32,
) -> StepAttempt {
    let request = match build(ctx, call, position) {
        Ok(request) => request,
        Err(failure) => {
            count(PURPOSE, "blocked");
            return StepAttempt::Failed(failure);
        }
    };
    if let Err(early) = charge(ctx).await {
        return early;
    }
    let limit = Duration::from_millis(u64::from(call.timeout_ms))
        .min(ctx.client.config().total_timeout)
        .saturating_add(SEND_GRACE);
    let started = Instant::now();
    let sent = tokio::time::timeout(limit, ctx.client.send(request))
        .await
        .unwrap_or(Err(ForwardError::TotalTimeout));
    let policy = ctx.settings.retry;
    match sent {
        Ok(response) => {
            let status = response.status.as_u16();
            if is_retryable_status(status) && policy.has_attempts_left(attempt) {
                count(PURPOSE, "retry");
                return StepAttempt::Retry {
                    after: policy.delay_after(attempt),
                };
            }
            count(PURPOSE, "sent");
            StepAttempt::Done(Box::new(done(ctx, &response, attempt)))
        }
        Err(error) => {
            if is_retryable_error(&error) && policy.has_attempts_left(attempt) {
                count(PURPOSE, "retry");
                return StepAttempt::Retry {
                    after: policy.delay_after(attempt),
                };
            }
            count(
                PURPOSE,
                if error.refusal().is_some() {
                    "blocked"
                } else {
                    "failed"
                },
            );
            let elapsed = started.elapsed().as_millis();
            failed(
                ctx.scrub,
                FailureCode::Http,
                &format!("{} (attempt {attempt}, {elapsed} ms)", describe(&error)),
            )
        }
    }
}
