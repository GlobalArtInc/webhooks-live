use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_auth::{Access, AuthRejection, Caller, Creation};
use whl_core::endpoint_id::EndpointId;
use whl_store::{
    AccessSettings, EndpointRole, Lifecycle, LifecycleLimits, NewEndpoint, UserRecord,
};

use crate::access;
use crate::body::RawBody;
use crate::dto::{EndpointDto, InstanceDto};
use crate::error::ApiError;
use crate::input::parse_endpoint_input;
use crate::lifecycle::{self, Standing, combine};
use crate::resolve::{defaults, resolve};
use crate::settings::ApiSettings;
use crate::state::ApiState;

pub async fn instance(State(state): State<ApiState>, caller: Caller) -> Json<InstanceDto> {
    Json(InstanceDto::new(
        &state.settings,
        &state.access,
        caller.user(),
        state.secrets_enabled(),
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
    let creation = state.access.creation(&caller)?;
    let parsed = parse_endpoint_input(&body, true)?;
    let standing = Standing {
        owned: matches!(creation, Creation::Owned(_)),
        signed_in: caller.user().is_some(),
        sign_in_possible: state.access.has_users(),
        role: None,
    };
    let id = EndpointId::generate().map_err(|_| ApiError::EntropyUnavailable)?;
    let change = combine(
        resolve(
            &state.settings,
            &defaults(),
            &parsed.patch,
            parsed.violations,
        ),
        lifecycle::resolve(
            &state.settings,
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
    let (owner_id, creator_net) = match &creation {
        Creation::Owned(user) => (Some(*user), None),
        Creation::Anonymous { network } => (None, network.as_ref()),
    };
    let record = state
        .store
        .create_endpoint_with_lifecycle(
            NewEndpoint {
                id: &id,
                owner: owner_id,
                creator_net,
                settings: &change.settings,
                lifecycle: change.lifecycle,
                access: &change.access,
                limits: never_expire_limits(&state.settings),
            },
            state.settings.retention,
        )
        .await?;
    let owner = caller.user();
    let location = format!("/api/v1/endpoints/{}", record.id.unpack());
    let role = match creation {
        Creation::Owned(_) => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } if state.access.is_mixed() => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } => None,
    };
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(EndpointDto::from_record(&record, &state.settings, &[]).with_identity(role, owner)),
    )
        .into_response())
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
    Ok(Json(
        EndpointDto::from_record(&record, &state.settings, &rules)
            .with_identity(access.role(), owner.as_ref()),
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
    let record = state
        .store
        .update_endpoint(
            &id,
            state.settings.retention,
            never_expire_limits(&state.settings),
            |current, owner_id| {
                let standing = Standing {
                    owned: owner_id.is_some(),
                    signed_in,
                    sign_in_possible,
                    role,
                };
                combine(
                    resolve(
                        &state.settings,
                        &current.settings,
                        &parsed.patch,
                        parsed.violations,
                    ),
                    lifecycle::resolve(
                        &state.settings,
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
                )
            },
        )
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let rules = state.store.list_forward_rules(&id).await?;
    let owner = owner_of(&state, &id).await?;
    Ok(Json(
        EndpointDto::from_record(&record, &state.settings, &rules)
            .with_identity(access.role(), owner.as_ref()),
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

fn never_expire_limits(settings: &ApiSettings) -> LifecycleLimits {
    LifecycleLimits {
        never_expire_max: settings.lifecycle_limits(true).never_expire_endpoints,
    }
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
