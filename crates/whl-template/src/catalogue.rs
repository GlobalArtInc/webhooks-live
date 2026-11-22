use std::collections::{BTreeSet, VecDeque};

use serde_json::Value as JsonValue;

use crate::Template;
use crate::ast::{Key, Reference, Segment};
use crate::context::Context;
use crate::env::{ChaChaRandom, RandomSeed, RenderEnv, SystemClock};
use crate::limits::{MAX_KEY_BYTES, RenderLimits};
use crate::parser::{is_key_character, is_valid_name};
use crate::render::RenderOutcome;
use crate::target::{Policy, RenderTarget};

pub const OPEN_DELIMITER: &str = "{{";
pub const CLOSE_DELIMITER: &str = "}}";

pub const MAX_SAMPLE_CHARS: usize = 80;

const MAX_DYNAMIC_NAMES: usize = 30;
const MAX_SEGMENT_ENTRIES: usize = 8;
const MAX_JSON_ENTRIES: usize = 100;
const MAX_JSON_DEPTH: usize = 4;
const MAX_JSON_ARRAY_ITEMS: usize = 3;
const MAX_JSON_OBJECT_KEYS: usize = 50;
const MAX_VARIABLE_ENTRIES: usize = 50;
const SAMPLE_SEED: &[u8] = b"whl-template-catalogue";
const ELLIPSIS: char = '\u{2026}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delimiters {
    pub open: &'static str,
    pub close: &'static str,
}

pub const DELIMITERS: Delimiters = Delimiters {
    open: OPEN_DELIMITER,
    close: CLOSE_DELIMITER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueBasis {
    Capture,
    NoCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueGroup {
    Request,
    Time,
    Variables,
    Helpers,
}

impl CatalogueGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Time => "time",
            Self::Variables => "variables",
            Self::Helpers => "helpers",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueType {
    String,
    Integer,
    Number,
    Boolean,
    Null,
    Object,
    Array,
    Json,
    Time,
    Function,
}

impl CatalogueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Null => "null",
            Self::Object => "object",
            Self::Array => "array",
            Self::Json => "json",
            Self::Time => "time",
            Self::Function => "function",
        }
    }

    fn of_json(value: &JsonValue) -> Self {
        match value {
            JsonValue::String(_) => Self::String,
            JsonValue::Number(number) if number.is_i64() || number.is_u64() => Self::Integer,
            JsonValue::Number(_) => Self::Number,
            JsonValue::Bool(_) => Self::Boolean,
            JsonValue::Null => Self::Null,
            JsonValue::Object(_) => Self::Object,
            JsonValue::Array(_) => Self::Array,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueEntry {
    pub path: String,
    pub insert_text: String,
    pub group: CatalogueGroup,
    pub value_type: CatalogueType,
    pub sample: Option<String>,
    pub description: Option<&'static str>,
}

struct Listed {
    reference: Reference,
    value_type: CatalogueType,
    group: CatalogueGroup,
    description: &'static str,
}

struct Helper {
    call: &'static str,
    description: &'static str,
}

const HELPERS: [Helper; 4] = [
    Helper {
        call: "uuid()",
        description: "A new random UUID",
    },
    Helper {
        call: "random_hex(8)",
        description: "Random hexadecimal digits, at most 64",
    },
    Helper {
        call: "random_int(1, 100)",
        description: "A random integer between the two bounds",
    },
    Helper {
        call: "json(\"$.path\")",
        description: "The first match of an RFC 9535 path in the JSON request body",
    },
];

fn listed(
    reference: Reference,
    value_type: CatalogueType,
    group: CatalogueGroup,
    description: &'static str,
) -> Listed {
    Listed {
        reference,
        value_type,
        group,
        description,
    }
}

fn fixed_references() -> Vec<Listed> {
    use CatalogueGroup::{Request, Time};
    use CatalogueType::{Integer, Json, String as Text, Time as Moment};
    vec![
        listed(Reference::RequestMethod, Text, Request, "The HTTP method"),
        listed(Reference::RequestPath, Text, Request, "The request path"),
        listed(
            Reference::RequestBody,
            Text,
            Request,
            "The request body as text, the first 64 KiB",
        ),
        listed(
            Reference::RequestHeaders,
            Json,
            Request,
            "All request headers as a JSON object",
        ),
        listed(Reference::RequestIp, Text, Request, "The sender address"),
        listed(
            Reference::RequestContentType,
            Text,
            Request,
            "The request content type",
        ),
        listed(
            Reference::RequestKind,
            Text,
            Request,
            "The kind of the capture",
        ),
        listed(
            Reference::RequestSignature,
            Text,
            Request,
            "valid, invalid or missing",
        ),
        listed(Reference::CaptureId, Text, Request, "The capture id"),
        listed(
            Reference::CaptureSeq,
            Integer,
            Request,
            "The sequence number of the capture in its endpoint",
        ),
        listed(
            Reference::CaptureReceivedAt,
            Moment,
            Request,
            "When the capture was received",
        ),
        listed(Reference::EndpointId, Text, Request, "The endpoint id"),
        listed(
            Reference::EndpointUrl,
            Text,
            Request,
            "The public URL of the endpoint",
        ),
        listed(
            Reference::EndpointTitle,
            Text,
            Request,
            "The title of the endpoint",
        ),
        listed(
            Reference::Now,
            Moment,
            Time,
            "The time the capture was received",
        ),
    ]
}

fn is_plain_key(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_KEY_BYTES && name.chars().all(is_key_character)
}

fn segment_text(segment: &Segment) -> Option<String> {
    match segment {
        Segment::Index(index) => Some(format!("[{index}]")),
        Segment::Key(key) if is_plain_key(key.unpack()) => Some(format!(".{}", key.unpack())),
        Segment::Key(_) => None,
    }
}

pub fn reference_path(reference: &Reference) -> Option<String> {
    let text = match reference {
        Reference::RequestMethod => "request.method".to_owned(),
        Reference::RequestPath => "request.path".to_owned(),
        Reference::RequestSegment(index) => format!("request.segments[{index}]"),
        Reference::RequestQuery(key) => keyed("request.query", key)?,
        Reference::RequestHeader(key) => keyed("request.header", key)?,
        Reference::RequestHeaders => "request.headers".to_owned(),
        Reference::RequestForm(key) => keyed("request.form", key)?,
        Reference::RequestBody => "request.body".to_owned(),
        Reference::RequestIp => "request.ip".to_owned(),
        Reference::RequestContentType => "request.content_type".to_owned(),
        Reference::RequestKind => "request.kind".to_owned(),
        Reference::RequestMeta(key) => keyed("request.meta", key)?,
        Reference::RequestSignature => "request.signature".to_owned(),
        Reference::CaptureId => "capture.id".to_owned(),
        Reference::CaptureSeq => "capture.seq".to_owned(),
        Reference::CaptureReceivedAt => "capture.received_at".to_owned(),
        Reference::EndpointId => "endpoint.id".to_owned(),
        Reference::EndpointUrl => "endpoint.url".to_owned(),
        Reference::EndpointTitle => "endpoint.title".to_owned(),
        Reference::Store(name, tail) => named("store", name, tail)?,
        Reference::Counter(name) => named("counter", name, &[])?,
        Reference::Now => "now".to_owned(),
        Reference::Var(..) | Reference::Step(..) | Reference::Secret(_) => return None,
    };
    Some(text)
}

fn keyed(root: &str, key: &Key) -> Option<String> {
    is_plain_key(key.unpack()).then(|| format!("{root}.{}", key.unpack()))
}

fn named(root: &str, name: &Key, tail: &[Segment]) -> Option<String> {
    if !is_valid_name(name.unpack()) {
        return None;
    }
    let mut text = format!("{root}.{}", name.unpack());
    for segment in tail {
        text.push_str(&segment_text(segment)?);
    }
    Some(text)
}

fn insert_text(path: &str) -> String {
    format!("{OPEN_DELIMITER} {path} {CLOSE_DELIMITER}")
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_SAMPLE_CHARS {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(MAX_SAMPLE_CHARS - 1).collect();
    kept.push(ELLIPSIS);
    kept
}

fn compact(value: &JsonValue) -> Option<String> {
    serde_json::to_string(value).ok().map(|text| shorten(&text))
}

fn quoted(text: &str) -> Option<String> {
    serde_json::to_string(text).ok().map(|text| shorten(&text))
}

struct Sampler<'a> {
    context: &'a Context,
    basis: CatalogueBasis,
    random: ChaChaRandom,
    clock: SystemClock,
}

impl<'a> Sampler<'a> {
    fn new(context: &'a Context, basis: CatalogueBasis) -> Self {
        Self {
            context,
            basis,
            random: ChaChaRandom::from_seed(&RandomSeed::from_bytes(SAMPLE_SEED)),
            clock: SystemClock::new(),
        }
    }

    fn sample(&mut self, path: &str, value_type: CatalogueType) -> Option<String> {
        if self.basis == CatalogueBasis::NoCapture {
            return None;
        }
        let source = insert_text(path);
        let policy = Policy::response(RenderTarget::Body);
        let template = Template::parse(&source, &policy).ok()?;
        let limits = RenderLimits::for_target(RenderTarget::Body);
        let mut env = RenderEnv::new(&mut self.random, &self.clock);
        let RenderOutcome::Clean(rendered) = template.render(self.context, &limits, &mut env)
        else {
            return None;
        };
        if !rendered.missing.is_empty() {
            return None;
        }
        match value_type {
            CatalogueType::String => quoted(&rendered.output),
            _ => Some(shorten(&rendered.output)),
        }
    }
}

fn distinct_names<'a>(names: impl Iterator<Item = &'a str>, lower: bool) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for name in names {
        let name = if lower {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        if is_plain_key(&name) && seen.insert(name.clone()) {
            found.push(name);
            if found.len() == MAX_DYNAMIC_NAMES {
                break;
            }
        }
    }
    found
}

fn json_key_path(parent: &str, key: &str) -> Option<String> {
    let mut characters = key.chars();
    let shorthand = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if shorthand {
        return Some(format!("{parent}.{key}"));
    }
    is_plain_key(key).then(|| format!("{parent}['{key}']"))
}

struct Pending<'a> {
    path: String,
    value: &'a JsonValue,
    depth: usize,
}

fn json_children<'a>(value: &'a JsonValue, path: &str) -> Vec<(String, &'a JsonValue)> {
    match value {
        JsonValue::Object(map) => map
            .iter()
            .take(MAX_JSON_OBJECT_KEYS)
            .filter_map(|(key, child)| Some((json_key_path(path, key)?, child)))
            .collect(),
        JsonValue::Array(items) => items
            .iter()
            .take(MAX_JSON_ARRAY_ITEMS)
            .enumerate()
            .map(|(index, child)| (format!("{path}[{index}]"), child))
            .collect(),
        _ => Vec::new(),
    }
}

fn json_entries(document: &JsonValue) -> Vec<CatalogueEntry> {
    let mut found = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(Pending {
        path: "$".to_owned(),
        value: document,
        depth: 1,
    });
    while let Some(pending) = queue.pop_front() {
        if pending.depth > MAX_JSON_DEPTH {
            continue;
        }
        for (child_path, child) in json_children(pending.value, &pending.path) {
            if found.len() >= MAX_JSON_ENTRIES {
                return found;
            }
            let call = format!("json(\"{child_path}\")");
            found.push(CatalogueEntry {
                insert_text: insert_text(&call),
                path: call,
                group: CatalogueGroup::Request,
                value_type: CatalogueType::of_json(child),
                sample: compact(child),
                description: None,
            });
            queue.push_back(Pending {
                path: child_path,
                value: child,
                depth: pending.depth + 1,
            });
        }
    }
    found
}

fn entry_for(
    sampler: &mut Sampler<'_>,
    reference: &Reference,
    value_type: CatalogueType,
    group: CatalogueGroup,
    description: Option<&'static str>,
) -> Option<CatalogueEntry> {
    let path = reference_path(reference)?;
    let sample = sampler.sample(&path, value_type);
    Some(CatalogueEntry {
        insert_text: insert_text(&path),
        path,
        group,
        value_type,
        sample,
        description,
    })
}

fn dynamic_request_entries(sampler: &mut Sampler<'_>) -> Vec<CatalogueEntry> {
    let request = &sampler.context.request;
    let segments = request.segments.len().min(MAX_SEGMENT_ENTRIES);
    let queries = distinct_names(request.query.iter().map(|pair| pair.name.as_str()), false);
    let headers = distinct_names(request.headers.iter().map(|pair| pair.name.as_str()), true);
    let forms = distinct_names(request.form.iter().map(|pair| pair.name.as_str()), false);
    let metas = distinct_names(request.meta.iter().map(|pair| pair.name.as_str()), false);
    let mut references = Vec::new();
    references.extend((0..segments).map(Reference::RequestSegment));
    references.extend(
        queries
            .iter()
            .map(|name| Reference::RequestQuery(Key::new(name))),
    );
    references.extend(
        headers
            .iter()
            .map(|name| Reference::RequestHeader(Key::new(name))),
    );
    references.extend(
        forms
            .iter()
            .map(|name| Reference::RequestForm(Key::new(name))),
    );
    references.extend(
        metas
            .iter()
            .map(|name| Reference::RequestMeta(Key::new(name))),
    );
    references
        .iter()
        .filter_map(|reference| {
            entry_for(
                sampler,
                reference,
                CatalogueType::String,
                CatalogueGroup::Request,
                None,
            )
        })
        .collect()
}

fn variable_entries(sampler: &mut Sampler<'_>) -> Vec<CatalogueEntry> {
    let context = sampler.context;
    let mut found = Vec::new();
    for (name, value) in context.store.iter().take(MAX_VARIABLE_ENTRIES) {
        let reference = Reference::Store(Key::new(name), Vec::new());
        if let Some(path) = reference_path(&reference) {
            found.push(CatalogueEntry {
                insert_text: insert_text(&path),
                path,
                group: CatalogueGroup::Variables,
                value_type: CatalogueType::of_json(value),
                sample: compact(value),
                description: Some("A persistent variable of the endpoint"),
            });
        }
    }
    for (name, value) in context.counters.iter().take(MAX_VARIABLE_ENTRIES) {
        let reference = Reference::Counter(Key::new(name));
        if let Some(path) = reference_path(&reference) {
            found.push(CatalogueEntry {
                insert_text: insert_text(&path),
                path,
                group: CatalogueGroup::Variables,
                value_type: CatalogueType::Integer,
                sample: Some(value.to_string()),
                description: Some("A persistent counter of the endpoint"),
            });
        }
    }
    found
}

pub fn catalogue(context: &Context, basis: CatalogueBasis) -> Vec<CatalogueEntry> {
    let mut sampler = Sampler::new(context, basis);
    let mut request = Vec::new();
    let mut time = Vec::new();
    for item in fixed_references() {
        let Some(entry) = entry_for(
            &mut sampler,
            &item.reference,
            item.value_type,
            item.group,
            Some(item.description),
        ) else {
            continue;
        };
        if entry.group == CatalogueGroup::Time {
            time.push(entry);
        } else {
            request.push(entry);
        }
    }
    if basis == CatalogueBasis::Capture {
        request.extend(dynamic_request_entries(&mut sampler));
        if let Some(document) = context.request.body.json() {
            request.extend(json_entries(document));
        }
    }
    let helpers = HELPERS.iter().map(|helper| CatalogueEntry {
        path: helper.call.to_owned(),
        insert_text: insert_text(helper.call),
        group: CatalogueGroup::Helpers,
        value_type: CatalogueType::Function,
        sample: None,
        description: Some(helper.description),
    });
    let mut entries = request;
    entries.extend(time);
    entries.extend(variable_entries(&mut sampler));
    entries.extend(helpers);
    entries
}
