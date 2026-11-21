use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_store::{ClaimOutcome, ClaimRefusal, EndpointRole};

use crate::dto::EndpointDto;
use crate::error::ApiError;
use crate::state::ApiState;

pub const CLAIM_WINDOW: Duration = Duration::from_secs(24 * 3600);

pub async fn claim(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
) -> Result<Json<EndpointDto>, ApiError> {
    if !state.access.is_mixed() {
        return Err(ApiError::NotAvailableInMode);
    }
    let user = caller.user().ok_or(AuthRejection::SignInRequired)?;
    let id = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let network = caller.network();
    let outcome = state
        .store
        .claim_endpoint_limited(
            &id,
            user.id,
            network.as_ref(),
            state.settings.retention,
            CLAIM_WINDOW,
            Some(state.tier_of(&caller).limits().endpoints),
        )
        .await?;
    match outcome {
        ClaimOutcome::Claimed(record) => {
            let rules = state.store.list_forward_rules(&id).await?;
            Ok(Json(
                EndpointDto::from_record(&record, &state.settings, &rules)
                    .with_identity(Some(EndpointRole::Owner), Some(user)),
            ))
        }
        ClaimOutcome::EndpointLimit { max } => Err(ApiError::EndpointLimit { max }),
        ClaimOutcome::AlreadyOwned => Err(ApiError::AlreadyOwned),
        ClaimOutcome::Gone => Err(ApiError::EndpointGone),
        ClaimOutcome::Refused(reason) => {
            tracing::info!(
                reason = refusal_name(reason),
                "an endpoint claim was refused"
            );
            Err(ApiError::ClaimRefused)
        }
    }
}

fn refusal_name(reason: ClaimRefusal) -> &'static str {
    match reason {
        ClaimRefusal::NetworkUnknown => "network_unknown",
        ClaimRefusal::NetworkMismatch => "network_mismatch",
        ClaimRefusal::WindowExpired => "window_expired",
    }
}
