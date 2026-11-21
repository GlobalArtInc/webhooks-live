use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_auth::{Access, Caller, Creation};
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointRole, UserRecord};

use crate::body::RawBody;
use crate::dto::{EndpointDto, InstanceDto};
use crate::error::ApiError;
use crate::input::parse_endpoint_input;
use crate::resolve::{defaults, resolve};
use crate::state::ApiState;

pub async fn instance(State(state): State<ApiState>, caller: Caller) -> Json<InstanceDto> {
    Json(InstanceDto::new(
        &state.settings,
        &state.access,
        caller.user(),
    ))
}

pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let creation = state.access.creation(&caller)?;
    let parsed = parse_endpoint_input(&body, true)?;
    let settings = resolve(
        &state.settings,
        &defaults(),
        &parsed.patch,
        parsed.violations,
    )
    .map_err(ApiError::Validation)?;
    let record = match &creation {
        Creation::Owned(user) => {
            state
                .store
                .create_endpoint_owned(Some(*user), &settings, state.settings.retention)
                .await?
        }
        Creation::Anonymous { network } => {
            state
                .store
                .create_endpoint_from_network(network.as_ref(), &settings, state.settings.retention)
                .await?
        }
    };
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
    let record = state
        .store
        .update_endpoint(&id, state.settings.retention, |current| {
            resolve(
                &state.settings,
                &current.settings,
                &parsed.patch,
                parsed.violations,
            )
            .map_err(ApiError::Validation)
        })
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

async fn owner_of(state: &ApiState, id: &EndpointId) -> Result<Option<UserRecord>, ApiError> {
    if !state.access.has_users() {
        return Ok(None);
    }
    Ok(state.store.endpoint_owner(id).await?)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
