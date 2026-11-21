use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Map, Value};
use whl_capture::{DryRunError, DryRunInput, DryRunRules, RuleFacts};
use whl_core::capture_id::CaptureId;
use whl_rules::{MAX_RULES_PER_ENDPOINT, RunView, StoredRule};
use whl_store::rules::RuleSet;

use super::handlers::{endpoint_id, references};
use super::input::{parse_object, violations};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const FIELDS: [&str; 3] = ["capture_id", "rules", "execute_http"];

struct DryRunRequest {
    capture: CaptureId,
    drafts: Option<Vec<Value>>,
}

fn invalid(field: &str, code: FieldCode, message: &str) -> ApiError {
    ApiError::Validation(vec![FieldViolation::new(field, code, message)])
}

fn parse_request(object: &Map<String, Value>) -> Result<DryRunRequest, ApiError> {
    if let Some(key) = object.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(invalid(key, FieldCode::NotAllowed, "is not a known field"));
    }
    let capture = match object.get("capture_id") {
        Some(Value::String(text)) => CaptureId::parse(text).map_err(|_| {
            invalid(
                "capture_id",
                FieldCode::InvalidFormat,
                "must be a capture id",
            )
        })?,
        Some(Value::Null) | None => {
            return Err(invalid("capture_id", FieldCode::Required, "is required"));
        }
        Some(_) => {
            return Err(invalid(
                "capture_id",
                FieldCode::InvalidFormat,
                "must be a string",
            ));
        }
    };
    if object
        .get("execute_http")
        .is_some_and(|value| !value.is_boolean() && !value.is_null())
    {
        return Err(invalid(
            "execute_http",
            FieldCode::InvalidFormat,
            "must be a boolean",
        ));
    }
    let drafts = match object.get("rules") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) if items.len() <= MAX_RULES_PER_ENDPOINT => Some(items.clone()),
        Some(Value::Array(_)) => {
            return Err(invalid(
                "rules",
                FieldCode::TooMany,
                &format!("must have at most {MAX_RULES_PER_ENDPOINT} rules"),
            ));
        }
        Some(_) => {
            return Err(invalid(
                "rules",
                FieldCode::InvalidFormat,
                "must be an array",
            ));
        }
    };
    Ok(DryRunRequest { capture, drafts })
}

fn saved_rules(set: &RuleSet) -> Vec<StoredRule> {
    set.rules
        .iter()
        .map(|rule| StoredRule {
            id: rule.id,
            name: rule.name.clone(),
            enabled: rule.enabled,
            condition: rule.condition.clone(),
            steps: rule.steps.clone(),
        })
        .collect()
}

pub async fn dry_run(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<RunView>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let request = parse_request(&parse_object(&body)?)?;
    let runtime = state.rules.as_ref().ok_or(ApiError::RulesDisabled)?;
    let record = state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let detail = state
        .store
        .get_capture(&endpoint, &request.capture)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    let facts = RuleFacts::from_detail(
        &detail,
        &state.settings.public_url,
        record.settings.title.as_deref(),
    )
    .map_err(|_| ApiError::InternalFault("a stored capture time cannot be read"))?;
    let set = state.store.list_rules(&endpoint).await?;
    let rules = match request.drafts {
        Some(rules) => DryRunRules::Drafts {
            rules,
            references: references(&state, &endpoint).await?,
        },
        None => DryRunRules::Saved(saved_rules(&set)),
    };
    let view = runtime
        .dry_run(
            &state.store,
            DryRunInput {
                endpoint,
                facts,
                rules,
                rules_version: set.version,
                capture_id: request.capture.unpack(),
            },
        )
        .await
        .map_err(|error| match error {
            DryRunError::Busy => ApiError::RateLimited {
                retry_after_secs: 1,
            },
            DryRunError::Disabled => ApiError::RulesDisabled,
            DryRunError::Invalid(error) => {
                ApiError::Validation(violations(error.diagnostics, None))
            }
            DryRunError::Stored(_) | DryRunError::Compile(_) | DryRunError::Internal => {
                ApiError::InternalFault("the dry run could not be prepared")
            }
        })?;
    Ok(Json(view))
}
