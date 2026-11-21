use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldPath {
    Status,
    ContentType,
    Headers,
    HeaderName(usize),
    HeaderValue(usize),
    Body,
    DelayMs,
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status => f.write_str("response.status"),
            Self::ContentType => f.write_str("response.content_type"),
            Self::Headers => f.write_str("response.headers"),
            Self::HeaderName(index) => write!(f, "response.headers[{index}].name"),
            Self::HeaderValue(index) => write!(f, "response.headers[{index}].value"),
            Self::Body => f.write_str("response.body"),
            Self::DelayMs => f.write_str("response.delay_ms"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ViolationCode {
    #[error("must be between {min} and {max}")]
    OutOfRange { min: i64, max: i64 },
    #[error("must not be empty")]
    Empty,
    #[error("must be at most {max} bytes")]
    TooLong { max: usize },
    #[error("must have at most {max} entries")]
    TooMany { max: usize },
    #[error("must not contain control characters")]
    ControlCharacter,
    #[error("must contain only ASCII characters")]
    NonAscii,
    #[error("must be a valid HTTP token")]
    InvalidToken,
    #[error("must be a valid media type of the form type/subtype with optional parameters")]
    InvalidMediaType,
    #[error("must not repeat a header name, compared case-insensitively")]
    Duplicate,
    #[error("is not allowed")]
    Denied,
}

impl ViolationCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OutOfRange { .. } => "out_of_range",
            Self::Empty => "empty",
            Self::TooLong { .. } => "too_long",
            Self::TooMany { .. } => "too_many",
            Self::ControlCharacter => "control_character",
            Self::NonAscii => "non_ascii",
            Self::InvalidToken => "invalid_token",
            Self::InvalidMediaType => "invalid_media_type",
            Self::Duplicate => "duplicate",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub field: FieldPath,
    pub code: ViolationCode,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.field, self.code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("validation failed: {}", join(.violations))]
pub struct ValidationError {
    violations: Vec<Violation>,
}

impl ValidationError {
    pub(crate) fn new(violations: Vec<Violation>) -> Self {
        Self { violations }
    }

    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid value: {}", join(.codes))]
pub struct InvalidValue {
    codes: Vec<ViolationCode>,
}

impl InvalidValue {
    pub(crate) fn from_codes(codes: Vec<ViolationCode>) -> Option<Self> {
        if codes.is_empty() {
            None
        } else {
            Some(Self { codes })
        }
    }

    pub fn codes(&self) -> &[ViolationCode] {
        &self.codes
    }
}

fn join<T: ToString>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
