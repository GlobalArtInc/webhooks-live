use thiserror::Error;
use whl_script::Position as ScriptPosition;
use whl_template::Position as TemplatePosition;

pub const MAX_DIAGNOSTICS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleErrorCode {
    Required,
    InvalidFormat,
    TooLong,
    OutOfRange,
    TooMany,
    Duplicate,
    NotAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub length: usize,
}

impl From<TemplatePosition> for Position {
    fn from(position: TemplatePosition) -> Self {
        Self {
            offset: position.offset,
            line: position.line,
            column: position.column,
            length: position.length,
        }
    }
}

impl From<ScriptPosition> for Position {
    fn from(position: ScriptPosition) -> Self {
        Self {
            offset: position.offset,
            line: position.line,
            column: position.column,
            length: position.length,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDiagnostic {
    pub field: String,
    pub code: RuleErrorCode,
    pub message: String,
    pub position: Option<Position>,
}

impl RuleDiagnostic {
    pub fn new(field: impl Into<String>, code: RuleErrorCode, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
            position: None,
        }
    }

    pub fn with_position(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("rules are invalid: {} error(s)", .diagnostics.len())]
pub struct RuleError {
    pub diagnostics: Vec<RuleDiagnostic>,
    pub truncated: bool,
}
