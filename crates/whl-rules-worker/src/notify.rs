use std::time::Duration;

use serde_json::json;
use whl_rules::{FailureCode, NotifyCall, OutboundDone, Outcome};
use whl_store::rule_exec::{EnqueueOutcome, StepDelivery};

use crate::ids::step_id;
use crate::outbound::{Ctx, StepAttempt, count, failed};

const PURPOSE: &str = "notify";
const STORE_RETRY: Duration = Duration::from_secs(5);

pub(crate) async fn execute(ctx: &Ctx<'_>, call: &NotifyCall, position: usize) -> StepAttempt {
    if ctx.hop >= 1 {
        return failed(
            ctx.scrub,
            FailureCode::Relayed,
            "a relayed capture never notifies",
        );
    }
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
            StepAttempt::Done(Box::new(OutboundDone {
                outcome: Outcome::Ok,
                detail: None,
                output: json!({"delivery_id": delivery.id.hyphenated().to_string()}),
                data: None,
            }))
        }
        Ok(EnqueueOutcome::ChannelNotFound) => failed(
            ctx.scrub,
            FailureCode::Http,
            "the channel does not belong to this endpoint",
        ),
        Ok(EnqueueOutcome::ChannelDisabled) => {
            failed(ctx.scrub, FailureCode::Http, "the channel is disabled")
        }
        Ok(EnqueueOutcome::AlreadyDelivered) => {
            count(PURPOSE, "skipped");
            StepAttempt::Done(Box::new(OutboundDone {
                outcome: Outcome::Skipped,
                detail: Some(
                    "a delivery for this channel and capture already exists, nothing was sent"
                        .to_owned(),
                ),
                output: json!({"delivery_id": null}),
                data: None,
            }))
        }
        Ok(EnqueueOutcome::DeliveryExists) => {
            tracing::error!(run_id = %ctx.run.id, "a delivery conflict has no delivery to blame");
            failed(
                ctx.scrub,
                FailureCode::Internal,
                "delivery_exists: the delivery conflicts with no stored delivery",
            )
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
