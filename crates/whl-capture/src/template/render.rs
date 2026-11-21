use std::collections::BTreeMap;
use std::sync::Arc;

use whl_core::header::{HeaderName, HeaderValue};
use whl_core::status::HttpStatus;
use whl_store::captures::{Endpoint, NameValue as StoredPair, NewCapture, StoredCapture};
use whl_store::vars::LoadedVars;
use whl_template::{
    BodyData, CaptureData, ChaChaRandom, Context, EndpointData, JSON_BODY_BYTES, NameValue,
    RandomSeed, RenderEnv, RenderErrorCode, RenderLimits, RenderOutcome, ResponseBudget,
    SystemClock, Template,
};

use super::compiled::{BODY_FIELD, CompiledResponse, STATUS_FIELD, header_field};
use super::report::{
    FallbackCode, MAX_RECORDED, Recorded, RenderedHeader, RenderedParts, Rendering, Served,
};

const KIND_HTTP: &str = "http";
const CODE_INVALID_HEADER: &str = "invalid_header";
const CODE_INVALID_STATUS: &str = "invalid_status";

pub(crate) struct RenderJob {
    pub compiled: Arc<CompiledResponse>,
    pub request: NewCapture,
    pub capture: StoredCapture,
    pub endpoint: EndpointFacts,
    pub vars: Option<LoadedVars>,
    pub seed: RandomSeed,
    pub budget: Arc<ResponseBudget>,
    pub clock: SystemClock,
    pub with_status: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct EndpointFacts {
    pub id: String,
    pub url: String,
    pub title: String,
}

impl EndpointFacts {
    pub(crate) fn new(endpoint: &Endpoint, public_url: &str) -> Self {
        Self {
            id: endpoint.id.unpack().to_owned(),
            url: format!("{public_url}/{}", endpoint.id.unpack()),
            title: endpoint.title.clone().unwrap_or_default(),
        }
    }
}

fn pairs(source: &[StoredPair]) -> Vec<NameValue> {
    source
        .iter()
        .map(|pair| NameValue::new(pair.name.as_str(), pair.value.as_str()))
        .collect()
}

fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

fn body_data(body: &[u8]) -> BodyData {
    let kept = body.get(..=JSON_BODY_BYTES).unwrap_or(body);
    BodyData::new(kept.to_vec())
}

fn build_context(job: &RenderJob) -> Context {
    let request = &job.request;
    let mut context = Context::new(job.capture.received_at);
    context.request.method.clone_from(&request.method);
    context.request.path.clone_from(&request.path);
    context.request.segments = segments(&request.path);
    context.request.query = pairs(&request.query);
    context.request.headers = pairs(&request.headers);
    if let Some(form) = &request.form {
        context.request.form = pairs(&form.fields);
    }
    context.request.body = body_data(&request.body);
    context.request.ip = request.client_ip.to_string();
    context.request.content_type = request.content_type.clone().unwrap_or_default();
    context.request.kind = KIND_HTTP.to_owned();
    context.capture = CaptureData {
        id: job.capture.id.to_string(),
        seq: job.capture.seq,
        received_at: job.capture.received_at,
    };
    context.endpoint = EndpointData {
        id: job.endpoint.id.clone(),
        url: job.endpoint.url.clone(),
        title: job.endpoint.title.clone(),
    };
    if let Some(vars) = &job.vars {
        let mut counters = BTreeMap::new();
        for name in &job.compiled.counters {
            if let Some(value) = vars.counter(name) {
                counters.insert(name.clone(), value);
            }
        }
        context.counters = counters;
        let mut store = BTreeMap::new();
        for name in &job.compiled.store {
            if let Some(value) = vars.values().get(name) {
                store.insert(name.clone(), value.clone());
            }
        }
        context.store = store;
    }
    context
}

struct Collected {
    recorded: Vec<Recorded>,
    truncated: bool,
}

impl Collected {
    fn note_code(&mut self, field: &str, code: &'static str) {
        if self.recorded.len() >= MAX_RECORDED {
            self.truncated = true;
            return;
        }
        self.recorded.push(Recorded {
            field: field.to_owned(),
            code,
            line: 1,
            column: 1,
        });
    }

    fn note(&mut self, field: &str, errors: &[whl_template::RenderError]) {
        for error in errors {
            if self.recorded.len() >= MAX_RECORDED {
                self.truncated = true;
                return;
            }
            self.recorded.push(Recorded {
                field: field.to_owned(),
                code: error.code.as_str(),
                line: error.position.line,
                column: error.position.column,
            });
        }
    }
}

fn is_time_limit(code: RenderErrorCode) -> Option<FallbackCode> {
    match code {
        RenderErrorCode::BudgetExhausted => Some(FallbackCode::Budget),
        RenderErrorCode::DeadlineExceeded => Some(FallbackCode::Deadline),
        _ => None,
    }
}

fn fallback_code(errors: &[whl_template::RenderError]) -> FallbackCode {
    if let Some(code) = errors.iter().find_map(|error| is_time_limit(error.code)) {
        return code;
    }
    if errors
        .iter()
        .any(|error| error.code == RenderErrorCode::InvalidStatus)
    {
        return FallbackCode::Status;
    }
    FallbackCode::Render
}

fn finish_fallback(collected: Collected, code: FallbackCode) -> Rendering {
    Rendering {
        served: Served::Fallback(code),
        recorded: collected.recorded,
        truncated: collected.truncated,
    }
}

pub(crate) fn run(job: &RenderJob) -> Rendering {
    let context = build_context(job);
    let mut random = ChaChaRandom::from_seed(&job.seed);
    let mut env = RenderEnv::new(&mut random, &job.clock);
    let mut collected = Collected {
        recorded: Vec::new(),
        truncated: false,
    };

    let render = |template: &Template, env: &mut RenderEnv<'_>| {
        let limits = RenderLimits::for_target(template.target());
        template.render_within(&context, &limits, env, &job.budget)
    };

    let mut status = None;
    if job.with_status
        && let Some(template) = &job.compiled.status
    {
        match render(template, &mut env) {
            RenderOutcome::Clean(rendered) => match parse_status(&rendered.output) {
                Some(parsed) => status = Some(parsed),
                None => {
                    collected.note_code(STATUS_FIELD, CODE_INVALID_STATUS);
                    return finish_fallback(collected, FallbackCode::Status);
                }
            },
            RenderOutcome::Partial(partial) => {
                collected.note(STATUS_FIELD, &partial.errors);
                let code = fallback_code(&partial.errors);
                return finish_fallback(collected, code);
            }
        }
    }

    let body = match render(&job.compiled.body, &mut env) {
        RenderOutcome::Clean(rendered) => rendered.output.into_bytes(),
        RenderOutcome::Partial(partial) => {
            collected.note(BODY_FIELD, &partial.errors);
            let code = fallback_code(&partial.errors);
            return finish_fallback(collected, code);
        }
    };

    let mut headers = Vec::with_capacity(job.compiled.headers.len());
    for header in &job.compiled.headers {
        let field = header_field(header.index);
        match render(&header.template, &mut env) {
            RenderOutcome::Clean(rendered) => {
                match validated_header(&header.name, rendered.output) {
                    Some(valid) => headers.push(valid),
                    None => collected.note_code(&field, CODE_INVALID_HEADER),
                }
            }
            RenderOutcome::Partial(partial) => {
                collected.note(&field, &partial.errors);
                let only_header_rules = partial
                    .errors
                    .iter()
                    .all(|error| error.code == RenderErrorCode::ForbiddenCharacter);
                if !only_header_rules {
                    let code = fallback_code(&partial.errors);
                    return finish_fallback(collected, code);
                }
            }
        }
    }

    Rendering {
        served: Served::Rendered(RenderedParts {
            status,
            headers,
            body,
        }),
        recorded: collected.recorded,
        truncated: collected.truncated,
    }
}

fn parse_status(output: &str) -> Option<HttpStatus> {
    if output.len() != 3 || !output.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let code: i64 = output.parse().ok()?;
    HttpStatus::new(code)
}

fn validated_header(name: &str, value: String) -> Option<RenderedHeader> {
    HeaderName::parse(name).ok()?;
    HeaderValue::parse(&value).ok()?;
    Some(RenderedHeader {
        name: name.to_owned(),
        value,
    })
}
