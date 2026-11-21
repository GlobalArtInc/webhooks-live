use thiserror::Error;

use crate::limits::LimitsError;

pub const MAX_MESSAGE_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    SourceTooLarge,
    Syntax,
    UnexpectedEnd,
    DisabledFeature,
    ExpressionTooDeep,
    TooManyFunctions,
    LiteralTooLarge,
    UndefinedVariable,
    AssignmentToConstant,
    InvalidFunction,
    InvalidLiteral,
    CompileFailed,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceTooLarge => "source_too_large",
            Self::Syntax => "syntax",
            Self::UnexpectedEnd => "unexpected_end",
            Self::DisabledFeature => "disabled_feature",
            Self::ExpressionTooDeep => "expression_too_deep",
            Self::TooManyFunctions => "too_many_functions",
            Self::LiteralTooLarge => "literal_too_large",
            Self::UndefinedVariable => "undefined_variable",
            Self::AssignmentToConstant => "assignment_to_constant",
            Self::InvalidFunction => "invalid_function",
            Self::InvalidLiteral => "invalid_literal",
            Self::CompileFailed => "compile_failed",
        }
    }

    pub fn field_code(self) -> &'static str {
        "script_invalid"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub message: String,
    pub position: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("script is invalid: {} error(s)", .diagnostics.len())]
pub struct CompileError {
    pub diagnostics: Vec<Diagnostic>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScriptError {
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error("operation limit exceeded")]
    Operations,
    #[error("time limit exceeded")]
    Time,
    #[error("memory limit exceeded")]
    Memory,
    #[error("host call budget exceeded")]
    HostBudget,
    #[error("call depth limit exceeded")]
    CallDepth,
    #[error("data too large: {message}")]
    DataTooLarge {
        message: String,
        position: Option<LinePosition>,
    },
    #[error("too many variables")]
    TooManyVariables { position: Option<LinePosition> },
    #[error("undefined function: {name}")]
    UndefinedFunction {
        name: String,
        position: Option<LinePosition>,
    },
    #[error("undefined variable: {name}")]
    UndefinedVariable {
        name: String,
        position: Option<LinePosition>,
    },
    #[error("runtime error: {message}")]
    Runtime {
        message: String,
        position: Option<LinePosition>,
    },
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid result: {0}")]
    InvalidResult(String),
    #[error("script panicked")]
    Panic,
    #[error("run was cancelled")]
    Cancelled,
    #[error("script queue is full")]
    Overloaded,
    #[error("script pool is shut down")]
    ShutDown,
    #[error("no result before the result timeout")]
    ResultTimeout,
    #[error(transparent)]
    Limits(#[from] LimitsError),
}

impl ScriptError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Compile(_) => "compile",
            Self::Operations => "operations",
            Self::Time => "time",
            Self::Memory => "memory",
            Self::HostBudget => "host_budget",
            Self::CallDepth => "call_depth",
            Self::DataTooLarge { .. } => "data_too_large",
            Self::TooManyVariables { .. } => "too_many_variables",
            Self::UndefinedFunction { .. } => "undefined_function",
            Self::UndefinedVariable { .. } => "undefined_variable",
            Self::Runtime { .. } => "runtime",
            Self::InvalidInput(_) => "invalid_input",
            Self::InvalidResult(_) => "invalid_result",
            Self::Panic => "script_panic",
            Self::Cancelled => "cancelled",
            Self::Overloaded => "overloaded",
            Self::ShutDown => "shut_down",
            Self::ResultTimeout => "result_timeout",
            Self::Limits(_) => "limits",
        }
    }

    pub fn outcome(&self) -> &'static str {
        match self {
            Self::Operations | Self::HostBudget | Self::CallDepth => "operations",
            Self::Memory => "memory",
            Self::Time | Self::ResultTimeout => "time",
            Self::Panic => "panic",
            _ => "error",
        }
    }
}

pub(crate) fn bounded_message(text: &str) -> String {
    match text.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((end, _)) => text.get(..end).unwrap_or("").to_owned(),
        None => text.to_owned(),
    }
}
