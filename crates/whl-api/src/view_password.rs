use std::net::IpAddr;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::Value;
use whl_auth::{Caller, IssuedGrant, ViewPassword};
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointChange, ViewPasswordChange, ViewPasswordHash};

use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const PASSWORD_KEY: &str = "password";
const FIELD: &str = "view_password";

#[derive(Serialize)]
pub struct GrantDto {
    grant: String,
    expires_at: String,
}

impl From<&IssuedGrant> for GrantDto {
    fn from(issued: &IssuedGrant) -> Self {
        Self {
            grant: issued.grant.unpack().to_owned(),
            expires_at: timestamp(issued.expires_at),
        }
    }
}

pub async fn unlock(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<GrantDto>, ApiError> {
    let endpoint = EndpointId::parse(&raw_id).map_err(|_| ApiError::NotFound)?;
    let password = parse_unlock(&body)?;
    let issued = state
        .access
        .view_access()?
        .unlock(&endpoint, caller.ip(), &password)
        .await?;
    Ok(Json(GrantDto::from(&issued)))
}

fn parse_unlock(bytes: &[u8]) -> Result<String, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(mut object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut violations: Vec<FieldViolation> = object
        .keys()
        .filter(|key| key.as_str() != PASSWORD_KEY)
        .map(|key| FieldViolation::new(key.as_str(), FieldCode::NotAllowed, "is not a known field"))
        .collect();
    match object.remove(PASSWORD_KEY) {
        None => violations.push(FieldViolation::new(
            PASSWORD_KEY,
            FieldCode::Required,
            "is required",
        )),
        Some(Value::String(text)) if violations.is_empty() => return Ok(text),
        Some(Value::String(_)) => {}
        Some(_) => violations.push(FieldViolation::new(
            PASSWORD_KEY,
            FieldCode::InvalidFormat,
            "must be a string",
        )),
    }
    Err(ApiError::Validation(violations))
}

pub async fn prepare(
    state: &ApiState,
    ip: Option<IpAddr>,
    requested: &Option<Option<ViewPassword>>,
) -> Result<Option<ViewPasswordHash>, ApiError> {
    match requested {
        Some(Some(password)) => Ok(Some(
            state
                .access
                .view_access()?
                .hash_password(ip, password)
                .await?,
        )),
        Some(None) | None => Ok(None),
    }
}

pub async fn grant_for_setter(
    state: &ApiState,
    endpoint: &EndpointId,
    hashed: Option<&ViewPasswordHash>,
) -> Option<GrantDto> {
    let hashed = hashed?;
    let views = state.access.view_access().ok()?;
    match views.grant_for_setter(endpoint, hashed).await {
        Ok(issued) => issued.as_ref().map(GrantDto::from),
        Err(rejection) => {
            tracing::warn!(%rejection, "cannot issue the grant for a password that was just set");
            None
        }
    }
}

pub fn change_of<'a>(
    requested: &Option<Option<ViewPassword>>,
    hashed: Option<&'a ViewPasswordHash>,
) -> ViewPasswordChange<'a> {
    match (requested, hashed) {
        (Some(Some(_)), Some(hash)) => ViewPasswordChange::Set(hash),
        (Some(None), _) => ViewPasswordChange::Remove,
        (Some(Some(_)) | None, _) => ViewPasswordChange::Keep,
    }
}

pub fn forbid_when_owned(
    outcome: Result<EndpointChange, ApiError>,
    requested: &Option<Option<ViewPassword>>,
    owned: bool,
) -> Result<EndpointChange, ApiError> {
    if requested.is_none() || !owned {
        return outcome;
    }
    let violation = FieldViolation::new(
        FIELD,
        FieldCode::NotAllowed,
        "only an endpoint without an owner can have a view password",
    );
    match outcome {
        Err(ApiError::Validation(mut violations)) => {
            violations.push(violation);
            Err(ApiError::Validation(violations))
        }
        Err(other) => Err(other),
        Ok(_) => Err(ApiError::Validation(vec![violation])),
    }
}

pub fn forbid_when_owned_on_create(
    violations: &mut Vec<FieldViolation>,
    requested: &Option<Option<ViewPassword>>,
    owned: bool,
) {
    if requested.is_some() && owned {
        violations.push(FieldViolation::new(
            FIELD,
            FieldCode::NotAllowed,
            "only an endpoint without an owner can have a view password",
        ));
    }
}
