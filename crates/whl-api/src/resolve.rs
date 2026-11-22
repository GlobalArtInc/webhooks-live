use whl_capture::{ResponseSources, TemplateViolation, ViolationKind, validate_response_templates};
use whl_core::header::HeaderInput;
use whl_core::response::{ResponseConfig, ResponseConfigInput};
use whl_core::validation::ValidationError;
use whl_store::{EndpointSettings, Overflow, ResponseTemplating, StoredVerifier};
use whl_verify::{ConfigIssue, ConfigProblem, Verifier};

use crate::error::{FieldCode, FieldViolation};
use crate::input::{EndpointPatch, ResponsePatch};
use crate::settings::ApiSettings;

pub fn defaults() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        templating: ResponseTemplating::default(),
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
    let templating = ResponseTemplating {
        enabled: patch.response.templated.unwrap_or(base.templating.enabled),
        status: patch
            .response
            .status_template
            .clone()
            .unwrap_or_else(|| base.templating.status.clone()),
    };
    match validate_response(&base.response, &patch.response) {
        Ok(response) => {
            if templating.enabled {
                violations.extend(template_violations(&response, &templating));
            }
            if violations.is_empty() {
                Ok(EndpointSettings {
                    title,
                    response,
                    templating,
                    cors,
                    overflow,
                    tags,
                    verifier,
                })
            } else {
                Err(violations)
            }
        }
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

pub(crate) fn validate_response(
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

fn template_violations(
    response: &ResponseConfig,
    templating: &ResponseTemplating,
) -> Vec<FieldViolation> {
    let sources = ResponseSources {
        status: templating.status.as_deref(),
        headers: response
            .headers()
            .iter()
            .map(|header| (header.name.unpack(), header.value.unpack()))
            .collect(),
        body: response.body(),
    };
    match validate_response_templates(&sources) {
        Ok(()) => Vec::new(),
        Err(violations) => violations.iter().map(field_violation).collect(),
    }
}

fn field_violation(violation: &TemplateViolation) -> FieldViolation {
    let code = match violation.kind {
        ViolationKind::TemplateInvalid => FieldCode::TemplateInvalid,
        ViolationKind::JsonPathInvalid => FieldCode::JsonPathInvalid,
        ViolationKind::SecretNotAllowed => FieldCode::SecretNotAllowed,
        ViolationKind::StepUnknown => FieldCode::StepUnknown,
        ViolationKind::NotUtf8 => FieldCode::InvalidFormat,
    };
    let built = FieldViolation::new(violation.field.as_str(), code, violation.message.as_str());
    match violation.position {
        Some(position) => built.at(position.line, position.column),
        None => built,
    }
}
