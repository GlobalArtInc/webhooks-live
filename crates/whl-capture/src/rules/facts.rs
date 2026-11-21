use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_store::CaptureDetail;
use whl_store::captures::{Endpoint, NameValue as StoredPair, NewCapture, StoredCapture};
use whl_template::{
    BodyData, CaptureData, Context, EndpointData, JSON_BODY_DEFAULT_BYTES, NameValue,
    SignatureState,
};
use whl_verify::Verification;

const KIND_HTTP: &str = "http";
pub const RULE_BODY_BYTES: usize = JSON_BODY_DEFAULT_BYTES + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the stored capture time cannot be read")]
pub struct FactsError;

#[derive(Debug, Clone)]
pub struct RuleFacts {
    pub endpoint_id: String,
    pub endpoint_url: String,
    pub endpoint_title: String,
    pub capture_id: String,
    pub capture_seq: i64,
    pub received_at: OffsetDateTime,
    pub kind: String,
    pub method: String,
    pub path: String,
    pub query: Vec<NameValue>,
    pub headers: Vec<NameValue>,
    pub form: Vec<NameValue>,
    pub meta: Vec<NameValue>,
    pub body: Vec<u8>,
    pub ip: String,
    pub content_type: String,
    pub signature: SignatureState,
}

fn pairs(source: &[StoredPair]) -> Vec<NameValue> {
    source
        .iter()
        .map(|pair| NameValue::new(pair.name.as_str(), pair.value.as_str()))
        .collect()
}

fn detail_pairs(source: &[whl_store::NameValue]) -> Vec<NameValue> {
    source
        .iter()
        .map(|pair| NameValue::new(pair.name.as_str(), pair.value.as_str()))
        .collect()
}

fn signature_of(verification: Option<Verification>) -> SignatureState {
    match verification {
        Some(Verification::Valid) => SignatureState::Valid,
        Some(Verification::Invalid) => SignatureState::Invalid,
        Some(Verification::Missing) | None => SignatureState::Missing,
    }
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

impl RuleFacts {
    pub fn from_detail(
        detail: &CaptureDetail,
        public_url: &str,
        title: Option<&str>,
    ) -> Result<Self, FactsError> {
        let summary = &detail.summary;
        let received_at =
            OffsetDateTime::parse(&summary.received_at, &Rfc3339).map_err(|_| FactsError)?;
        Ok(Self {
            endpoint_id: summary.endpoint_id.clone(),
            endpoint_url: format!("{public_url}/{}", summary.endpoint_id),
            endpoint_title: title.unwrap_or_default().to_owned(),
            capture_id: summary.id.clone(),
            capture_seq: summary.seq,
            received_at,
            kind: summary.kind.as_str().to_owned(),
            method: summary.method.clone(),
            path: summary.path.clone(),
            query: detail_pairs(&detail.query),
            headers: detail_pairs(&detail.headers),
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
            body: detail
                .body
                .prefix
                .get(..RULE_BODY_BYTES)
                .unwrap_or(&detail.body.prefix)
                .to_vec(),
            ip: summary.client_ip.clone(),
            content_type: summary.content_type.clone().unwrap_or_default(),
            signature: signature_of(summary.verification),
        })
    }

    pub(crate) fn from_capture(
        request: &NewCapture,
        capture: &StoredCapture,
        endpoint: &Endpoint,
        public_url: &str,
        verification: Option<Verification>,
    ) -> Self {
        Self {
            endpoint_id: endpoint.id.unpack().to_owned(),
            endpoint_url: format!("{public_url}/{}", endpoint.id.unpack()),
            endpoint_title: endpoint.title.clone().unwrap_or_default(),
            capture_id: capture.id.to_string(),
            capture_seq: capture.seq,
            received_at: capture.received_at,
            kind: KIND_HTTP.to_owned(),
            method: request.method.clone(),
            path: request.path.clone(),
            query: pairs(&request.query),
            headers: pairs(&request.headers),
            form: request
                .form
                .as_ref()
                .map(|form| pairs(&form.fields))
                .unwrap_or_default(),
            meta: Vec::new(),
            body: request
                .body
                .get(..RULE_BODY_BYTES)
                .unwrap_or(&request.body)
                .to_vec(),
            ip: request.client_ip.to_string(),
            content_type: request.content_type.clone().unwrap_or_default(),
            signature: signature_of(verification),
        }
    }

    pub fn into_context(self) -> Context {
        let mut context = Context::new(self.received_at);
        context.request.segments = self
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect();
        context.request.method = self.method;
        context.request.path = self.path;
        context.request.query = self.query;
        context.request.headers = self.headers;
        context.request.form = self.form;
        context.request.meta = self.meta;
        let mut body = self.body;
        body.truncate(RULE_BODY_BYTES);
        context.request.body = BodyData::new(body);
        context.request.ip = self.ip;
        context.request.content_type = self.content_type;
        context.request.kind = self.kind;
        context.request.signature = self.signature;
        context.capture = CaptureData {
            id: self.capture_id,
            seq: self.capture_seq,
            received_at: self.received_at,
        };
        context.endpoint = EndpointData {
            id: self.endpoint_id,
            url: self.endpoint_url,
            title: self.endpoint_title,
        };
        context
    }
}
