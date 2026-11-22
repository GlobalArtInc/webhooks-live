use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_store::{ClaimOutcome, ClaimRefusal, EndpointRole};

use crate::dto::EndpointDto;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::StoreUnavailable;
use crate::state::ApiState;

pub const CLAIM_WINDOW: Duration = Duration::from_secs(24 * 3600);

#[utoipa::path(
    post,
    path = "/endpoints/{id}/claim",
    operation_id = "endpoints_claim",
    tag = "Endpoints",
    summary = "Claim an anonymous endpoint as its owner",
    description = "Only in `mixed`. The caller becomes the owner of an endpoint that has no owner, was created within the last 24 hours and was created from the caller's network; the response is the endpoint with `my_role` `owner`. Session only: a Bearer token answers 403 `session_required`. A view-protected endpoint also needs a valid grant (`X-Webhooks-Live-Grant`).",
    security(("session_cookie" = [])),
    params(
        EndpointIdParam,
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of a view-protected endpoint, from `POST .../unlock`")
    ),
    responses(
        (status = 200, description = "The claimed endpoint", body = EndpointDto),
        (status = 401, description = "`unauthenticated` or `invalid_token` (no valid session or Bearer token); `grant_required`: a view-protected endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`not_available_in_mode`: the instance is not `mixed`; `sign_in_required`: no signed-in caller; `claim_refused`: the caller is outside the creating network, the endpoint is older than 24 hours or has no recorded network; `session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 409, description = "`already_owned`: the endpoint has an owner, or the caller is already a member", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or one owned by someone else", body = ErrorBody),
        (status = 422, description = "`limit_reached`: the caller's endpoint limit (`error.limit` = `{name: \"endpoints\", max}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
