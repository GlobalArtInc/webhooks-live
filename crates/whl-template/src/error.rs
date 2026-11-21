use thiserror::Error;

use crate::span::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    SourceTooLarge,
    TooManyExpressions,
    UnterminatedExpression,
    UnterminatedString,
    InvalidEscape,
    UnexpectedCharacter,
    InvalidNumber,
    LiteralTooLarge,
    PathTooDeep,
    TooManyFilters,
    UnknownRoot,
    UnknownField,
    MissingKey,
    UnexpectedSegment,
    InvalidSegment,
    UnknownFunction,
    UnknownFilter,
    WrongArgumentCount,
    WrongArgumentType,
    ArgumentOutOfRange,
    InvalidJsonPath,
    JsonPathTooComplex,
    InvalidDateFormat,
    SecretNotAllowed,
    StepUnknown,
    ForbiddenCharacter,
    OutputTooLarge,
    UnknownName,
    JsonFilterInJsonString,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceTooLarge => "source_too_large",
            Self::TooManyExpressions => "too_many_expressions",
            Self::UnterminatedExpression => "unterminated_expression",
            Self::UnterminatedString => "unterminated_string",
            Self::InvalidEscape => "invalid_escape",
            Self::UnexpectedCharacter => "unexpected_character",
            Self::InvalidNumber => "invalid_number",
            Self::LiteralTooLarge => "literal_too_large",
            Self::PathTooDeep => "path_too_deep",
            Self::TooManyFilters => "too_many_filters",
            Self::UnknownRoot => "unknown_root",
            Self::UnknownField => "unknown_field",
            Self::MissingKey => "missing_key",
            Self::UnexpectedSegment => "unexpected_segment",
            Self::InvalidSegment => "invalid_segment",
            Self::UnknownFunction => "unknown_function",
            Self::UnknownFilter => "unknown_filter",
            Self::WrongArgumentCount => "wrong_argument_count",
            Self::WrongArgumentType => "wrong_argument_type",
            Self::ArgumentOutOfRange => "argument_out_of_range",
            Self::InvalidJsonPath => "invalid_json_path",
            Self::JsonPathTooComplex => "json_path_too_complex",
            Self::InvalidDateFormat => "invalid_date_format",
            Self::SecretNotAllowed => "secret_not_allowed",
            Self::StepUnknown => "step_unknown",
            Self::ForbiddenCharacter => "forbidden_character",
            Self::OutputTooLarge => "output_too_large",
            Self::UnknownName => "unknown_name",
            Self::JsonFilterInJsonString => "json_filter_in_json_string",
        }
    }

    pub fn field_code(self) -> &'static str {
        match self {
            Self::InvalidJsonPath | Self::JsonPathTooComplex => "json_path_invalid",
            Self::SecretNotAllowed => "secret_not_allowed",
            Self::StepUnknown => "step_unknown",
            _ => "template_invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub message: String,
    pub position: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("template is invalid: {} error(s)", .diagnostics.len())]
pub struct TemplateError {
    pub diagnostics: Vec<Diagnostic>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderErrorCode {
    OutputLimitExceeded,
    EvaluationLimitExceeded,
    DeadlineExceeded,
    JsonLookupLimitExceeded,
    JsonDepthExceeded,
    ErrorLimitExceeded,
    ValueTooLarge,
    TypeMismatch,
    InvalidArgument,
    InvalidBase64,
    ForbiddenCharacter,
    InvalidStatus,
    BudgetExhausted,
    JsonNodeLimitExceeded,
    JsonBodyTooLarge,
}

impl RenderErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutputLimitExceeded => "output_limit_exceeded",
            Self::EvaluationLimitExceeded => "evaluation_limit_exceeded",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::JsonLookupLimitExceeded => "json_lookup_limit_exceeded",
            Self::JsonDepthExceeded => "json_depth_exceeded",
            Self::ErrorLimitExceeded => "error_limit_exceeded",
            Self::ValueTooLarge => "value_too_large",
            Self::TypeMismatch => "type_mismatch",
            Self::InvalidArgument => "invalid_argument",
            Self::InvalidBase64 => "invalid_base64",
            Self::ForbiddenCharacter => "forbidden_character",
            Self::InvalidStatus => "invalid_status",
            Self::BudgetExhausted => "budget_exhausted",
            Self::JsonNodeLimitExceeded => "json_node_limit_exceeded",
            Self::JsonBodyTooLarge => "json_body_too_large",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{} at line {}, column {}: {}", .code.as_str(), .position.line, .position.column, .message)]
pub struct RenderError {
    pub code: RenderErrorCode,
    pub message: String,
    pub position: Position,
}
