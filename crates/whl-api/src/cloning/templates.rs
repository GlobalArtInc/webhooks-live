use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use serde_json::Value;
use whl_auth::{Caller, EndpointRole, decide_access};
use whl_core::endpoint_id::EndpointId;
use whl_store::Uuid;
use whl_store::templates::{
    CreateTemplateOutcome, RenameTemplateOutcome, TemplateRecord, TemplateSummary,
};

use super::document::ConfigDocument;
use super::export::{ChannelUrls, Selection, export};
use super::handlers::{create_from_document, sign_in_rejection};
use super::prepare::Overrides;
use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::folders::{name_of, object_of, reject_unknown};
use crate::input::parse_endpoint_input;
use crate::state::ApiState;

const TEMPLATE_ID_KEY: &str = "template_id";

pub struct TemplateRequest {
    template_id: String,
    rest: Vec<u8>,
}

#[derive(Serialize)]
pub struct TemplateDto {
    id: String,
    name: String,
    created_at: String,
    updated_at: String,
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

#[derive(Serialize)]
pub struct TemplateSummaryDto {
    id: String,
    name: String,
    created_at: String,
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

#[derive(Serialize)]
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
