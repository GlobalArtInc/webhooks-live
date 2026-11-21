use std::time::Duration;

use whl_core::endpoint_id::EndpointId;
use whl_core::tier::Tier;
use whl_forward::GuardedClient;
use whl_rules::{Failure, FailureCode, OutboundCall, OutboundDone, OutboundStep};
use whl_store::CaptureDetail;
use whl_store::Store;
use whl_store::composer::{OUTBOUND_METRIC, QuotaCharge, QuotaOutcome, QuotaWindow};
use whl_store::rule_exec::ClaimedRun;
use whl_telemetry::series::{OUTBOUND_REQUESTS_TOTAL, QUOTA_EXCEEDED_TOTAL};

use crate::scrub::{Opened, Scrubber};
use crate::settings::WorkerSettings;
use crate::{forward, http, notify, s3};

const QUOTA_RETRY: Duration = Duration::from_secs(30);

pub(crate) enum StepAttempt {
    Done(Box<OutboundDone>),
    Failed(Failure),
    Retry { after: Duration },
}

pub(crate) struct Ctx<'a> {
    pub store: &'a Store,
    pub client: &'a GuardedClient,
    pub settings: &'a WorkerSettings,
    pub run: &'a ClaimedRun,
    pub endpoint: &'a EndpointId,
    pub detail: &'a CaptureDetail,
    pub hop: u32,
    pub opened: &'a Opened,
    pub scrub: &'a Scrubber,
}

pub(crate) fn failed(scrub: &Scrubber, code: FailureCode, detail: &str) -> StepAttempt {
    StepAttempt::Failed(Failure::new(code, scrub.text(detail)))
}

pub(crate) fn charges(run: &ClaimedRun, tier: Tier<'_>) -> Vec<QuotaCharge> {
    let limits = tier.limits();
    if let Some(owner) = run.owner_id {
        return vec![QuotaCharge {
            subject: format!("u:{}", owner.hyphenated()),
            window: QuotaWindow::Hour,
            limit: limits.outbound_per_user_hour.unwrap_or(0),
            name: "rule_user_hour",
        }];
    }
    let mut charges = Vec::with_capacity(2);
    if let Some(network) = &run.creator_net {
        charges.push(QuotaCharge {
            subject: format!("n:{network}"),
            window: QuotaWindow::Day,
            limit: limits.outbound_per_network_day.unwrap_or(0),
            name: "rule_network_day",
        });
    }
    charges.push(QuotaCharge {
        subject: format!("e:{}", run.endpoint_id),
        window: QuotaWindow::Hour,
        limit: limits.outbound_per_endpoint_hour.unwrap_or(0),
        name: "rule_endpoint_hour",
    });
    charges
}

pub(crate) async fn charge(ctx: &Ctx<'_>) -> Result<(), StepAttempt> {
    let tier = ctx.settings.tiers.for_caller(ctx.run.owner_tier.as_deref());
    let charges = charges(ctx.run, tier);
    match ctx.store.consume_quota(OUTBOUND_METRIC, &charges).await {
        Ok(QuotaOutcome::Allowed) => Ok(()),
        Ok(QuotaOutcome::Exceeded {
            name,
            limit,
            retry_after,
            ..
        }) => {
            metrics::counter!(
                QUOTA_EXCEEDED_TOTAL,
                "metric" => OUTBOUND_METRIC,
                "tier" => tier.name().to_owned()
            )
            .increment(1);
            Err(StepAttempt::Failed(Failure::new(
                FailureCode::Quota,
                format!(
                    "the {name} limit of {limit} outbound requests is used up; it resets in {} s",
                    retry_after.as_secs()
                ),
            )))
        }
        Err(error) => {
            tracing::warn!(%error, run_id = %ctx.run.id, "the outbound quota cannot be counted");
            Err(StepAttempt::Retry { after: QUOTA_RETRY })
        }
    }
}

pub(crate) fn count(purpose: &'static str, outcome: &'static str) {
    metrics::counter!(OUTBOUND_REQUESTS_TOTAL, "purpose" => purpose, "outcome" => outcome)
        .increment(1);
}

pub(crate) async fn perform(
    ctx: &Ctx<'_>,
    step: &OutboundStep,
    position: usize,
    attempt: u32,
) -> StepAttempt {
    match &step.call {
        OutboundCall::Http(call) => http::execute(ctx, call, position, attempt).await,
        OutboundCall::Forward(call) => forward::execute(ctx, call, position).await,
        OutboundCall::Notify(call) => notify::execute(ctx, call, position).await,
        OutboundCall::StoreS3(call) => s3::execute(ctx, call, position, attempt).await,
    }
}
