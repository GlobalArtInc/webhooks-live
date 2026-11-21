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
