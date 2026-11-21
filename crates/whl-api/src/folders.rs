use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::{Map, Value};
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_store::{CreateFolderOutcome, FolderRecord, UpdateFolderOutcome, Uuid};

use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::routing::{Routes, get, patch, put};
use crate::state::ApiState;

const NAME_MAX_CHARS: usize = 64;

#[derive(Serialize)]
pub struct FolderDto {
    id: String,
    name: String,
    position: i32,
    endpoint_ids: Vec<String>,
    created_at: String,
}

impl From<&FolderRecord> for FolderDto {
    fn from(record: &FolderRecord) -> Self {
        Self {
            id: record.id.hyphenated().to_string(),
            name: record.name.clone(),
            position: record.position,
            endpoint_ids: record.endpoint_ids.clone(),
            created_at: timestamp(record.created_at),
        }
    }
}

#[derive(Serialize)]
pub struct FolderListDto {
    items: Vec<FolderDto>,
}

struct FolderPatch {
    name: Option<String>,
    position: Option<i32>,
}

pub fn routes() -> Routes {
    Routes::new()
        .route("/folders", get(list).post(create))
        .route("/folders/{folder_id}", patch(update).delete(destroy))
        .route(
            "/folders/{folder_id}/endpoints/{id}",
            put(add_endpoint).delete(remove_endpoint),
        )
}

async fn list(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<FolderListDto>, ApiError> {
    let user = signed_in(&state, &caller)?;
    let folders = state
        .store
        .list_folders(user, state.settings.retention)
        .await?;
    Ok(Json(FolderListDto {
        items: folders.iter().map(FolderDto::from).collect(),
    }))
}

async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    RawBody(body): RawBody,
) -> Result<(StatusCode, Json<FolderDto>), ApiError> {
    let user = signed_in(&state, &caller)?;
    let name = parse_create(&body)?;
    let max = usize::try_from(state.tier_of(&caller).limits().folders).unwrap_or(usize::MAX);
    match state.store.create_folder(user, &name, max).await? {
        CreateFolderOutcome::Created(record) => {
            Ok((StatusCode::CREATED, Json(FolderDto::from(&record))))
        }
        CreateFolderOutcome::NameTaken => Err(ApiError::FolderNameTaken),
        CreateFolderOutcome::LimitReached => Err(ApiError::FolderLimit { max }),
    }
}

async fn update(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_folder): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<FolderDto>, ApiError> {
    let user = signed_in(&state, &caller)?;
    let folder = folder_id(&raw_folder)?;
    let patch = parse_update(&body)?;
    match state
        .store
        .update_folder(
            user,
            folder,
            patch.name.as_deref(),
            patch.position,
            state.settings.retention,
        )
        .await?
    {
        UpdateFolderOutcome::Updated(record) => Ok(Json(FolderDto::from(&record))),
        UpdateFolderOutcome::NameTaken => Err(ApiError::FolderNameTaken),
        UpdateFolderOutcome::NotFound => Err(ApiError::FolderNotFound),
    }
}

async fn destroy(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_folder): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = signed_in(&state, &caller)?;
    let folder = folder_id(&raw_folder)?;
    if state.store.delete_folder(user, folder).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::FolderNotFound)
    }
}

async fn add_endpoint(
    State(state): State<ApiState>,
    caller: Caller,
    Path((raw_folder, raw_endpoint)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = signed_in(&state, &caller)?;
    let folder = folder_id(&raw_folder)?;
    let endpoint = EndpointId::parse(&raw_endpoint).map_err(|_| ApiError::NotFound)?;
    if state
        .store
        .add_folder_endpoint(user, folder, &endpoint)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::FolderNotFound)
    }
}

async fn remove_endpoint(
    State(state): State<ApiState>,
    caller: Caller,
    Path((raw_folder, raw_endpoint)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = signed_in(&state, &caller)?;
    let folder = folder_id(&raw_folder)?;
    let endpoint = EndpointId::parse(&raw_endpoint).map_err(|_| ApiError::NotFound)?;
    if state
        .store
        .remove_folder_endpoint(user, folder, &endpoint)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::FolderNotFound)
    }
}

pub(crate) fn signed_in(state: &ApiState, caller: &Caller) -> Result<Uuid, ApiError> {
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    let rejection = if state.access.is_mixed() {
        AuthRejection::SignInRequired
    } else {
        AuthRejection::Unauthenticated
    };
    caller
        .user()
        .map(|user| user.id)
        .ok_or_else(|| ApiError::from(rejection))
}

fn folder_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::FolderNotFound)
}

fn parse_create(bytes: &[u8]) -> Result<String, ApiError> {
    let object = object_of(bytes)?;
    let mut violations = Vec::new();
    reject_unknown(&object, &["name"], &mut violations);
    let name = match object.get("name") {
        Some(value) => name_of(value, &mut violations),
        None => {
            violations.push(FieldViolation::new(
                "name",
                FieldCode::Required,
                "is required",
            ));
            None
        }
    };
    match name {
        Some(name) if violations.is_empty() => Ok(name),
        _ => Err(ApiError::Validation(violations)),
    }
}

fn parse_update(bytes: &[u8]) -> Result<FolderPatch, ApiError> {
    let object = object_of(bytes)?;
    let mut violations = Vec::new();
    reject_unknown(&object, &["name", "position"], &mut violations);
    let name = object
        .get("name")
        .and_then(|value| name_of(value, &mut violations));
    let position = object
        .get("position")
        .and_then(|value| position_of(value, &mut violations));
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    Ok(FolderPatch { name, position })
}

pub(crate) fn object_of(bytes: &[u8]) -> Result<Map<String, Value>, ApiError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        )),
    }
}

pub(crate) fn reject_unknown(
    object: &Map<String, Value>,
    known: &[&str],
    violations: &mut Vec<FieldViolation>,
) {
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            violations.push(FieldViolation::new(
                key.as_str(),
                FieldCode::NotAllowed,
                "is not a known field",
            ));
        }
    }
}

pub(crate) fn name_of(value: &Value, violations: &mut Vec<FieldViolation>) -> Option<String> {
    let Some(text) = value.as_str() else {
        violations.push(FieldViolation::new(
            "name",
            FieldCode::InvalidFormat,
            "must be a string",
        ));
        return None;
    };
    let length = text.chars().count();
    if length == 0 {
        violations.push(FieldViolation::new(
            "name",
            FieldCode::InvalidFormat,
            "must not be empty",
        ));
        None
    } else if length > NAME_MAX_CHARS {
        violations.push(FieldViolation::new(
            "name",
            FieldCode::TooLong,
            "must be at most 64 characters",
        ));
        None
    } else {
        Some(text.to_owned())
    }
}

fn position_of(value: &Value, violations: &mut Vec<FieldViolation>) -> Option<i32> {
    match value.as_i64().and_then(|number| i32::try_from(number).ok()) {
        Some(position) => Some(position),
        None => {
            violations.push(FieldViolation::new(
                "position",
                FieldCode::InvalidFormat,
                "must be an integer",
            ));
            None
        }
    }
}
