use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_auth::{Access, AuthRejection, Caller, Creation};
use whl_core::endpoint_id::EndpointId;
use whl_core::tier::TierLimits;
use whl_notify::PushService;
use whl_store::{
    AccessSettings, EndpointChange, EndpointRecord, EndpointRole, Lifecycle, LifecycleLimits,
    NewEndpoint, UserRecord, Uuid, ViewPasswordChange, ViewPasswordHash,
};

use crate::access;
use crate::body::RawBody;
use crate::cloning;
use crate::dto::{Availability, EndpointDto, InstanceDto};
use crate::error::{ApiError, FieldViolation};
use crate::input::{EndpointPatch, ParsedInput, parse_endpoint_input};
use crate::lifecycle::{self, Standing, combine};
use crate::resolve::{defaults, resolve};
use crate::state::ApiState;
use crate::view_password;

pub async fn instance(State(state): State<ApiState>, caller: Caller) -> Json<InstanceDto> {
    Json(InstanceDto::new(
        &state.settings,
        &state.access,
        caller.user(),
        state.tier_of(&caller),
        Availability {
            secrets: state.secrets_enabled(),
            schedules: state.schedules.enabled,
            notifications: state.notifications.enabled,
        },
        state.push.as_ref().map(PushService::public_key),
    ))
}

pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    if caller
        .token()
        .is_some_and(|grant| grant.endpoint_limit().is_some())
    {
        return Err(AuthRejection::InsufficientScope.into());
    }
    if let Some(request) = cloning::split_template_request(&body)? {
        if caller.token().is_some() {
            return Err(AuthRejection::SessionRequired.into());
        }
        return cloning::create_from_template(&state, &caller, request).await;
    }
    let creation = state.access.creation(&caller)?;
    let parsed = parse_endpoint_input(&body, true)?;
    let (record, view_hash) =
        store_new_endpoint_with_view(&state, &caller, &creation, parsed).await?;
    let owner = caller.user();
    let location = format!("/api/v1/endpoints/{}", record.id.unpack());
    let role = match creation {
        Creation::Owned(_) => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } if state.access.is_mixed() => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } => None,
    };
    let view_grant = view_password::grant_for_setter(&state, &record.id, view_hash.as_ref()).await;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(
            EndpointDto::from_record(&record, &state.settings, &[])
                .with_identity(role, owner)
                .with_view_protected(view_hash.is_some())
                .with_view_grant(view_grant),
        ),
    )
        .into_response())
}

pub(crate) async fn store_new_endpoint(
    state: &ApiState,
    caller: &Caller,
    creation: &Creation,
    parsed: ParsedInput,
) -> Result<EndpointRecord, ApiError> {
    store_new_endpoint_with_view(state, caller, creation, parsed)
        .await
        .map(|(record, _)| record)
}

async fn store_new_endpoint_with_view(
    state: &ApiState,
    caller: &Caller,
    creation: &Creation,
    mut parsed: ParsedInput,
) -> Result<(EndpointRecord, Option<ViewPasswordHash>), ApiError> {
    require_secrets_for_verifier(state, &parsed.patch)?;
    view_password::forbid_when_owned_on_create(
        &mut parsed.violations,
        &parsed.patch.view_password,
        matches!(creation, Creation::Owned(_)),
    );
    let standing = Standing {
        owned: matches!(creation, Creation::Owned(_)),
        signed_in: caller.user().is_some(),
        sign_in_possible: state.access.has_users(),
        role: None,
    };
    let id = EndpointId::generate().map_err(|_| ApiError::EntropyUnavailable)?;
    let tier = if standing.owned {
        state.tier_of(caller)
    } else {
        state.tiers.anonymous()
    };
    let change = combine(
        resolve(
            &state.settings,
            &defaults(),
            &parsed.patch,
            parsed.violations,
        ),
        lifecycle::resolve(
            &state.settings,
            tier,
            standing,
            &Lifecycle::default(),
            &parsed.patch.lifecycle,
        ),
        access::resolve(
            state.secrets.as_deref(),
            standing,
            &id,
            &AccessSettings::default(),
            &parsed.patch.access,
        ),
    )?;
    let (owner_id, creator_net) = match creation {
        Creation::Owned(user) => (Some(*user), None),
        Creation::Anonymous { network } => (None, network.as_ref()),
    };
    let view_hash =
        view_password::prepare(state, caller.ip(), &parsed.patch.view_password).await?;
    let new = NewEndpoint {
        id: &id,
        owner: owner_id,
        creator_net,
        settings: &change.settings,
        lifecycle: change.lifecycle,
        access: &change.access,
        limits: creation_limits(tier.limits()),
    };
    let record = match &view_hash {
        Some(hash) => {
            state
                .store
                .create_endpoint_with_view_password(new, hash, state.settings.retention)
                .await?
        }
        None => {
            state
                .store
                .create_endpoint_with_lifecycle(new, state.settings.retention)
                .await?
        }
    };
    Ok((record, view_hash))
}

pub async fn show(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
) -> Result<Json<EndpointDto>, ApiError> {
    let id = endpoint_id(&raw_id)?;
    let record = state
        .store
        .get_endpoint(&id, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let rules = state.store.list_forward_rules(&id).await?;
    let owner = owner_of(&state, &id).await?;
    let protected = state.access.view_access()?.is_protected(&id).await?;
    Ok(Json(
        EndpointDto::from_record(&record, &state.settings, &rules)
            .with_identity(access.role(), owner.as_ref())
            .with_view_protected(protected),
    ))
}

pub async fn update(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<EndpointDto>, ApiError> {
    let id = endpoint_id(&raw_id)?;
    let parsed = parse_endpoint_input(&body, false)?;
    let signed_in = access.caller().user().is_some();
    let sign_in_possible = state.access.has_users();
    let role = access.role();
    require_secrets_for_verifier(&state, &parsed.patch)?;
    let caller_tier = state.tier_of(access.caller());
    let merge = |current: &EndpointRecord,
                 owner_id: Option<Uuid>,
                 violations: Vec<FieldViolation>|
     -> Result<EndpointChange, ApiError> {
        let standing = Standing {
            owned: owner_id.is_some(),
            signed_in,
            sign_in_possible,
            role,
        };
        let tier = if standing.owned {
            caller_tier
        } else {
            state.tiers.anonymous()
        };
        let outcome = combine(
            resolve(
                &state.settings,
                &current.settings,
                &parsed.patch,
                violations,
            ),
            lifecycle::resolve(
                &state.settings,
                tier,
                standing,
                &current.lifecycle,
                &parsed.patch.lifecycle,
            ),
            access::resolve(
                state.secrets.as_deref(),
                standing,
                &id,
                &current.access,
                &parsed.patch.access,
            ),
        );
        view_password::forbid_when_owned(outcome, &parsed.patch.view_password, owner_id.is_some())
    };
    if matches!(parsed.patch.view_password, Some(Some(_))) {
        let snapshot = state
            .store
            .endpoint_snapshot(&id, state.settings.retention)
            .await?
            .ok_or(ApiError::EndpointGone)?;
        merge(&snapshot.record, snapshot.owner, parsed.violations.clone())?;
    }
    let view_hash =
        view_password::prepare(&state, access.caller().ip(), &parsed.patch.view_password).await?;
    let view_change = view_password::change_of(&parsed.patch.view_password, view_hash.as_ref());
    let record = state
        .store
        .update_endpoint_with_view_password(
            &id,
            state.settings.retention,
            update_limits(caller_tier.limits()),
            view_change,
            |current, owner_id| merge(current, owner_id, parsed.violations.clone()),
        )
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let rules = state.store.list_forward_rules(&id).await?;
    let owner = owner_of(&state, &id).await?;
    let protected = match view_change {
        ViewPasswordChange::Set(_) => true,
        ViewPasswordChange::Remove => false,
        ViewPasswordChange::Keep => state.access.view_access()?.is_protected(&id).await?,
    };
    let view_grant = view_password::grant_for_setter(&state, &id, view_hash.as_ref()).await;
    Ok(Json(
        EndpointDto::from_record(&record, &state.settings, &rules)
            .with_identity(access.role(), owner.as_ref())
            .with_view_protected(protected)
            .with_view_grant(view_grant),
    ))
}

pub async fn delete(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = endpoint_id(&raw_id)?;
    if state
        .store
        .delete_endpoint(&id, state.settings.retention)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::EndpointGone)
    }
}

pub(crate) fn creation_limits(tier: &TierLimits) -> LifecycleLimits {
    LifecycleLimits {
        endpoints_max: Some(tier.endpoints),
        ..update_limits(tier)
    }
}

fn update_limits(tier: &TierLimits) -> LifecycleLimits {
    LifecycleLimits {
        never_expire_max: tier.never_expire_endpoints,
        endpoints_max: None,
        storage_bytes_max: tier.storage_bytes,
    }
}

fn require_secrets_for_verifier(state: &ApiState, patch: &EndpointPatch) -> Result<(), ApiError> {
    if matches!(patch.verifier, Some(Some(_))) && !state.secrets_enabled() {
        return Err(ApiError::SecretsDisabled);
    }
    Ok(())
}

async fn owner_of(state: &ApiState, id: &EndpointId) -> Result<Option<UserRecord>, ApiError> {
    if !state.access.has_users() {
        return Ok(None);
    }
    Ok(state.store.endpoint_owner(id).await?)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
