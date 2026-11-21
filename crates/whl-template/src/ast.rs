use std::collections::BTreeSet;

use serde_json_path::JsonPath;

use crate::span::ByteRange;
use crate::target::RenderTarget;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key(String);

impl Key {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Key(Key),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepField {
    Status,
    Header(Key),
    Body,
    DurationMs,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    RequestMethod,
    RequestPath,
    RequestSegment(usize),
    RequestQuery(Key),
    RequestHeader(Key),
    RequestHeaders,
    RequestForm(Key),
    RequestBody,
    RequestIp,
    RequestContentType,
    RequestKind,
    RequestMeta(Key),
    RequestSignature,
    CaptureId,
    CaptureSeq,
    CaptureReceivedAt,
    EndpointId,
    EndpointUrl,
    EndpointTitle,
    Var(Key, Vec<Segment>),
    Store(Key, Vec<Segment>),
    Counter(Key),
    Step(Key, StepField),
    Secret(Key),
    Now,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Reference(Reference),
    Text(String),
    Int(i64),
}

#[derive(Debug, Clone)]
pub struct CompiledJsonPath {
    pub path: JsonPath,
}

#[derive(Debug, Clone)]
pub enum JsonSource {
    Request,
    Step(Key),
}

#[derive(Debug, Clone)]
pub enum Function {
    Json(JsonSource, CompiledJsonPath),
    Uuid,
    RandomHex(Arg),
    RandomInt(Arg, Arg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKind {
    Default(Arg),
    Upper,
    Lower,
    Trim,
    Truncate(Arg),
    Replace(Arg, Arg),
    Length,
    Json,
    Url,
    Html,
    Base64,
    Base64Decode,
    Hex,
    Sha256,
    HmacSha256(Arg),
    Date(Arg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub kind: FilterKind,
    pub range: ByteRange,
}

#[derive(Debug, Clone)]
pub enum Primary {
    Reference(Reference),
    Function(Function),
    Text(String),
    Int(i64),
}

#[derive(Debug, Clone)]
pub struct Expression {
    pub range: ByteRange,
    pub primary_range: ByteRange,
    pub primary: Primary,
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone)]
pub enum Part {
    Text(ByteRange),
    Expression(Expression),
}

#[derive(Debug, Clone)]
pub struct Template {
    source: Box<str>,
    parts: Vec<Part>,
    target: RenderTarget,
    expressions: usize,
}

impl Template {
    pub(crate) fn assemble(
        source: &str,
        parts: Vec<Part>,
        target: RenderTarget,
        expressions: usize,
    ) -> Self {
        Self {
            source: source.into(),
            parts,
            target,
            expressions,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn target(&self) -> RenderTarget {
        self.target
    }

    pub fn expression_count(&self) -> usize {
        self.expressions
    }

    pub(crate) fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub(crate) fn slice(&self, range: ByteRange) -> &str {
        self.source.get(range.start..range.end).unwrap_or("")
    }
}

fn arg_reference(arg: &Arg) -> Option<&Reference> {
    match arg {
        Arg::Reference(reference) => Some(reference),
        Arg::Text(_) | Arg::Int(_) => None,
    }
}

fn expression_references(expression: &Expression) -> Vec<&Reference> {
    let mut found = Vec::new();
    match &expression.primary {
        Primary::Reference(reference) => found.push(reference),
        Primary::Function(Function::RandomHex(arg)) => found.extend(arg_reference(arg)),
        Primary::Function(Function::RandomInt(low, high)) => {
            found.extend(arg_reference(low));
            found.extend(arg_reference(high));
        }
        Primary::Function(Function::Json(..) | Function::Uuid)
        | Primary::Text(_)
        | Primary::Int(_) => {}
    }
    for filter in &expression.filters {
        match &filter.kind {
            FilterKind::Default(arg)
            | FilterKind::Truncate(arg)
            | FilterKind::HmacSha256(arg)
            | FilterKind::Date(arg) => found.extend(arg_reference(arg)),
            FilterKind::Replace(from, to) => {
                found.extend(arg_reference(from));
                found.extend(arg_reference(to));
            }
            FilterKind::Upper
            | FilterKind::Lower
            | FilterKind::Trim
            | FilterKind::Length
            | FilterKind::Json
            | FilterKind::Url
            | FilterKind::Html
            | FilterKind::Base64
            | FilterKind::Base64Decode
            | FilterKind::Hex
            | FilterKind::Sha256 => {}
        }
    }
    found
}

impl Template {
    fn referenced_names(&self, select: fn(&Reference) -> Option<&Key>) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for part in &self.parts {
            if let Part::Expression(expression) = part {
                for reference in expression_references(expression) {
                    if let Some(key) = select(reference) {
                        names.insert(key.unpack().to_owned());
                    }
                }
            }
        }
        names
    }

    pub fn referenced_counters(&self) -> BTreeSet<String> {
        self.referenced_names(|reference| match reference {
            Reference::Counter(name) => Some(name),
            _ => None,
        })
    }

    pub fn referenced_vars(&self) -> BTreeSet<String> {
        self.referenced_names(|reference| match reference {
            Reference::Var(name, _) => Some(name),
            _ => None,
        })
    }

    pub fn referenced_secrets(&self) -> BTreeSet<String> {
        self.referenced_names(|reference| match reference {
            Reference::Secret(name) => Some(name),
            _ => None,
        })
    }
}
