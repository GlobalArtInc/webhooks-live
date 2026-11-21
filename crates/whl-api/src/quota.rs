use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;
use time::OffsetDateTime;
use whl_auth::{AuthRejection, Caller};
use whl_core::tier::Tier;
use whl_store::composer::{QuotaCharge, QuotaOutcome, QuotaWindow};
use whl_store::quota::OwnerUsage;
use whl_telemetry::series;

use crate::dto::timestamp;
use crate::error::ApiError;
use crate::state::ApiState;

#[derive(Serialize)]
pub struct UsageDto {
    tier: String,
    items: Vec<UsageItemDto>,
}

#[derive(Serialize)]
struct UsageItemDto {
    name: &'static str,
    window: &'static str,
    used: u64,
    max: u64,
    resets_at: Option<String>,
}

pub fn routes() -> Router<ApiState> {
    Router::new().route("/usage", get(usage))
}

pub(crate) async fn charge(
    state: &ApiState,
    tier: Tier<'_>,
    metric: &str,
    charges: &[QuotaCharge],
) -> Result<(), ApiError> {
    match state.store.consume_quota(metric, charges).await? {
        QuotaOutcome::Allowed => Ok(()),
        QuotaOutcome::Exceeded {
            name,
            window,
            limit,
            ..
        } => {
            metrics::counter!(
                series::QUOTA_EXCEEDED_TOTAL,
                "metric" => name,
                "tier" => tier.name().to_owned()
            )
            .increment(1);
            Err(ApiError::QuotaExceeded {
                name,
                window: window.as_str(),
                max: u64::from(limit),
                resets_at: window.resets_at(OffsetDateTime::now_utc()),
            })
        }
    }
}

async fn usage(State(state): State<ApiState>, caller: Caller) -> Result<Json<UsageDto>, ApiError> {
    let user = caller.user().ok_or(AuthRejection::Unauthenticated)?;
    if caller
        .token()
        .is_some_and(|grant| grant.endpoint_limit().is_some())
    {
        return Err(AuthRejection::InsufficientScope.into());
    }
    let tier = state.tier_of(&caller);
    let used = state
        .store
        .owner_usage(user.id, state.settings.retention)
        .await?;
    Ok(Json(UsageDto {
        tier: tier.name().to_owned(),
        items: items(tier, &used, OffsetDateTime::now_utc()),
    }))
}

fn items(tier: Tier<'_>, used: &OwnerUsage, now: OffsetDateTime) -> Vec<UsageItemDto> {
    let limits = tier.limits();
    let total = |name: &'static str, used: i64, max: u64| UsageItemDto {
        name,
        window: "total",
        used: u64::try_from(used).unwrap_or(0),
        max,
        resets_at: None,
    };
    let hourly = |name: &'static str, used: i64, max: u64| UsageItemDto {
        name,
        window: QuotaWindow::Hour.as_str(),
        used: u64::try_from(used).unwrap_or(0),
        max,
        resets_at: Some(timestamp(QuotaWindow::Hour.resets_at(now))),
    };
    let mut items = vec![
        total("endpoints", used.endpoints, u64::from(limits.endpoints)),
        total(
            "never_expire",
            used.never_expire,
            u64::from(limits.never_expire_endpoints),
        ),
        total("schedules", used.schedules, u64::from(limits.schedules)),
        total("tokens", used.tokens, u64::from(limits.tokens)),
    ];
    if let Some(max) = limits.storage_bytes {
        items.push(total("storage_bytes", used.storage_bytes, max));
    }
    if let Some(max) = limits.outbound_per_user_hour {
        items.push(hourly("outbound", used.outbound_hour, u64::from(max)));
    }
    if let Some(max) = limits.notify_per_user_hour {
        items.push(hourly(
            "notifications",
            used.notifications_hour,
            u64::from(max),
        ));
    }
    items.push(hourly(
        "exports",
        used.exports_hour,
        u64::from(limits.exports_per_hour),
    ));
    items
}
