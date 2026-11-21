use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use whl_auth::{Access, AuthRejection, Caller, Creation, EndpointRole};
use whl_core::endpoint_id::EndpointId;
use whl_store::{CloneOutcome, NewClonedEndpoint, NewEndpoint};

use super::document::ConfigDocument;
use super::export::{ChannelUrls, Selection, export};
use super::prepare::{Overrides, prepare};
use super::request::parse_clone_request;
use crate::body::RawBody;
use crate::dto::EndpointDto;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::handlers::never_expire_limits;
use crate::state::ApiState;

#[derive(Serialize)]
struct CloneDto {
    #[serde(flatten)]
    endpoint: EndpointDto,
    secrets_missing: Vec<String>,
    skipped: Vec<String>,
}

pub async fn clone_endpoint(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let caller = access.caller();
    if state.access.has_users() && caller.user().is_none() {
        return Err(sign_in_rejection(&state).into());
    }
    let source = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let request = parse_clone_request(&body)?;
    let creation = state.access.creation(caller)?;
    let role = access.role().unwrap_or(EndpointRole::Viewer);
    let record = state
        .store
        .get_endpoint(&source, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let urls = match state.secrets.as_deref() {
        Some(keyring) if role >= EndpointRole::Editor => ChannelUrls::Include(keyring),
        _ => ChannelUrls::Omit,
    };
    let exported = export(&state, &record, request.selection, role, urls).await?;
    let response = create_from_document(
        &state,
        caller,
        &creation,
        &exported.document,
        request.selection,
        request.overrides,
        exported.withheld,
    )
    .await?;
    tracing::info!(source = %source, "endpoint cloned");
    Ok(response)
}

pub(crate) async fn create_from_document(
    state: &ApiState,
    caller: &Caller,
    creation: &Creation,
    document: &ConfigDocument,
    selection: Selection,
    overrides: Overrides,
    withheld: Vec<String>,
) -> Result<Response, ApiError> {
    let prepared = prepare(state, caller, creation, document, selection, overrides).await?;
    let (owner, creator_net) = match creation {
        Creation::Owned(user) => (Some(*user), None),
        Creation::Anonymous { network } => (None, network.as_ref()),
    };
    let new = NewClonedEndpoint {
        id: &prepared.id,
        endpoint: NewEndpoint {
            owner,
            creator_net,
            settings: &prepared.change.settings,
            lifecycle: prepared.change.lifecycle,
            limits: never_expire_limits(&state.settings),
        },
        parts: &prepared.parts,
        max_schedules_per_owner: state.schedules.max_per_user,
    };
    let record = match state
        .store
        .create_endpoint_with_parts(&new, state.settings.retention)
        .await?
    {
        CloneOutcome::Created(record) => *record,
        CloneOutcome::ScheduleLimitReached => {
            return Err(ApiError::ScheduleLimit(state.schedules.max_per_user));
        }
        CloneOutcome::PartLimitExceeded => {
            return Err(ApiError::Validation(vec![FieldViolation::new(
                "config",
                FieldCode::TooMany,
                "has more parts than an endpoint can hold",
            )]));
        }
    };
    let forward_rules = state.store.list_forward_rules(&record.id).await?;
    let role = match creation {
        Creation::Owned(_) => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } if state.access.is_mixed() => Some(EndpointRole::Owner),
        Creation::Anonymous { .. } => None,
    };
    let mut skipped = prepared.skipped;
    skipped.extend(withheld);
    let location = format!("/api/v1/endpoints/{}", record.id.unpack());
    let dto = CloneDto {
        endpoint: EndpointDto::from_record(&record, &state.settings, &forward_rules)
            .with_identity(role, caller.user()),
        secrets_missing: prepared.secrets_missing,
        skipped,
    };
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(dto),
    )
        .into_response())
}

pub(crate) fn sign_in_rejection(state: &ApiState) -> AuthRejection {
    if state.access.is_mixed() {
        AuthRejection::SignInRequired
    } else {
        AuthRejection::Unauthenticated
    }
}
