use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use serde::Serialize;
use whl_auth::Caller;
use whl_core::alias::{Alias, AliasError};
use whl_core::endpoint_id::EndpointId;
use whl_store::{AliasAvailability, AliasClaim, AliasRecord, PutAliasOutcome, ReleaseAliasOutcome};

use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::folders::signed_in;
use crate::settings::MEMBER_ALIASES;
use crate::state::ApiState;

#[derive(Serialize)]
pub struct AliasDto {
    alias: String,
    url: String,
    created_at: String,
}

impl AliasDto {
    fn new(record: &AliasRecord, public_url: &str) -> Self {
        Self {
            alias: record.alias.unpack().to_owned(),
            url: format!("{public_url}/a/{}", record.alias.unpack()),
            created_at: timestamp(record.created_at),
        }
    }
}

#[derive(Serialize)]
pub struct AliasListDto {
    items: Vec<AliasDto>,
}

#[derive(Serialize)]
pub struct AvailabilityDto {
    alias: String,
    available: bool,
    reason: Option<&'static str>,
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/endpoints/{id}/aliases", get(list))
        .route(
            "/endpoints/{id}/aliases/{alias}",
            put(assign).delete(release),
        )
        .route("/aliases/{alias}/availability", get(availability))
}

async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<AliasListDto>, ApiError> {
    ensure_users(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let aliases = state.store.list_aliases(&endpoint).await?;
    Ok(Json(AliasListDto {
        items: aliases
            .iter()
            .map(|record| AliasDto::new(record, &state.settings.public_url))
            .collect(),
    }))
}

async fn assign(
    State(state): State<ApiState>,
    caller: Caller,
    Path((raw_id, raw_alias)): Path<(String, String)>,
) -> Result<Json<AliasDto>, ApiError> {
    ensure_users(&state)?;
    let user = signed_in(&state, &caller)?;
    let endpoint = endpoint_id(&raw_id)?;
    let alias = Alias::parse(&raw_alias).map_err(invalid_alias)?;
    let outcome = state
        .store
        .put_alias(AliasClaim {
            endpoint: &endpoint,
            user,
            alias: &alias,
            retention: state.settings.retention,
            quarantine: state.alias_quarantine,
            max_owned: MEMBER_ALIASES,
        })
        .await?;
    match outcome {
        PutAliasOutcome::Stored(record) => {
            tracing::info!(endpoint = %endpoint, alias = %alias, "alias stored");
            Ok(Json(AliasDto::new(&record, &state.settings.public_url)))
        }
        PutAliasOutcome::Taken => Err(ApiError::AliasTaken),
        PutAliasOutcome::LimitReached => Err(ApiError::AliasLimit),
        PutAliasOutcome::EndpointGone | PutAliasOutcome::NotOwner => Err(ApiError::EndpointGone),
    }
}

async fn release(
    State(state): State<ApiState>,
    Path((raw_id, raw_alias)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ensure_users(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let alias = Alias::parse(&raw_alias).map_err(|_| ApiError::AliasNotFound)?;
    match state.store.release_alias(&endpoint, &alias).await? {
        ReleaseAliasOutcome::Released => {
            tracing::info!(endpoint = %endpoint, alias = %alias, "alias released");
            Ok(StatusCode::NO_CONTENT)
        }
        ReleaseAliasOutcome::NotFound => Err(ApiError::AliasNotFound),
    }
}

async fn availability(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_alias): Path<String>,
) -> Result<Json<AvailabilityDto>, ApiError> {
    ensure_users(&state)?;
    let user = signed_in(&state, &caller)?;
    let verdict = |available: bool, reason: Option<&'static str>| {
        Json(AvailabilityDto {
            alias: raw_alias.clone(),
            available,
            reason,
        })
    };
    let alias = match Alias::parse(&raw_alias) {
        Ok(alias) => alias,
        Err(AliasError::Reserved) => return Ok(verdict(false, Some("reserved"))),
        Err(AliasError::Format) => return Ok(verdict(false, Some("invalid"))),
    };
    match state
        .store
        .alias_availability(&alias, user, state.alias_quarantine)
        .await?
    {
        AliasAvailability::Available => Ok(verdict(true, None)),
        AliasAvailability::Taken => Ok(verdict(false, Some("taken"))),
    }
}

fn ensure_users(state: &ApiState) -> Result<(), ApiError> {
    if state.access.has_users() {
        Ok(())
    } else {
        Err(ApiError::NotAvailableInMode)
    }
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

fn invalid_alias(error: AliasError) -> ApiError {
    let code = match error {
        AliasError::Reserved => FieldCode::Reserved,
        AliasError::Format => FieldCode::InvalidFormat,
    };
    ApiError::Validation(vec![FieldViolation::new("alias", code, error.to_string())])
}
