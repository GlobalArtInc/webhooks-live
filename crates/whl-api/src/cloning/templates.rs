use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use whl_auth::{Caller, EndpointRole, decide_access};
use whl_core::endpoint_id::EndpointId;
use whl_store::Uuid;
use whl_store::templates::{
    CreateTemplateOutcome, RenameTemplateOutcome, TemplateRecord, TemplateSummary,
};

use super::doc::{TemplateInputDoc, TemplatePatchInputDoc};
use super::document::ConfigDocument;
use super::export::{ChannelUrls, Selection, export};
use super::handlers::{create_from_document, sign_in_rejection};
use super::prepare::Overrides;
use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::folders::{name_of, object_of, reject_unknown};
use crate::input::parse_endpoint_input;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, Unauthenticated, UnsupportedMediaType,
    ValidationFailed,
};
use crate::state::ApiState;

const TEMPLATE_ID_KEY: &str = "template_id";

pub struct TemplateRequest {
    template_id: String,
    rest: Vec<u8>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = Template)]
pub struct TemplateDto {
    #[schema(example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70")]
    id: String,
    #[schema(example = "Staging")]
    name: String,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
    updated_at: String,
    #[schema(value_type = Object)]
    config: Value,
}

impl From<&TemplateRecord> for TemplateDto {
    fn from(record: &TemplateRecord) -> Self {
        Self {
            id: record.id.hyphenated().to_string(),
            name: record.name.clone(),
            created_at: timestamp(record.created_at),
            updated_at: timestamp(record.updated_at),
            config: record.config.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = TemplateSummary)]
pub struct TemplateSummaryDto {
    #[schema(example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70")]
    id: String,
    #[schema(example = "Staging")]
    name: String,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
    updated_at: String,
}

impl From<&TemplateSummary> for TemplateSummaryDto {
    fn from(summary: &TemplateSummary) -> Self {
        Self {
            id: summary.id.hyphenated().to_string(),
            name: summary.name.clone(),
            created_at: timestamp(summary.created_at),
            updated_at: timestamp(summary.updated_at),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = TemplateList)]
pub struct TemplateListDto {
    items: Vec<TemplateSummaryDto>,
}

pub fn split_template_request(body: &[u8]) -> Result<Option<TemplateRequest>, ApiError> {
    if !body
        .windows(TEMPLATE_ID_KEY.len())
        .any(|window| window == TEMPLATE_ID_KEY.as_bytes())
    {
        return Ok(None);
    }
    let Ok(Value::Object(mut object)) = serde_json::from_slice::<Value>(body) else {
        return Ok(None);
    };
    let Some(template_id) = object.remove(TEMPLATE_ID_KEY) else {
        return Ok(None);
    };
    let Value::String(template_id) = template_id else {
        return Err(ApiError::Validation(vec![FieldViolation::new(
            TEMPLATE_ID_KEY,
            FieldCode::InvalidFormat,
            "must be a template id",
        )]));
    };
    let rest = serde_json::to_vec(&Value::Object(object))
        .map_err(|_| ApiError::InternalFault("the endpoint input could not be encoded"))?;
    Ok(Some(TemplateRequest { template_id, rest }))
}

pub async fn create_from_template(
    state: &ApiState,
    caller: &Caller,
    request: TemplateRequest,
) -> Result<Response, ApiError> {
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    let user = caller
        .user()
        .map(|user| user.id)
        .ok_or_else(|| ApiError::from(sign_in_rejection(state)))?;
    let id = template_id(&request.template_id)?;
    let record = state
        .store
        .get_template(user, id)
        .await?
        .ok_or(ApiError::TemplateNotFound)?;
    let document = ConfigDocument::parse(&record.config)?;
    let overrides = Overrides::from_input(parse_endpoint_input(&request.rest, true)?);
    let creation = state.access.creation(caller)?;
    create_from_document(
        state,
        caller,
        &creation,
        &document,
        Selection::TEMPLATE,
        overrides,
        Vec::new(),
    )
    .await
}

#[utoipa::path(
    get,
    path = "/templates",
    operation_id = "cloning_templates_list",
    tag = "Cloning",
    summary = "List the caller's templates",
    description = "Lists the caller's templates without `config`; `GET /templates/{template_id}` returns the document. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The caller's templates", body = TemplateListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<TemplateListDto>, ApiError> {
    let user = signed_in(&state, &caller)?;
    let templates = state.store.list_templates(user).await?;
    Ok(Json(TemplateListDto {
        items: templates.iter().map(TemplateSummaryDto::from).collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/templates/{template_id}",
    operation_id = "cloning_templates_get",
    tag = "Cloning",
    summary = "Get a template with its configuration document",
    description = "`config` is the configuration document: `{version: 1, endpoint: {title, response, cors, overflow, tags, retention, capture_ttl_seconds, verifier, max_captures}, forward_rules: [{id, enabled, target_url, method, content_type, header_allowlist, append_path}], rules: [{name, enabled, condition, steps}], channels: [], schedules: [{name, cron, timezone, request, enabled: false}], secret_names: [string]}`. `channels` is always empty; `forward_rules` is empty when forwarding is off on the instance, `schedules` when schedules are off. Secret values and channel URLs are never in it; `secret_names` lists the secrets it uses. A template of another user, an unknown id and a malformed id all answer 404 `template_not_found`. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("template_id" = String, Path, description = "Template id, UUID")),
    responses(
        (status = 200, description = "The template; `config` as described above", body = TemplateDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 404, description = "`template_not_found`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn show(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_template): Path<String>,
) -> Result<Json<TemplateDto>, ApiError> {
    let user = signed_in(&state, &caller)?;
    let id = template_id(&raw_template)?;
    let record = state
        .store
        .get_template(user, id)
        .await?
        .ok_or(ApiError::TemplateNotFound)?;
    Ok(Json(TemplateDto::from(&record)))
}

#[utoipa::path(
    post,
    path = "/templates",
    operation_id = "cloning_templates_create",
    tag = "Cloning",
    summary = "Create a template from an endpoint",
    description = "Stores the current configuration of `from_endpoint_id` as a template named `name` (1 to 64 characters, unique per user). The caller needs the viewer role on the source. A source the caller cannot read, or a view-protected one without a valid grant, answers 410 `endpoint_gone`. Channel URLs and secret values are not stored. At most 20 templates per user. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    request_body(content = TemplateInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The template; `config` as in GET", body = TemplateDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 409, description = "`name_taken`: the user already has a template with this name", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: the source is unknown, expired or deleted, or the caller has no viewer role on it", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`: `name`, `from_endpoint_id`, or the exported configuration) or `limit_reached` (`error.limit` = `{name: \"templates\", max: 20}`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    RawBody(body): RawBody,
) -> Result<(StatusCode, Json<TemplateDto>), ApiError> {
    let user = signed_in(&state, &caller)?;
    let (name, source) = parse_create(&body)?;
    let role = state.access.role_on(&caller, &source).await?;
    decide_access(role, EndpointRole::Viewer)
        .into_result()
        .map_err(ApiError::Access)?;
    if !state
        .access
        .view_grant_valid(&source, &headers, None)
        .await?
    {
        return Err(ApiError::EndpointGone);
    }
    let role = role.ok_or(ApiError::EndpointGone)?;
    let record = state
        .store
        .get_endpoint(&source, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let exported = export(
        &state,
        &record,
        Selection::TEMPLATE,
        role,
        ChannelUrls::Omit,
    )
    .await?;
    let config = exported.document.to_value();
    ConfigDocument::parse(&config)?;
    if config.to_string().contains("\\u0000") {
        return Err(ApiError::Validation(vec![FieldViolation::new(
            "config",
            FieldCode::InvalidFormat,
            "must not contain NUL characters",
        )]));
    }
    match state.store.create_template(user, &name, &config).await? {
        CreateTemplateOutcome::Created(created) => {
            Ok((StatusCode::CREATED, Json(TemplateDto::from(&created))))
        }
        CreateTemplateOutcome::NameTaken => Err(ApiError::TemplateNameTaken),
        CreateTemplateOutcome::LimitReached => Err(ApiError::TemplateLimit),
    }
}

#[utoipa::path(
    patch,
    path = "/templates/{template_id}",
    operation_id = "cloning_templates_update",
    tag = "Cloning",
    summary = "Rename a template",
    description = "Only `name` changes (1 to 64 characters, unique per user); the configuration document is kept. An empty body is 400 `invalid_json`. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("template_id" = String, Path, description = "Template id, UUID")),
    request_body(content = TemplatePatchInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The renamed template", body = TemplateDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`template_not_found`", body = ErrorBody),
        (status = 409, description = "`name_taken`: the user already has a template with this name", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn rename(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_template): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<TemplateDto>, ApiError> {
    let user = signed_in(&state, &caller)?;
    let id = template_id(&raw_template)?;
    let name = parse_rename(&body)?;
    match state.store.rename_template(user, id, &name).await? {
        RenameTemplateOutcome::Renamed(record) => Ok(Json(TemplateDto::from(&record))),
        RenameTemplateOutcome::NameTaken => Err(ApiError::TemplateNameTaken),
        RenameTemplateOutcome::NotFound => Err(ApiError::TemplateNotFound),
    }
}

#[utoipa::path(
    delete,
    path = "/templates/{template_id}",
    operation_id = "cloning_templates_delete",
    tag = "Cloning",
    summary = "Delete a template",
    description = "A template of another user, an unknown id and a malformed id all answer 404 `template_not_found`. Needs a signed-in caller; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(("template_id" = String, Path, description = "Template id, UUID")),
    responses(
        (status = 204, description = "The template is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: `mixed` without a session; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`template_not_found`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn destroy(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_template): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = signed_in(&state, &caller)?;
    let id = template_id(&raw_template)?;
    if state.store.delete_template(user, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::TemplateNotFound)
    }
}

fn signed_in(state: &ApiState, caller: &Caller) -> Result<Uuid, ApiError> {
    crate::folders::signed_in(state, caller)
}

fn template_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw).map_err(|_| ApiError::TemplateNotFound)
}

fn parse_create(bytes: &[u8]) -> Result<(String, EndpointId), ApiError> {
    let object = object_of(bytes)?;
    let mut violations = Vec::new();
    reject_unknown(&object, &["name", "from_endpoint_id"], &mut violations);
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
    let source = match object.get("from_endpoint_id") {
        Some(Value::String(text)) => match EndpointId::parse(text) {
            Ok(source) => Some(source),
            Err(_) => {
                violations.push(FieldViolation::new(
                    "from_endpoint_id",
                    FieldCode::InvalidFormat,
                    "must be an endpoint id",
                ));
                None
            }
        },
        Some(_) => {
            violations.push(FieldViolation::new(
                "from_endpoint_id",
                FieldCode::InvalidFormat,
                "must be an endpoint id",
            ));
            None
        }
        None => {
            violations.push(FieldViolation::new(
                "from_endpoint_id",
                FieldCode::Required,
                "is required",
            ));
            None
        }
    };
    match (name, source) {
        (Some(name), Some(source)) if violations.is_empty() => Ok((name, source)),
        _ => Err(ApiError::Validation(violations)),
    }
}

fn parse_rename(bytes: &[u8]) -> Result<String, ApiError> {
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
