use std::time::Duration;

use http::Method;
use serde_json::json;
use whl_forward::validate_target_url;
use whl_rules::{BodySource, FailureCode, ForwardCall, OutboundDone};
use whl_store::rule_exec::{EnqueueOutcome, ForwardStepJob};

use crate::ids::step_id;
use crate::outbound::{Ctx, StepAttempt, charge, count, failed};

const PURPOSE: &str = "forward";
const STORE_RETRY: Duration = Duration::from_secs(5);
const MAX_FORWARD_PAYLOAD_BYTES: usize = 16 * 1024;

pub(crate) async fn execute(ctx: &Ctx<'_>, call: &ForwardCall, position: usize) -> StepAttempt {
    if let Some(target) = &call.target_url
        && let Err(error) = validate_target_url(target, ctx.client.config())
    {
        count(PURPOSE, "blocked");
        return failed(ctx.scrub, FailureCode::Http, &crate::http::describe(&error));
    }
    let method = match &call.method {
        Some(text) => match Method::from_bytes(text.as_bytes()) {
            Ok(method) => Some(method.as_str().to_owned()),
            Err(_) => {
                return failed(
                    ctx.scrub,
                    FailureCode::Http,
                    "the method is not a valid HTTP method",
                );
            }
        },
        None => None,
    };
    let payload = match call.body_from {
        BodySource::Payload => call.payload.as_ref().map(|text| text.clone().into_bytes()),
        BodySource::Capture => None,
    };
    if payload
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_FORWARD_PAYLOAD_BYTES)
    {
        return failed(
            ctx.scrub,
            FailureCode::Http,
            "the payload is larger than a forward job can carry",
        );
    }
    if let Err(early) = charge(ctx).await {
        return early;
    }
    let job = ForwardStepJob {
        id: step_id(ctx.run.id, position),
        endpoint: ctx.endpoint.clone(),
        capture: ctx.run.capture_id,
        rule: call.forward_rule_id,
        target_url: call.target_url.clone(),
        method,
        content_type: call.content_type.clone(),
        header_allowlist: call.header_allowlist.clone(),
        payload,
    };
    match ctx.store.enqueue_step_forward(&job).await {
        Ok(EnqueueOutcome::Queued) => {
            count(PURPOSE, "queued");
            StepAttempt::Done(Box::new(OutboundDone {
                detail: None,
                output: json!({"job_id": job.id.hyphenated().to_string()}),
                data: None,
            }))
        }
        Ok(EnqueueOutcome::RuleNotFound) => failed(
            ctx.scrub,
            FailureCode::Http,
            "the forward rule does not exist on this endpoint",
        ),
        Ok(_) => failed(
            ctx.scrub,
            FailureCode::Internal,
            "the capture no longer exists",
        ),
        Err(error) if error.is_unavailable() => StepAttempt::Retry { after: STORE_RETRY },
        Err(error) => {
            tracing::warn!(%error, run_id = %ctx.run.id, "a forward job cannot be stored");
            failed(
                ctx.scrub,
                FailureCode::Internal,
                "the forward job cannot be stored",
            )
        }
    }
}
