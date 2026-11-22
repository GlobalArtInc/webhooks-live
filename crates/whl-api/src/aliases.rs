use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};
use whl_auth::Caller;
use whl_core::alias::{Alias, AliasError};
use whl_core::endpoint_id::EndpointId;
use whl_store::{AliasAvailability, AliasClaim, AliasRecord, PutAliasOutcome, ReleaseAliasOutcome};

use crate::dto::timestamp;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::folders::signed_in;
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::StoreUnavailable;
use crate::routing::{Routes, get, put};
use crate::state::ApiState;

#[derive(Serialize, ToSchema)]
#[schema(as = Alias)]
pub struct AliasDto {
    alias: String,
    url: String,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = AliasList)]
pub struct AliasListDto {
    items: Vec<AliasDto>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = AliasAvailability)]
pub struct AvailabilityDto {
    alias: String,
    available: bool,
    #[schema(pattern = "^(reserved|invalid|taken)$")]
    reason: Option<&'static str>,
}

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/aliases", get(list))
        .route(
            "/endpoints/{id}/aliases/{alias}",
            put(assign).delete(release),
        )
        .route("/aliases/{alias}/availability", get(availability))
}

#[derive(OpenApi)]
#[openapi(
    paths(list, assign, release, availability),
    components(schemas(AliasDto, AliasListDto, AvailabilityDto)),
    tags((name = "Aliases", description = "Vanity aliases of an endpoint, served at `/a/<alias>`, and whether an alias is available"))
)]
pub struct AliasesDoc;

#[utoipa::path(
    get,
    path = "/endpoints/{id}/aliases",
    operation_id = "aliases_list",
    tag = "Aliases",
    summary = "List the aliases of an endpoint",
    description = "Needs the viewer role; a token needs the `read` scope. Not available in `open`.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
    ),
    responses(
        (status = 200, description = "The aliases of the endpoint", body = AliasListDto),
        (status = 401, description = "`unauthenticated`, `invalid_token` or `grant_required`", body = ErrorBody),
        (status = 403, description = "`not_available_in_mode`: the instance is `open`; `insufficient_scope`: a Bearer token without `read`", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    put,
    path = "/endpoints/{id}/aliases/{alias}",
    operation_id = "aliases_assign",
    tag = "Aliases",
    summary = "Give an endpoint an alias",
    description = "Owner only, signed in, session only (a Bearer token answers 403 `session_required`); not in `open`. The alias is stored with the endpoint and is served at `/a/<alias>`, and the caller's tier limits how many aliases it may own. A session cookie needs an `Origin` equal to the public origin, else 403 `csrf_rejected`. Answers 200 with the alias.",
    security(("session_cookie" = [])),
    params(
        EndpointIdParam,
        ("alias" = String, Path, description = "The alias to assign, in its path syntax"),
    ),
    responses(
        (status = 200, description = "The alias", body = AliasDto),
        (status = 401, description = "`unauthenticated`, `invalid_token` or `grant_required`", body = ErrorBody),
        (status = 403, description = "`not_available_in_mode`: the instance is `open`; `session_required`: a Bearer token; `sign_in_required`: `mixed` without a signed-in caller; `csrf_rejected`: the `Origin` check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 409, description = "`alias_taken`: the alias is in use or in quarantine", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or one the caller does not own", body = ErrorBody),
        (status = 422, description = "`validation_failed` (`error.fields[].code` `reserved` or `invalid_format` on `alias`) or `limit_reached` (`error.limit` = `{name: \"aliases\", max}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
async fn assign(
    State(state): State<ApiState>,
    caller: Caller,
    Path((raw_id, raw_alias)): Path<(String, String)>,
) -> Result<Json<AliasDto>, ApiError> {
    ensure_users(&state)?;
    let user = signed_in(&state, &caller)?;
    let endpoint = endpoint_id(&raw_id)?;
    let alias = Alias::parse(&raw_alias).map_err(invalid_alias)?;
    let max_owned = state.tier_of(&caller).limits().aliases;
    let outcome = state
        .store
        .put_alias(AliasClaim {
            endpoint: &endpoint,
            user,
            alias: &alias,
            retention: state.settings.retention,
            quarantine: state.alias_quarantine,
            max_owned,
        })
        .await?;
    match outcome {
        PutAliasOutcome::Stored(record) => {
            tracing::info!(endpoint = %endpoint, alias = %alias, "alias stored");
            Ok(Json(AliasDto::new(&record, &state.settings.public_url)))
        }
        PutAliasOutcome::Taken => Err(ApiError::AliasTaken),
        PutAliasOutcome::LimitReached => Err(ApiError::AliasLimit { max: max_owned }),
        PutAliasOutcome::EndpointGone | PutAliasOutcome::NotOwner => Err(ApiError::EndpointGone),
    }
}

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/aliases/{alias}",
    operation_id = "aliases_release",
    tag = "Aliases",
    summary = "Remove an alias from an endpoint",
    description = "Owner only, session only (a Bearer token answers 403 `session_required`); not in `open`. A session cookie needs an `Origin` equal to the public origin, else 403 `csrf_rejected`. An alias the endpoint does not hold, and a malformed alias, answer 404 `alias_not_found`.",
    security(("session_cookie" = [])),
    params(
        EndpointIdParam,
        ("alias" = String, Path, description = "The alias to remove"),
    ),
    responses(
        (status = 204, description = "The alias is removed"),
        (status = 401, description = "`unauthenticated`, `invalid_token` or `grant_required`", body = ErrorBody),
        (status = 403, description = "`not_available_in_mode`: the instance is `open`; `session_required`: a Bearer token; `csrf_rejected`: the `Origin` check failed", body = ErrorBody),
        (status = 404, description = "`alias_not_found`; `not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or one the caller does not own", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    get,
    path = "/aliases/{alias}/availability",
    operation_id = "aliases_availability",
    tag = "Aliases",
    summary = "Check whether an alias can be taken",
    description = "Signed in only; a token needs the `read` scope. Answers 200 for any text: `reason` is `reserved` for a reserved name, `invalid` for a malformed one, `taken` when another endpoint holds it or it is in quarantine, and null when `available` is true.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(("alias" = String, Path, description = "The alias to check")),
    responses(
        (status = 200, description = "The verdict", body = AvailabilityDto),
        (status = 401, description = "`unauthenticated` or `invalid_token`", body = ErrorBody),
        (status = 403, description = "`not_available_in_mode`: the instance is `open`; `sign_in_required`: `mixed` without a signed-in caller; `insufficient_scope`: a Bearer token without `read`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
