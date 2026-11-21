use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_forward::vet_target;
use whl_store::Uuid;
use whl_store::forward::{
    CreateRuleOutcome, CustomTarget, DeleteRuleOutcome, ForwardRulePatch, ManualJobOutcome,
    ManualTarget, NewForwardRule, UpdateRuleOutcome,
};

use super::dto::{JobDto, RuleDto, RuleListDto};
use super::input::{parse_request, parse_rule, target_violation};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<RuleListDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let rules = state.store.list_forward_rules(&endpoint).await?;
    Ok(Json(RuleListDto {
        items: rules.iter().map(RuleDto::from).collect(),
    }))
}

pub async fn create(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let parsed = parse_rule(&body, true)?;
    let mut violations = parsed.violations;
    let fields = parsed.fields;
    check_target(&state, fields.target_url.as_deref(), &mut violations).await;
    let Some(target_url) = fields.target_url.filter(|_| violations.is_empty()) else {
        return Err(ApiError::Validation(violations));
    };
    let rule = NewForwardRule {
        enabled: fields.enabled.unwrap_or(true),
        target_url,
        method: fields.method.flatten(),
        content_type: fields.content_type.flatten(),
        header_allowlist: fields.header_allowlist.unwrap_or_default(),
        append_path: fields.append_path.unwrap_or(false),
    };
    match state
        .store
        .create_forward_rule(&endpoint, state.settings.retention, &rule)
        .await?
    {
        CreateRuleOutcome::Created(created) => {
            Ok((StatusCode::CREATED, Json(RuleDto::from(&created))).into_response())
        }
        CreateRuleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        CreateRuleOutcome::LimitReached => Err(ApiError::ForwardRuleLimit),
    }
}

pub async fn update(
    State(state): State<ApiState>,
    Path((raw_id, raw_rule)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<RuleDto>, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let rule_id = rule_id(&raw_rule)?;
    live_endpoint(&state, &endpoint).await?;
    let parsed = parse_rule(&body, false)?;
    let mut violations = parsed.violations;
    let patch: ForwardRulePatch = parsed.fields;
    check_target(&state, patch.target_url.as_deref(), &mut violations).await;
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    match state
        .store
        .update_forward_rule(&endpoint, state.settings.retention, rule_id, &patch)
        .await?
    {
        UpdateRuleOutcome::Updated(updated) => Ok(Json(RuleDto::from(&updated))),
        UpdateRuleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        UpdateRuleOutcome::RuleNotFound => Err(ApiError::ForwardRuleNotFound),
    }
}

pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_rule)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let rule_id = rule_id(&raw_rule)?;
    match state
        .store
        .delete_forward_rule(&endpoint, state.settings.retention, rule_id)
        .await?
    {
        DeleteRuleOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteRuleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteRuleOutcome::RuleNotFound => Err(ApiError::ForwardRuleNotFound),
    }
}

pub async fn resend(
    State(state): State<ApiState>,
    Path((raw_id, raw_capture)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let capture = CaptureId::parse(&raw_capture).map_err(|_| ApiError::NotFound)?;
    live_endpoint(&state, &endpoint).await?;
    let parsed = parse_request(&body)?;
    let mut violations = parsed.violations;
    let fields = parsed.fields;
    if parsed.rule_id.is_none() {
        check_target(&state, fields.target_url.as_deref(), &mut violations).await;
    }
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    let target = match (parsed.rule_id, fields.target_url) {
        (Some(rule_id), _) => ManualTarget::Rule(rule_id),
        (None, Some(target_url)) => ManualTarget::Custom(CustomTarget {
            target_url,
            method: fields.method.flatten(),
            content_type: fields.content_type.flatten(),
            header_allowlist: fields.header_allowlist.unwrap_or_default(),
        }),
        (None, None) => {
            return Err(ApiError::Validation(vec![FieldViolation::new(
                "target_url",
                FieldCode::Required,
                "is required",
            )]));
        }
    };
    match state
        .store
        .create_manual_job(
            &endpoint,
            state.settings.retention,
            capture.unpack(),
            &target,
        )
        .await?
    {
        ManualJobOutcome::Created(job) => {
            Ok((StatusCode::ACCEPTED, Json(JobDto::from(&job))).into_response())
        }
        ManualJobOutcome::EndpointGone => Err(ApiError::EndpointGone),
        ManualJobOutcome::CaptureNotFound => Err(ApiError::CaptureNotFound),
        ManualJobOutcome::RuleNotFound => Err(ApiError::ForwardRuleNotFound),
    }
}

async fn check_target(
    state: &ApiState,
    target: Option<&str>,
    violations: &mut Vec<FieldViolation>,
) {
    let Some(target) = target else {
        return;
    };
    let vetted = vet_target(target, &state.forward.guard, state.forward.lookup.as_ref()).await;
    if let Err(error) = vetted {
        violations.push(target_violation(&error));
    }
}

fn ensure_enabled(state: &ApiState) -> Result<(), ApiError> {
    if state.settings.forwarding {
        Ok(())
    } else {
        Err(ApiError::ForwardingDisabled)
    }
}

async fn live_endpoint(state: &ApiState, endpoint: &EndpointId) -> Result<(), ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .map(|_| ())
        .ok_or(ApiError::EndpointGone)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

fn rule_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::NotFound)
}
