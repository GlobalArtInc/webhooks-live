use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::{OpenApi, ToSchema};
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_store::{CreateFolderOutcome, FolderRecord, UpdateFolderOutcome, Uuid};

use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
    ValidationFailed,
};
use crate::routing::{Routes, get, patch, put};
use crate::state::ApiState;

const NAME_MAX_CHARS: usize = 64;

#[derive(OpenApi)]
#[openapi(
    paths(list, create, update, destroy, add_endpoint, remove_endpoint),
    components(schemas(FolderInputDoc, FolderPatchInputDoc, FolderDto, FolderListDto)),
    tags((name = "Folders", description = "The signed-in user's folders, each grouping endpoints"))
)]
pub struct FoldersDoc;

#[derive(Serialize, ToSchema)]
#[schema(as = Folder)]
pub struct FolderDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = "Checkout")]
    name: String,
    #[schema(example = 0)]
    position: i32,
    endpoint_ids: Vec<String>,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = FolderList)]
pub struct FolderListDto {
    items: Vec<FolderDto>,
}

struct FolderPatch {
    name: Option<String>,
    position: Option<i32>,
}

#[derive(ToSchema)]
#[schema(as = FolderInput)]
#[allow(dead_code)]
struct FolderInputDoc {
    #[schema(min_length = 1, max_length = 64, example = "Checkout")]
    name: String,
}

#[derive(ToSchema)]
#[schema(as = FolderPatchInput)]
#[allow(dead_code)]
struct FolderPatchInputDoc {
    #[schema(min_length = 1, max_length = 64, example = "Checkout v2")]
    name: Option<String>,
    #[schema(example = 1)]
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

#[utoipa::path(
    get,
    path = "/folders",
    operation_id = "folders_list",
    tag = "Folders",
    summary = "List the caller's folders",
    description = "Each folder lists its endpoint ids. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The caller's folders", body = FolderListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    post,
    path = "/folders",
    operation_id = "folders_create",
    tag = "Folders",
    summary = "Create a folder",
    description = "`name` is 1 to 64 characters and unique per user. The tier caps the number of folders. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    request_body(content = FolderInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The folder, with no endpoints yet", body = FolderDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 409, description = "`name_taken`: the user already has a folder with this name", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`) or `limit_reached` (`error.limit` = `{name: \"folders\", max}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    patch,
    path = "/folders/{folder_id}",
    operation_id = "folders_update",
    tag = "Folders",
    summary = "Rename or move a folder",
    description = "`name` (1 to 64 characters, unique per user) and `position` (integer, 32-bit) are both optional. An empty body is 400 `invalid_json`. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("folder_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Folder id, UUID")),
    request_body(content = FolderPatchInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The updated folder", body = FolderDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`folder_not_found`: not a folder of the caller, unknown or malformed id", body = ErrorBody),
        (status = 409, description = "`name_taken`: the user already has a folder with this name", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/folders/{folder_id}",
    operation_id = "folders_delete",
    tag = "Folders",
    summary = "Delete a folder",
    description = "Deletes the folder only; its endpoints are kept. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("folder_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Folder id, UUID")),
    responses(
        (status = 204, description = "The folder is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`folder_not_found`: not a folder of the caller, unknown or malformed id", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    put,
    path = "/folders/{folder_id}/endpoints/{id}",
    operation_id = "folders_add_endpoint",
    tag = "Folders",
    summary = "Add an endpoint to a folder",
    description = "Needs a folder of the caller and the viewer role on the endpoint. Adding an endpoint that is already in the folder changes nothing. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("folder_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Folder id, UUID"), EndpointIdParam),
    responses(
        (status = 204, description = "The endpoint is in the folder"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`folder_not_found`: not a folder of the caller, or `not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/folders/{folder_id}/endpoints/{id}",
    operation_id = "folders_remove_endpoint",
    tag = "Folders",
    summary = "Remove an endpoint from a folder",
    description = "Takes the endpoint out of the folder; the endpoint itself is kept, and no role on it is needed. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("folder_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Folder id, UUID"), EndpointIdParam),
    responses(
        (status = 204, description = "The endpoint is out of the folder"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`folder_not_found`: not a folder of the caller, or `not_found`: malformed endpoint id", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
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
