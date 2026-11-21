use serde_json::{Value, json};
use whl_core::status::HttpStatus;

pub(crate) const MAX_RECORDED: usize = 20;

pub(crate) const OUTCOME_OK: &str = "ok";
pub(crate) const OUTCOME_HEADER_DROPPED: &str = "header_dropped";
pub(crate) const OUTCOME_FALLBACK: &str = "fallback";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FallbackCode {
    Invalid,
    Render,
    Deadline,
    Budget,
    Status,
    Overloaded,
    Variables,
    Internal,
}

impl FallbackCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Render => "render",
            Self::Deadline => "deadline",
            Self::Budget => "budget",
            Self::Status => "status",
            Self::Overloaded => "overloaded",
            Self::Variables => "variables",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedParts {
    pub status: Option<HttpStatus>,
    pub headers: Vec<RenderedHeader>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Recorded {
    pub field: String,
    pub code: &'static str,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug)]
pub(crate) enum Served {
    Rendered(RenderedParts),
    Fallback(FallbackCode),
}

#[derive(Debug)]
pub(crate) struct Rendering {
    pub served: Served,
    pub recorded: Vec<Recorded>,
    pub truncated: bool,
}

impl Rendering {
    pub(crate) fn fallback(code: FallbackCode) -> Self {
        Self {
            served: Served::Fallback(code),
            recorded: Vec::new(),
            truncated: false,
        }
    }

    pub(crate) fn outcome(&self) -> &'static str {
        match &self.served {
            Served::Fallback(_) => OUTCOME_FALLBACK,
            Served::Rendered(_) if self.recorded.is_empty() => OUTCOME_OK,
            Served::Rendered(_) => OUTCOME_HEADER_DROPPED,
        }
    }

    pub(crate) fn fallback_code(&self) -> Option<FallbackCode> {
        match &self.served {
            Served::Fallback(code) => Some(*code),
            Served::Rendered(_) => None,
        }
    }

    pub(crate) fn meta(&self) -> Option<Value> {
        if self.recorded.is_empty() && self.fallback_code().is_none() {
            return None;
        }
        let diagnostics: Vec<Value> = self
            .recorded
            .iter()
            .take(MAX_RECORDED)
            .map(|entry| {
                json!({
                    "field": entry.field,
                    "code": entry.code,
                    "line": entry.line,
                    "column": entry.column,
                })
            })
            .collect();
        let truncated = self.truncated || self.recorded.len() > MAX_RECORDED;
        let code = self.fallback_code().map(FallbackCode::as_str);
        Some(json!({
            "template": {
                "outcome": self.outcome(),
                "code": code,
                "diagnostics": diagnostics,
                "truncated": truncated,
            }
        }))
    }

    pub(crate) fn rendered_status(&self) -> Option<HttpStatus> {
        match &self.served {
            Served::Rendered(parts) => parts.status,
            Served::Fallback(_) => None,
        }
    }
}
