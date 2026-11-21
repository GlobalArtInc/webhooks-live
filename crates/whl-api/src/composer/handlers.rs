use std::net::IpAddr;
use std::time::Duration;

use axum::Json;
use axum::extract::{FromRequest, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use whl_auth::{AccessControl, AccessMode, AuthRejection, Caller, EndpointRole, decide_access};
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_forward::{TargetCheckError, vet_target};
use whl_store::Uuid;
use whl_store::composer::{
    MAX_SENT_PER_OWNER, NewSent, OUTBOUND_METRIC, QuotaCharge, QuotaWindow, SentOutcome, SentOwner,
};

use super::dto::{HistoryDto, SendResultDto, SentRequestDto, StoredEntry};
use super::input::{SendInput, parse_send};
use super::outbound::{Executed, HOP_HEADER, run_external, run_local};
use super::redact::PathPolicy;
use super::settings::{Clients, MAX_HISTORY_PAGE, MAX_HOPS};
use super::target::{Destination, PublicOrigin, classify};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const OVERLOAD_RETRY_SECONDS: u64 = 1;
const SEND_BODY_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SendBody(pub axum::body::Bytes);

impl<S: Send + Sync> FromRequest<S> for SendBody {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let read = tokio::time::timeout(
            SEND_BODY_READ_TIMEOUT,
            RawBody::from_request(request, state),
        )
        .await
        .map_err(|_| ApiError::SendBodyTimeout)?;
        match read {
            Ok(RawBody(bytes)) => Ok(Self(bytes)),
            Err(ApiError::PayloadTooLarge) => Err(ApiError::SendPayloadTooLarge),
            Err(other) => Err(other),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct HistoryParams {
    endpoint_id: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

struct Identity {
    subject: String,
    owner: Option<SentOwner>,
    charges: Vec<QuotaCharge>,
}

pub async fn send(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    SendBody(bytes): SendBody,
) -> Result<Json<SendResultDto>, ApiError> {
    let composer = &state.composer;
    if !composer.enabled {
        return Err(ApiError::ComposerDisabled);
    }
    let clients = composer.clients(&state.forward).map_err(|error| {
        tracing::error!(%error, "the composer clients cannot be built");
        ApiError::ComposerInternal
    })?;
    let hop = incoming_hop(&headers);
    if hop >= MAX_HOPS {
        return Err(ApiError::HopLimit);
    }
    let input = parse_send(&bytes)?;
    let origin = PublicOrigin::parse(&state.settings.public_url);
    let destination = classify(&input.url, origin.as_ref(), &clients.guard)
        .map_err(|violation| ApiError::Validation(vec![violation]))?;
    if matches!(destination, Destination::Local(_)) && composer.local.is_none() {
        return Err(ApiError::Validation(vec![FieldViolation::new(
            "url",
            FieldCode::TargetRefused,
            "this instance cannot be called from here",
        )]));
    }
    let identity = identify(&state, &caller, &input)?;
    if let Some(endpoint) = &input.endpoint {
        authorize_endpoint(&state, &caller, &headers, endpoint, EndpointRole::Viewer).await?;
    }
    let busy = || ApiError::RateLimited {
        retry_after_secs: OVERLOAD_RETRY_SECONDS,
    };
    let _permit = composer
        .gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    let _subject = composer
        .acquire_subject(identity.subject.clone())
        .ok_or_else(busy)?;
    crate::quota::charge(
        &state,
        state.tier_of(&caller),
        OUTBOUND_METRIC,
        &identity.charges,
    )
    .await?;
    let local = matches!(destination, Destination::Local(_));
    let executed = match destination {
        Destination::External(target) => {
            vet(&state, clients, target.unpack().as_str()).await?;
            run_external(clients, &input, target, hop + 1).await
        }
        Destination::Local(target) => {
            let ip = caller.ip().ok_or(ApiError::ClientUnknown)?;
            let Some(local) = composer.local.as_deref() else {
                return Err(ApiError::ComposerInternal);
            };
            run_local(local, target, &input, ip, hop + 1).await
        }
    };
    tracing::info!(
        client_ip = caller.ip().map(|ip| ip.to_string()).unwrap_or_default(),
        subject = %identity.subject,
        who = if caller.user().is_some() { "user" } else { "anonymous" },
        route = if local { "local" } else { "external" },
        host = input.url.host_str().unwrap_or_default(),
        status = ?executed.status,
        error = executed.error.as_deref().and_then(|text| text.split(':').next()).unwrap_or_default(),
        "composer request sent"
    );
    let id = new_id()?;
    if let Some(owner) = identity.owner {
        record(&state, id, owner, &input, &executed, local).await;
    }
    Ok(Json(SendResultDto::new(id, &executed)))
}

fn incoming_hop(headers: &HeaderMap) -> u32 {
    headers
        .get(HOP_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

fn new_id() -> Result<Uuid, ApiError> {
    whl_core::capture_id::CaptureId::generate()
        .map(|id| id.unpack())
        .map_err(|error| {
            tracing::error!(%error, "an id for a composer request cannot be generated");
            ApiError::ComposerInternal
        })
}

async fn vet(state: &ApiState, clients: &Clients, target: &str) -> Result<(), ApiError> {
    match vet_target(target, &clients.guard, state.forward.lookup.as_ref()).await {
        Ok(_) => Ok(()),
        Err(error) => {
            let (code, message) = match &error {
                TargetCheckError::Refused(reason) => (FieldCode::TargetRefused, reason.to_string()),
                TargetCheckError::Unresolvable(detail) => {
                    (FieldCode::TargetUnresolvable, detail.clone())
                }
            };
            Err(ApiError::Validation(vec![FieldViolation::new(
                "url", code, message,
            )]))
        }
    }
}

fn identify(state: &ApiState, caller: &Caller, input: &SendInput) -> Result<Identity, ApiError> {
    let tier = state.tier_of(caller).limits();
    let forced = state.composer.limits;
    if let Some(user) = caller.user() {
        return Ok(Identity {
            subject: format!("u:{}", user.id.hyphenated()),
            owner: Some(SentOwner::User(user.id)),
            charges: vec![QuotaCharge {
                subject: format!("u:{}", user.id.hyphenated()),
                window: QuotaWindow::Hour,
                limit: forced
                    .map(|limits| limits.user_per_hour)
                    .or(tier.outbound_per_user_hour)
                    .unwrap_or(0),
                name: OUTBOUND_METRIC,
            }],
        });
    }
    let ip: IpAddr = caller.ip().ok_or(ApiError::ClientUnknown)?;
    let subject = format!("n:{}", CreatorNet::from_ip(ip));
    let mut charges = vec![QuotaCharge {
        subject: subject.clone(),
        window: QuotaWindow::Day,
        limit: forced
            .map(|limits| limits.network_per_day)
            .or(tier.outbound_per_network_day)
            .unwrap_or(0),
        name: OUTBOUND_METRIC,
    }];
    let owner = input.endpoint.as_ref().map(|endpoint| {
        charges.push(QuotaCharge {
            subject: format!("e:{}", endpoint.unpack()),
            window: QuotaWindow::Hour,
            limit: forced
                .map(|limits| limits.endpoint_per_hour)
                .or(tier.outbound_per_endpoint_hour)
                .unwrap_or(0),
            name: OUTBOUND_METRIC,
        });
        SentOwner::Endpoint(endpoint.clone())
    });
    Ok(Identity {
        subject,
        owner,
        charges,
    })
}

async fn record(
    state: &ApiState,
    id: Uuid,
    owner: SentOwner,
    input: &SendInput,
    executed: &Executed,
    local: bool,
) {
    let policy = match owner {
        SentOwner::Endpoint(_) => PathPolicy::Origin,
        SentOwner::User(_) => PathPolicy::Tokens {
            first_segment: local,
        },
    };
    let stored = match StoredEntry::build(input, executed, policy) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!(%error, "a composer history entry cannot be encoded");
            return;
        }
    };
    let sent = NewSent {
        id,
        owner,
        source_capture_id: input.source_capture,
        request: stored.request,
        result: stored.result,
        response_body: stored.response_body,
    };
    match state.store.record_sent(&sent).await {
        Ok(SentOutcome::Recorded) => {}
        Ok(SentOutcome::OwnerGone) => {
            tracing::warn!("a composer history entry was not kept, its owner is gone");
        }
        Err(error) => tracing::warn!(%error, "a composer history entry was not kept"),
    }
}

pub async fn history(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryDto>, ApiError> {
    if !state.composer.enabled {
        return Err(ApiError::ComposerDisabled);
    }
    let limit = parse_limit(params.limit.as_deref())?;
    let after = match params.cursor.as_deref() {
        Some(text) => Some(parse_uuid(text).ok_or(ApiError::InvalidCursor)?),
        None => None,
    };
    let owner = match params.endpoint_id.as_deref() {
        Some(raw) => {
            let endpoint = EndpointId::parse(raw).map_err(|_| ApiError::NotFound)?;
            authorize_endpoint(&state, &caller, &headers, &endpoint, EndpointRole::Viewer).await?;
            SentOwner::Endpoint(endpoint)
        }
        None => match caller.user() {
            Some(user) => SentOwner::User(user.id),
            None => return Err(sign_in_needed(&state.access)),
        },
    };
    let page = state.store.list_sent(&owner, limit, after).await?;
    Ok(Json(HistoryDto {
        items: page.items.iter().map(SentRequestDto::from_record).collect(),
        next_cursor: page.next_after.map(|id| id.hyphenated().to_string()),
    }))
}

pub async fn delete(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Path(raw): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !state.composer.enabled {
        return Err(ApiError::ComposerDisabled);
    }
    let id = parse_uuid(&raw).ok_or(ApiError::NotFound)?;
    let owner = state
        .store
        .sent_owner(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let allowed = match &owner {
        SentOwner::User(user) => caller.user().is_some_and(|current| current.id == *user),
        SentOwner::Endpoint(endpoint) => {
            match authorize_endpoint(&state, &caller, &headers, endpoint, EndpointRole::Editor)
                .await
            {
                Ok(()) => true,
                Err(ApiError::Access(_) | ApiError::EndpointGone) => false,
                Err(other) => return Err(other),
            }
        }
    };
    if !allowed {
        return Err(ApiError::NotFound);
    }
    if state.store.delete_sent(id, &owner).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

fn parse_limit(text: Option<&str>) -> Result<i64, ApiError> {
    let Some(text) = text else {
        return Ok(MAX_SENT_PER_OWNER.min(MAX_HISTORY_PAGE));
    };
    let violation =
        |code, message| ApiError::Validation(vec![FieldViolation::new("limit", code, message)]);
    match text.parse::<i64>() {
        Ok(limit) if (1..=MAX_HISTORY_PAGE).contains(&limit) => Ok(limit),
        Ok(_) => Err(violation(FieldCode::OutOfRange, "must be between 1 and 50")),
        Err(_) => Err(violation(FieldCode::InvalidFormat, "must be an integer")),
    }
}

fn parse_uuid(text: &str) -> Option<Uuid> {
    Uuid::try_parse(text)
        .ok()
        .filter(|id| id.hyphenated().to_string() == text)
}

fn sign_in_needed(access: &AccessControl) -> ApiError {
    if access.is_mixed() {
        ApiError::Auth(AuthRejection::SignInRequired)
    } else {
        ApiError::Auth(AuthRejection::Unauthenticated)
    }
}

async fn authorize_endpoint(
    state: &ApiState,
    caller: &Caller,
    headers: &HeaderMap,
    endpoint: &EndpointId,
    needed: EndpointRole,
) -> Result<(), ApiError> {
    if state.access.mode() == AccessMode::Open {
        state
            .store
            .get_endpoint(endpoint, state.settings.retention)
            .await?
            .ok_or(ApiError::EndpointGone)?;
    } else {
        let role = state.access.role_on(caller, endpoint).await?;
        decide_access(role, needed)
            .into_result()
            .map_err(ApiError::Access)?;
    }
    if state
        .access
        .view_grant_valid(endpoint, headers, None)
        .await?
    {
        Ok(())
    } else {
        Err(ApiError::EndpointGone)
    }
}
