use crate::filters::push_json_escaped;
use crate::target::RenderTarget;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WriteFailure {
    LimitExceeded,
    ForbiddenCharacter,
}

pub(crate) struct BoundedWriter {
    buffer: String,
    max_bytes: usize,
    target: RenderTarget,
}

impl BoundedWriter {
    pub(crate) fn new(target: RenderTarget, max_bytes: usize, capacity_hint: usize) -> Self {
        Self {
            buffer: String::with_capacity(capacity_hint.min(max_bytes)),
            max_bytes,
            target,
        }
    }

    pub(crate) fn write_literal(&mut self, text: &str) -> Result<(), WriteFailure> {
        if self.buffer.len().saturating_add(text.len()) > self.max_bytes {
            return Err(WriteFailure::LimitExceeded);
        }
        if self.target.first_forbidden(text).is_some() {
            return Err(WriteFailure::ForbiddenCharacter);
        }
        self.buffer.push_str(text);
        Ok(())
    }

    pub(crate) fn write_value(&mut self, text: &str) -> Result<(), WriteFailure> {
        if self.target != RenderTarget::JsonString {
            return self.write_literal(text);
        }
        let start = self.buffer.len();
        if push_json_escaped(text, &mut self.buffer, self.max_bytes).is_err() {
            self.buffer.truncate(start);
            return Err(WriteFailure::LimitExceeded);
        }
        Ok(())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.buffer
    }

    pub(crate) fn into_string(self) -> String {
        self.buffer
    }
}
