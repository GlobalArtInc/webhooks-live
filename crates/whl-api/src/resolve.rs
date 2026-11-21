use whl_core::header::HeaderInput;
use whl_core::response::{ResponseConfig, ResponseConfigInput};
use whl_core::validation::ValidationError;
use whl_store::{EndpointSettings, Overflow, StoredVerifier};
use whl_verify::{ConfigIssue, ConfigProblem, Verifier};

use crate::error::{FieldCode, FieldViolation};
use crate::input::{EndpointPatch, ResponsePatch};
use crate::settings::ApiSettings;

pub fn defaults() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
        verifier: None,
    }
}

pub fn resolve(
    settings: &ApiSettings,
    base: &EndpointSettings,
    patch: &EndpointPatch,
    mut violations: Vec<FieldViolation>,
) -> Result<EndpointSettings, Vec<FieldViolation>> {
    if patch.overflow == Some(Overflow::Ring) && !settings.ring_mode_allowed {
        violations.push(FieldViolation::new(
            "overflow",
            FieldCode::NotAllowed,
            "ring mode is not allowed on this instance",
        ));
    }
    let title = patch.title.clone().unwrap_or_else(|| base.title.clone());
    let cors = patch.cors.unwrap_or(base.cors);
    let overflow = patch.overflow.unwrap_or(base.overflow);
    let tags = patch.tags.clone().unwrap_or_else(|| base.tags.clone());
    let verifier = match &patch.verifier {
        None => base.verifier.clone(),
        Some(None) => None,
        Some(Some(input)) => match Verifier::new(input) {
            Ok(verifier) => Some(StoredVerifier::Configured(verifier)),
            Err(issues) => {
                violations.extend(issues.iter().map(verifier_violation));
                None
            }
        },
    };
    match validate_response(&base.response, &patch.response) {
        Ok(response) if violations.is_empty() => Ok(EndpointSettings {
            title,
            response,
            cors,
            overflow,
            tags,
            verifier,
        }),
        Ok(_) => Err(violations),
        Err(error) => {
            violations.extend(error.violations().iter().map(FieldViolation::from_core));
            Err(violations)
        }
    }
}

fn verifier_violation(issue: &ConfigIssue) -> FieldViolation {
    let code = match issue.problem {
        ConfigProblem::Required => FieldCode::Required,
        ConfigProblem::InvalidFormat => FieldCode::InvalidFormat,
        ConfigProblem::TooLong => FieldCode::TooLong,
        ConfigProblem::NotAllowed => FieldCode::NotAllowed,
    };
    FieldViolation::new(
        format!("verifier.{}", issue.field),
        code,
        issue.problem.to_string(),
    )
}

fn validate_response(
    base: &ResponseConfig,
    patch: &ResponsePatch,
) -> Result<ResponseConfig, ValidationError> {
    ResponseConfig::validate(ResponseConfigInput {
        status: patch
            .status
            .unwrap_or_else(|| i64::from(base.status().unpack())),
        content_type: patch
            .content_type
            .clone()
            .unwrap_or_else(|| base.content_type().unpack().to_owned()),
        headers: patch.headers.clone().unwrap_or_else(|| {
            base.headers()
                .iter()
                .map(|header| HeaderInput {
                    name: header.name.unpack().to_owned(),
                    value: header.value.unpack().to_owned(),
                })
                .collect()
        }),
        body: patch.body.clone().unwrap_or_else(|| base.body().to_vec()),
        delay_ms: patch
            .delay_ms
            .unwrap_or_else(|| i64::try_from(base.delay().as_millis()).unwrap_or(i64::MAX)),
    })
}
