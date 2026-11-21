use crate::limits::MAX_DELAY_MS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderOp {
    Add(String, String),
    Set(String, String),
    Remove(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseDraft {
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub body: Option<Vec<u8>>,
    pub replaces_headers: bool,
    pub header_ops: Vec<HeaderOp>,
    pub delay_ms: u32,
}

impl ResponseDraft {
    pub fn is_untouched(&self) -> bool {
        self.status.is_none()
            && self.content_type.is_none()
            && self.body.is_none()
            && !self.replaces_headers
            && self.header_ops.is_empty()
            && self.delay_ms == 0
    }

    pub fn replace(
        &mut self,
        status: u16,
        content_type: Option<String>,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) {
        self.status = Some(status);
        self.content_type = content_type;
        self.body = Some(body);
        self.replaces_headers = true;
        self.header_ops = headers
            .into_iter()
            .map(|(name, value)| HeaderOp::Add(name, value))
            .collect();
    }

    pub fn add_delay(&mut self, ms: u32) {
        let cap = u32::try_from(MAX_DELAY_MS).unwrap_or(u32::MAX);
        self.delay_ms = self.delay_ms.saturating_add(ms).min(cap);
    }

    pub fn apply_headers(&self, base: Vec<(String, String)>) -> Vec<(String, String)> {
        let mut headers = if self.replaces_headers {
            Vec::new()
        } else {
            base
        };
        for operation in &self.header_ops {
            match operation {
                HeaderOp::Add(name, value) => headers.push((name.clone(), value.clone())),
                HeaderOp::Set(name, value) => {
                    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
                    headers.push((name.clone(), value.clone()));
                }
                HeaderOp::Remove(name) => {
                    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
                }
            }
        }
        headers
    }
}
