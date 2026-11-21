use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use crate::limits::{BODY_TEXT_BYTES, JSON_BODY_BYTES, JSON_BODY_DEPTH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SignatureState {
    Valid,
    Invalid,
    #[default]
    Missing,
}

impl SignatureState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Missing => "missing",
        }
    }
}

fn json_depth(value: &JsonValue) -> usize {
    match value {
        JsonValue::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        JsonValue::Object(map) => 1 + map.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

pub(crate) enum JsonBody<'a> {
    Parsed(&'a JsonValue),
    Unusable,
    TooLarge,
}

pub(crate) fn json_body_fits(length: usize, max: usize) -> bool {
    length <= max.min(JSON_BODY_BYTES)
}

pub(crate) fn parse_json_document(bytes: &[u8]) -> Option<JsonValue> {
    if bytes.len() > JSON_BODY_BYTES {
        return None;
    }
    let content = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes);
    let value: JsonValue = serde_json::from_slice(content).ok()?;
    if json_depth(&value) > JSON_BODY_DEPTH {
        return None;
    }
    Some(value)
}

#[derive(Clone, Default)]
pub struct BodyData {
    raw: Vec<u8>,
    text: String,
    json: OnceLock<Option<JsonValue>>,
}

impl BodyData {
    pub fn new(raw: Vec<u8>) -> Self {
        let head = raw.get(..BODY_TEXT_BYTES).unwrap_or(&raw);
        let text = String::from_utf8_lossy(head).into_owned();
        Self {
            raw,
            text,
            json: OnceLock::new(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        Self::new(text.as_bytes().to_vec())
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn json(&self) -> Option<&JsonValue> {
        self.json
            .get_or_init(|| parse_json_document(&self.raw))
            .as_ref()
    }

    pub(crate) fn json_within(&self, max: usize) -> JsonBody<'_> {
        if !json_body_fits(self.raw.len(), max) {
            return JsonBody::TooLarge;
        }
        match self.json() {
            Some(document) => JsonBody::Parsed(document),
            None => JsonBody::Unusable,
        }
    }
}

impl fmt::Debug for BodyData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BodyData")
            .field("bytes", &self.raw.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

impl NameValue {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestData {
    pub method: String,
    pub path: String,
    pub segments: Vec<String>,
    pub query: Vec<NameValue>,
    pub headers: Vec<NameValue>,
    pub form: Vec<NameValue>,
    pub body: BodyData,
    pub ip: String,
    pub content_type: String,
    pub kind: String,
    pub meta: Vec<NameValue>,
    pub signature: SignatureState,
}

#[derive(Debug, Clone)]
pub struct CaptureData {
    pub id: String,
    pub seq: i64,
    pub received_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default)]
pub struct EndpointData {
    pub id: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Default)]
pub struct StepData {
    pub status: Option<u16>,
    pub headers: Vec<NameValue>,
    pub body: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets(BTreeMap<String, String>);

impl Secrets {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self(values)
    }

    pub fn unpack(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.keys().map(|name| format!("{name}=<redacted>")))
            .finish()
    }
}

#[derive(Clone)]
pub struct Context {
    pub request: RequestData,
    pub capture: CaptureData,
    pub endpoint: EndpointData,
    pub now: OffsetDateTime,
    pub vars: BTreeMap<String, JsonValue>,
    pub store: BTreeMap<String, JsonValue>,
    pub counters: BTreeMap<String, i64>,
    pub steps: BTreeMap<String, StepData>,
    pub secrets: Secrets,
}

impl fmt::Debug for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Context")
            .field("request", &self.request)
            .field("capture", &self.capture)
            .field("endpoint", &self.endpoint)
            .field("now", &self.now)
            .field("vars", &self.vars)
            .field("store", &self.store)
            .field("counters", &self.counters)
            .field("steps", &self.steps)
            .field(
                "secrets",
                &format_args!("<{} redacted>", self.secrets.unpack().len()),
            )
            .finish()
    }
}

impl Context {
    pub fn new(now: OffsetDateTime) -> Self {
        Self {
            request: RequestData::default(),
            capture: CaptureData {
                id: String::new(),
                seq: 0,
                received_at: now,
            },
            endpoint: EndpointData::default(),
            now,
            vars: BTreeMap::new(),
            store: BTreeMap::new(),
            counters: BTreeMap::new(),
            steps: BTreeMap::new(),
            secrets: Secrets::default(),
        }
    }
}
