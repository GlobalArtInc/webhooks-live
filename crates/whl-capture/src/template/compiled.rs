use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use whl_store::captures::Endpoint;
use whl_template::{MAX_DIAGNOSTICS, Policy, RenderTarget, StepPolicy, Template};

pub const STATUS_FIELD: &str = "response.status_template";
pub const BODY_FIELD: &str = "response.body";

const CACHE_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    TemplateInvalid,
    JsonPathInvalid,
    SecretNotAllowed,
    StepUnknown,
    NotUtf8,
}

impl ViolationKind {
    fn from_field_code(code: &str) -> Self {
        match code {
            "json_path_invalid" => Self::JsonPathInvalid,
            "secret_not_allowed" => Self::SecretNotAllowed,
            "step_unknown" => Self::StepUnknown,
            _ => Self::TemplateInvalid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateViolation {
    pub field: String,
    pub kind: ViolationKind,
    pub message: String,
    pub position: Option<SourcePosition>,
}

#[derive(Debug, Clone)]
pub struct ResponseSources<'a> {
    pub status: Option<&'a str>,
    pub headers: Vec<(&'a str, &'a str)>,
    pub body: &'a [u8],
}

#[derive(Debug)]
pub(crate) struct CompiledHeader {
    pub index: usize,
    pub name: String,
    pub template: Template,
}

#[derive(Debug)]
pub(crate) struct CompiledResponse {
    pub status: Option<Template>,
    pub headers: Vec<CompiledHeader>,
    pub body: Template,
    pub counters: BTreeSet<String>,
    pub store: BTreeSet<String>,
}

fn policy(target: RenderTarget) -> Policy {
    let mut policy = Policy::response(target);
    policy.steps = StepPolicy::Only(BTreeSet::new());
    policy
}

pub(crate) fn header_field(index: usize) -> String {
    format!("response.headers.{index}.value")
}

fn parse(
    source: &str,
    target: RenderTarget,
    field: String,
    violations: &mut Vec<TemplateViolation>,
) -> Option<Template> {
    match Template::parse(source, &policy(target)) {
        Ok(template) => Some(template),
        Err(error) => {
            for diagnostic in error.diagnostics {
                if violations.len() >= MAX_DIAGNOSTICS {
                    break;
                }
                violations.push(TemplateViolation {
                    field: field.clone(),
                    kind: ViolationKind::from_field_code(diagnostic.code.field_code()),
                    message: diagnostic.message,
                    position: Some(SourcePosition {
                        line: diagnostic.position.line,
                        column: diagnostic.position.column,
                    }),
                });
            }
            None
        }
    }
}

pub(crate) fn compile(
    sources: &ResponseSources<'_>,
) -> Result<CompiledResponse, Vec<TemplateViolation>> {
    let mut violations = Vec::new();
    let status = sources.status.and_then(|source| {
        parse(
            source,
            RenderTarget::Status,
            STATUS_FIELD.to_owned(),
            &mut violations,
        )
    });
    let mut headers = Vec::with_capacity(sources.headers.len());
    for (index, (name, value)) in sources.headers.iter().enumerate() {
        if let Some(template) = parse(
            value,
            RenderTarget::HeaderValue,
            header_field(index),
            &mut violations,
        ) {
            headers.push(CompiledHeader {
                index,
                name: (*name).to_owned(),
                template,
            });
        }
    }
    let body = match std::str::from_utf8(sources.body) {
        Ok(text) => parse(
            text,
            RenderTarget::Body,
            BODY_FIELD.to_owned(),
            &mut violations,
        ),
        Err(_) => {
            violations.push(TemplateViolation {
                field: BODY_FIELD.to_owned(),
                kind: ViolationKind::NotUtf8,
                message: "must be UTF-8 text when response templating is on".to_owned(),
                position: None,
            });
            None
        }
    };
    let Some(body) = body else {
        return Err(violations);
    };
    if !violations.is_empty() {
        return Err(violations);
    }
    let mut counters = BTreeSet::new();
    let mut store = BTreeSet::new();
    let templates = status
        .iter()
        .chain(headers.iter().map(|header| &header.template))
        .chain(std::iter::once(&body));
    for template in templates {
        counters.extend(template.referenced_counters());
        store.extend(template.referenced_store());
    }
    Ok(CompiledResponse {
        status,
        headers,
        body,
        counters,
        store,
    })
}

impl CompiledResponse {
    pub(crate) fn variable_names(&self) -> Vec<String> {
        self.counters.union(&self.store).cloned().collect()
    }

    fn matches(&self, endpoint: &Endpoint) -> bool {
        self.status.as_ref().map(Template::source) == endpoint.status_template.as_deref()
            && self.body.source().as_bytes() == endpoint.body.as_slice()
            && self.headers.len() == endpoint.headers.len()
            && self.headers.iter().all(|header| {
                endpoint
                    .headers
                    .get(header.index)
                    .is_some_and(|stored| stored.name == header.name)
                    && endpoint
                        .headers
                        .get(header.index)
                        .is_some_and(|stored| stored.value == header.template.source())
            })
    }
}

#[derive(Debug, Default)]
struct CacheEntries {
    map: HashMap<String, Arc<CompiledResponse>>,
    order: VecDeque<String>,
}

#[derive(Debug, Default)]
pub(crate) struct TemplateCache {
    entries: Mutex<CacheEntries>,
}

impl TemplateCache {
    pub(crate) fn lookup(&self, endpoint: &Endpoint) -> Option<Arc<CompiledResponse>> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .map
            .get(endpoint.id.unpack())
            .filter(|compiled| compiled.matches(endpoint))
            .cloned()
    }

    pub(crate) fn store(&self, endpoint: &Endpoint, compiled: Arc<CompiledResponse>) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let key = endpoint.id.unpack();
        if entries.map.insert(key.to_owned(), compiled).is_none() {
            entries.order.push_back(key.to_owned());
            if entries.order.len() > CACHE_CAPACITY
                && let Some(oldest) = entries.order.pop_front()
            {
                entries.map.remove(&oldest);
            }
        }
    }
}
