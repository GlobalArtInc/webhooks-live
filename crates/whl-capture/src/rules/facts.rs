use bytes::Bytes;
use time::OffsetDateTime;
use whl_store::captures::{Endpoint, NameValue as StoredPair, NewCapture, StoredCapture};
use whl_template::{
    BodyData, CaptureData, Context, EndpointData, JSON_BODY_DEFAULT_BYTES, NameValue,
    SignatureState,
};
use whl_verify::Verification;

const KIND_HTTP: &str = "http";
pub const RULE_BODY_BYTES: usize = JSON_BODY_DEFAULT_BYTES + 1;

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
    pub body: Bytes,
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

fn signature_of(verification: Option<Verification>) -> SignatureState {
    match verification {
        Some(Verification::Valid) => SignatureState::Valid,
        Some(Verification::Invalid) => SignatureState::Invalid,
        Some(Verification::Missing) | None => SignatureState::Missing,
    }
}

impl RuleFacts {
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
            body: Bytes::copy_from_slice(
                request.body.get(..RULE_BODY_BYTES).unwrap_or(&request.body),
            ),
            ip: request.client_ip.to_string(),
            content_type: request.content_type.clone().unwrap_or_default(),
            signature: signature_of(verification),
        }
    }

    pub fn context(&self) -> Context {
        let mut context = Context::new(self.received_at);
        context.request.method.clone_from(&self.method);
        context.request.path.clone_from(&self.path);
        context.request.segments = self
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect();
        context.request.query.clone_from(&self.query);
        context.request.headers.clone_from(&self.headers);
        context.request.form.clone_from(&self.form);
        context.request.meta.clone_from(&self.meta);
        let kept = self.body.get(..RULE_BODY_BYTES).unwrap_or(&self.body);
        context.request.body = BodyData::new(kept.to_vec());
        context.request.ip.clone_from(&self.ip);
        context.request.content_type.clone_from(&self.content_type);
        context.request.kind.clone_from(&self.kind);
        context.request.signature = self.signature;
        context.capture = CaptureData {
            id: self.capture_id.clone(),
            seq: self.capture_seq,
            received_at: self.received_at,
        };
        context.endpoint = EndpointData {
            id: self.endpoint_id.clone(),
            url: self.endpoint_url.clone(),
            title: self.endpoint_title.clone(),
        };
        context
    }
}
