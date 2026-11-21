use crate::log::{MAX_DETAIL_BYTES, bound_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    Template,
    Budget,
    Script,
    Http,
    Quota,
    Overloaded,
    HopLimit,
    Relayed,
    Internal,
}

impl FailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Budget => "budget",
            Self::Script => "script",
            Self::Http => "http",
            Self::Quota => "quota",
            Self::Overloaded => "overloaded",
            Self::HopLimit => "hop_limit",
            Self::Relayed => "relayed",
            Self::Internal => "internal",
        }
    }

    pub fn run_error(self) -> &'static str {
        match self {
            Self::Quota => "quota_exceeded",
            other => other.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: FailureCode,
    pub detail: String,
}

impl Failure {
    pub fn new(code: FailureCode, detail: impl AsRef<str>) -> Self {
        Self {
            code,
            detail: bound_text(detail.as_ref(), MAX_DETAIL_BYTES),
        }
    }

    pub fn run_error(&self) -> String {
        if self.detail.is_empty() {
            self.code.run_error().to_owned()
        } else {
            format!("{}: {}", self.code.run_error(), self.detail)
        }
    }
}
