use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use whl_auth::{Access, AuthRejection, Caller, Creation, EndpointRole};
use whl_core::endpoint_id::EndpointId;
use whl_store::{CloneOutcome, NewClonedEndpoint, NewEndpoint};

use super::doc::{CloneInputDoc, CloneResultDoc};
use super::document::ConfigDocument;
use super::export::{ChannelUrls, Selection, export};
use super::prepare::{Overrides, prepare};
use super::request::parse_clone_request;
use crate::body::RawBody;
use crate::dto::EndpointDto;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::handlers::creation_limits;
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, UnsupportedMediaType,
};
use crate::state::ApiState;

#[derive(Serialize)]
struct CloneDto {
    #[serde(flatten)]
    endpoint: EndpointDto,
    secrets_missing: Vec<String>,
    skipped: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/clone",
    operation_id = "cloning_clone",
    tag = "Cloning",
    summary = "Clone an endpoint",
    description = "Creates a new endpoint from the configuration of the source. `include` picks the parts to copy (`response`, `forward_rules`, `rules`, `channels`, `schedules`); absent copies all of them, and `secrets` is refused. Secret values are never copied: `secrets_missing` lists the secret names the copy uses and does not have, to be set again on the clone. Channel URLs are copied only for an editor of the source; otherwise `skipped` lists `channels`. `skipped` also names other parts that could not be copied. The clone is owned by the caller in `sso` and `mixed`; in `open` anonymous callers may clone and the clone is anonymous. Needs the viewer role on the source; a token is refused with 403 `session_required`. The body is optional; an empty body copies everything.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    request_body(content = Option<CloneInputDoc>, content_type = "application/json"),
    responses(
        (status = 201, description = "The new endpoint (its Endpoint fields at the top level) with `secrets_missing` and `skipped`; `Location` is its path", body = CloneResultDoc, headers(("Location" = String, description = "`/api/v1/endpoints/{id}` of the clone"))),
        (status = 400, response = BadRequest),
        (status = 401, description = "`unauthenticated`: `sso` without a session; `grant_required`: a view-protected anonymous source without a valid grant", body = ErrorBody),
        (status = 403, description = "`sign_in_required`: `mixed` without a session; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted source, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`: an unknown field, an unknown part in `include` or `secrets` there, a `title` over 100 characters, or a copied part that fails validation) or `limit_reached` (`error.limit` = `{name: \"endpoints\", max}` or `{name: \"schedules\", max}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
    let tier = state.creation_tier(caller, creation);
    let max_schedules = usize::try_from(tier.limits().schedules).unwrap_or(usize::MAX);
    let new = NewClonedEndpoint {
        id: &prepared.id,
        endpoint: NewEndpoint {
            id: &prepared.id,
            owner,
            creator_net,
            settings: &prepared.change.settings,
            lifecycle: prepared.change.lifecycle,
            access: &prepared.change.access,
            limits: creation_limits(tier.limits()),
        },
        parts: &prepared.parts,
        max_schedules_per_owner: max_schedules,
    };
    let record = match state
        .store
        .create_endpoint_with_parts(&new, state.settings.retention)
        .await?
    {
        CloneOutcome::Created(record) => *record,
        CloneOutcome::ScheduleLimitReached => {
            return Err(ApiError::ScheduleLimit(max_schedules));
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
