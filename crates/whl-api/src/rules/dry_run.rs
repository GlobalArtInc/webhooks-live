use axum::Json;
use axum::extract::{Path, State};
use bytes::Bytes;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_capture::{DryRunError, DryRunInput, DryRunRules, RULE_BODY_BYTES, RuleFacts};
use whl_core::capture_id::CaptureId;
use whl_rules::{MAX_RULES_PER_ENDPOINT, RunView, StoredRule};
use whl_store::CaptureDetail;
use whl_store::rules::RuleSet;
use whl_template::{NameValue, SignatureState};
use whl_verify::Verification;

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

fn pairs(source: &[whl_store::NameValue]) -> Vec<NameValue> {
    source
        .iter()
        .map(|pair| NameValue::new(pair.name.as_str(), pair.value.as_str()))
        .collect()
}

fn meta_pairs(meta: &Value) -> Vec<NameValue> {
    let Value::Object(map) = meta else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, value)| match value {
            Value::String(text) => Some(NameValue::new(name.as_str(), text.as_str())),
            Value::Number(number) => Some(NameValue::new(name.as_str(), number.to_string())),
            Value::Bool(flag) => Some(NameValue::new(name.as_str(), flag.to_string())),
            _ => None,
        })
        .collect()
}

fn signature_of(verification: Option<Verification>) -> SignatureState {
    match verification {
        Some(Verification::Valid) => SignatureState::Valid,
        Some(Verification::Invalid) => SignatureState::Invalid,
        Some(Verification::Missing) | None => SignatureState::Missing,
    }
}

fn facts_of(
    detail: &CaptureDetail,
    state: &ApiState,
    endpoint: &str,
    title: Option<&str>,
) -> Result<RuleFacts, ApiError> {
    let summary = &detail.summary;
    let received_at = OffsetDateTime::parse(&summary.received_at, &Rfc3339)
        .map_err(|_| ApiError::InternalFault("a stored capture time cannot be read"))?;
    Ok(RuleFacts {
        endpoint_id: endpoint.to_owned(),
        endpoint_url: format!("{}/{endpoint}", state.settings.public_url),
        endpoint_title: title.unwrap_or_default().to_owned(),
        capture_id: summary.id.clone(),
        capture_seq: summary.seq,
        received_at,
        kind: summary.kind.as_str().to_owned(),
        method: summary.method.clone(),
        path: summary.path.clone(),
        query: pairs(&detail.query),
        headers: pairs(&detail.headers),
        form: detail
            .form
            .as_ref()
            .map(|form| {
                form.fields
                    .iter()
                    .map(|field| NameValue::new(field.name.as_str(), field.value.as_str()))
                    .collect()
            })
            .unwrap_or_default(),
        meta: meta_pairs(&detail.meta),
        body: Bytes::copy_from_slice(
            detail
                .body
                .prefix
                .get(..RULE_BODY_BYTES)
                .unwrap_or(&detail.body.prefix),
        ),
        ip: summary.client_ip.clone(),
        content_type: summary.content_type.clone().unwrap_or_default(),
        signature: signature_of(summary.verification),
    })
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
    let facts = facts_of(
        &detail,
        &state,
        endpoint.unpack(),
        record.settings.title.as_deref(),
    )?;
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
