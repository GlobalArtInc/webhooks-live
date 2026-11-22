use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value as JsonValue;
use time::OffsetDateTime;
use whl_template::{
    ChaChaRandom, Context, MAX_DIAGNOSTICS, MissingVariable, MonotonicClock, NameValue, RandomSeed,
    RenderEnv, RenderError, RenderErrorCode, RenderLimits, RenderOutcome, ResponseBudget,
    SignatureState, SystemClock, Template,
};

use super::compiled::{
    BODY_FIELD, CompiledResponse, ResponseSources, STATUS_FIELD, TemplateViolation, ViolationKind,
    compile, header_field,
};
use super::engine::DEFAULT_RESPONSE_BUDGET;
use super::render::{parse_status, validated_header};
use crate::rules::RuleFacts;

const CODE_INVALID_STATUS: &str = "invalid_status";
const CODE_INVALID_HEADER: &str = "invalid_header";
const CODE_VALUE_MISSING: &str = "value_missing";
const CODE_TEMPLATE_INVALID: &str = "template_invalid";
const CODE_JSON_PATH_INVALID: &str = "json_path_invalid";
const CODE_SECRET_NOT_ALLOWED: &str = "secret_not_allowed";
const CODE_STEP_UNKNOWN: &str = "step_unknown";
const CODE_INVALID_FORMAT: &str = "invalid_format";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDraft {
    pub status: u16,
    pub status_template: Option<String>,
    pub content_type: String,
    pub headers: Vec<NameValue>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSeverity {
    Error,
    Warning,
}

impl PreviewSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDiagnostic {
    pub field: String,
    pub code: String,
    pub severity: PreviewSeverity,
    pub line: u32,
    pub column: u32,
    pub length: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRendered {
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<NameValue>,
    pub body: String,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewReport {
    pub rendered: Option<PreviewRendered>,
    pub diagnostics: Vec<PreviewDiagnostic>,
}

fn add_persistent(context: &mut Context, persistent: &BTreeMap<String, JsonValue>) {
    for (name, value) in persistent {
        if let Some(counter) = value.as_i64() {
            context.counters.insert(name.clone(), counter);
        }
        context.store.insert(name.clone(), value.clone());
    }
}

pub fn response_context(facts: RuleFacts, persistent: &BTreeMap<String, JsonValue>) -> Context {
    let mut context = facts.into_context();
    context.request.meta.clear();
    context.request.signature = SignatureState::Missing;
    add_persistent(&mut context, persistent);
    context
}

pub fn persistent_context(persistent: &BTreeMap<String, JsonValue>) -> Context {
    let mut context = Context::new(OffsetDateTime::now_utc());
    add_persistent(&mut context, persistent);
    context
}

fn violation_code(kind: ViolationKind) -> &'static str {
    match kind {
        ViolationKind::TemplateInvalid => CODE_TEMPLATE_INVALID,
        ViolationKind::JsonPathInvalid => CODE_JSON_PATH_INVALID,
        ViolationKind::SecretNotAllowed => CODE_SECRET_NOT_ALLOWED,
        ViolationKind::StepUnknown => CODE_STEP_UNKNOWN,
        ViolationKind::NotUtf8 => CODE_INVALID_FORMAT,
    }
}

fn violation_diagnostic(violation: &TemplateViolation) -> PreviewDiagnostic {
    let (line, column, length) = violation
        .position
        .map_or((1, 1, 1), |at| (at.line, at.column, at.length.max(1)));
    PreviewDiagnostic {
        field: violation.field.clone(),
        code: violation_code(violation.kind).to_owned(),
        severity: PreviewSeverity::Error,
        line,
        column,
        length,
        message: violation.message.clone(),
    }
}

fn saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn render_diagnostic(
    field: &str,
    error: &RenderError,
    severity: PreviewSeverity,
) -> PreviewDiagnostic {
    PreviewDiagnostic {
        field: field.to_owned(),
        code: error.code.as_str().to_owned(),
        severity,
        line: error.position.line,
        column: error.position.column,
        length: saturating(error.position.length).max(1),
        message: error.message.clone(),
    }
}

fn missing_diagnostic(field: &str, missing: &MissingVariable) -> PreviewDiagnostic {
    PreviewDiagnostic {
        field: field.to_owned(),
        code: CODE_VALUE_MISSING.to_owned(),
        severity: PreviewSeverity::Warning,
        line: missing.position.line,
        column: missing.position.column,
        length: saturating(missing.position.length).max(1),
        message: format!("{} has no value and renders empty", missing.reference),
    }
}

fn plain_diagnostic(
    field: &str,
    code: &str,
    severity: PreviewSeverity,
    message: &str,
) -> PreviewDiagnostic {
    PreviewDiagnostic {
        field: field.to_owned(),
        code: code.to_owned(),
        severity,
        line: 1,
        column: 1,
        length: 1,
        message: message.to_owned(),
    }
}

enum Field {
    Text(String),
    Dropped,
    Failed,
}

struct Renderer<'a> {
    context: &'a Context,
    budget: ResponseBudget,
    clock: SystemClock,
    random: ChaChaRandom,
    diagnostics: Vec<PreviewDiagnostic>,
}

impl<'a> Renderer<'a> {
    fn new(context: &'a Context) -> Self {
        let clock = SystemClock::new();
        Self {
            context,
            budget: ResponseBudget::start(&clock, DEFAULT_RESPONSE_BUDGET),
            clock,
            random: ChaChaRandom::from_seed(&RandomSeed::from_bytes(context.capture.id.as_bytes())),
            diagnostics: Vec::new(),
        }
    }

    fn note(&mut self, diagnostic: PreviewDiagnostic) {
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics.push(diagnostic);
        }
    }

    fn run(&mut self, template: &Template, field: &str, drops_on_forbidden: bool) -> Field {
        let limits = RenderLimits::for_target(template.target());
        let mut env = RenderEnv::new(&mut self.random, &self.clock);
        let outcome = template.render_within(self.context, &limits, &mut env, &self.budget);
        for missing in outcome.missing() {
            self.note(missing_diagnostic(field, missing));
        }
        match outcome {
            RenderOutcome::Clean(rendered) => Field::Text(rendered.output),
            RenderOutcome::Partial(partial) => {
                let only_forbidden = partial
                    .errors
                    .iter()
                    .all(|error| error.code == RenderErrorCode::ForbiddenCharacter);
                let dropped = drops_on_forbidden && only_forbidden;
                let severity = if dropped {
                    PreviewSeverity::Warning
                } else {
                    PreviewSeverity::Error
                };
                for error in &partial.errors {
                    self.note(render_diagnostic(field, error, severity));
                }
                if dropped {
                    Field::Dropped
                } else {
                    Field::Failed
                }
            }
        }
    }

    fn status(&mut self, compiled: &CompiledResponse, fallback: u16) -> Option<u16> {
        let Some(template) = &compiled.status else {
            return Some(fallback);
        };
        match self.run(template, STATUS_FIELD, false) {
            Field::Text(text) => {
                if let Some(status) = parse_status(&text) {
                    Some(status.unpack())
                } else {
                    self.note(plain_diagnostic(
                        STATUS_FIELD,
                        CODE_INVALID_STATUS,
                        PreviewSeverity::Error,
                        "the status must render as three digits from 100 to 599",
                    ));
                    None
                }
            }
            Field::Dropped | Field::Failed => None,
        }
    }

    fn headers(&mut self, compiled: &CompiledResponse) -> Option<Vec<NameValue>> {
        let mut kept = Vec::with_capacity(compiled.headers.len());
        for header in &compiled.headers {
            let field = header_field(header.index);
            match self.run(&header.template, &field, true) {
                Field::Text(text) => match validated_header(&header.name, text) {
                    Some(valid) => kept.push(NameValue::new(valid.name, valid.value)),
                    None => self.note(plain_diagnostic(
                        &field,
                        CODE_INVALID_HEADER,
                        PreviewSeverity::Warning,
                        "the rendered header is not valid and is not sent",
                    )),
                },
                Field::Dropped => {}
                Field::Failed => return None,
            }
        }
        Some(kept)
    }
}

pub fn render_preview(draft: &TemplateDraft, context: Option<&Context>) -> PreviewReport {
    let headers = draft
        .headers
        .iter()
        .map(|pair| (pair.name.as_str(), pair.value.as_str()))
        .collect();
    let sources = ResponseSources {
        status: draft.status_template.as_deref(),
        headers,
        body: &draft.body,
    };
    let compiled = match compile(&sources) {
        Ok(compiled) => compiled,
        Err(violations) => {
            return PreviewReport {
                rendered: None,
                diagnostics: violations.iter().map(violation_diagnostic).collect(),
            };
        }
    };
    let Some(context) = context else {
        return PreviewReport {
            rendered: None,
            diagnostics: Vec::new(),
        };
    };
    let mut renderer = Renderer::new(context);
    let status = renderer.status(&compiled, draft.status);
    let body = match renderer.run(&compiled.body, BODY_FIELD, false) {
        Field::Text(text) => Some(text),
        Field::Dropped | Field::Failed => None,
    };
    let headers = renderer.headers(&compiled);
    let rendered = match (status, body, headers) {
        (Some(status), Some(body), Some(headers)) => Some(PreviewRendered {
            status,
            content_type: draft.content_type.clone(),
            headers,
            body,
            elapsed: renderer.clock.elapsed(),
        }),
        _ => None,
    };
    PreviewReport {
        rendered,
        diagnostics: renderer.diagnostics,
    }
}
