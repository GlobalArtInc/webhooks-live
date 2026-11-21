use std::time::Duration;

use serde_json::json;
use whl_rules::{FailureCode, NotifyCall, OutboundDone};
use whl_store::rule_exec::{EnqueueOutcome, StepDelivery};

use crate::ids::step_id;
use crate::outbound::{Ctx, StepAttempt, count, failed};

const PURPOSE: &str = "notify";
const STORE_RETRY: Duration = Duration::from_secs(5);

pub(crate) async fn execute(ctx: &Ctx<'_>, call: &NotifyCall, position: usize) -> StepAttempt {
    let delivery = StepDelivery {
        id: step_id(ctx.run.id, position),
        endpoint: ctx.endpoint.clone(),
        capture: ctx.run.capture_id,
        channel: call.channel_id,
        message: call.message.as_deref().map(|text| ctx.scrub.text(text)),
    };
    match ctx.store.enqueue_step_delivery(&delivery).await {
        Ok(EnqueueOutcome::Queued) => {
            count(PURPOSE, "queued");
            StepAttempt::Done(OutboundDone {
                detail: None,
                output: json!({"delivery_id": delivery.id.hyphenated().to_string()}),
                data: None,
            })
        }
        Ok(EnqueueOutcome::ChannelNotFound) => failed(
            ctx.scrub,
            FailureCode::Http,
            "the channel does not belong to this endpoint",
        ),
        Ok(EnqueueOutcome::ChannelDisabled) => {
            failed(ctx.scrub, FailureCode::Http, "the channel is disabled")
        }
        Ok(_) => failed(
            ctx.scrub,
            FailureCode::Internal,
            "the capture no longer exists",
        ),
        Err(error) if error.is_unavailable() => StepAttempt::Retry { after: STORE_RETRY },
        Err(error) => {
            tracing::warn!(%error, run_id = %ctx.run.id, "a notification cannot be stored");
            failed(
                ctx.scrub,
                FailureCode::Internal,
                "the notification cannot be stored",
            )
        }
    }
}
