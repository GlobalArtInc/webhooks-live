use std::collections::HashSet;
use std::time::Duration;

use crate::content_type::ContentType;
use crate::header::{HeaderInput, HeaderName, HeaderValue, MAX_HEADERS, ResponseHeader};
use crate::status::{HttpStatus, STATUS_MAX, STATUS_MIN};
use crate::validation::{
    FieldPath, InvalidValue, MAX_VIOLATIONS, ValidationError, Violation, ViolationCode,
};

pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_DELAY_MS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseConfigInput {
    pub status: i64,
    pub content_type: String,
    pub headers: Vec<HeaderInput>,
    pub body: Vec<u8>,
    pub delay_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponseConfig {
    status: HttpStatus,
    content_type: ContentType,
    headers: Vec<ResponseHeader>,
    body: Vec<u8>,
    delay: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposedResponse<'a> {
    pub status: HttpStatus,
    pub content_type: &'a ContentType,
    pub headers: &'a [ResponseHeader],
}

impl ResponseConfig {
    pub fn validate(input: ResponseConfigInput) -> Result<Self, ValidationError> {
        let mut violations = Vec::new();

        let status = HttpStatus::new(input.status);
        if status.is_none() {
            violations.push(Violation {
                field: FieldPath::Status,
                code: ViolationCode::OutOfRange {
                    min: i64::from(STATUS_MIN),
                    max: i64::from(STATUS_MAX),
                },
            });
        }

        let content_type = ContentType::parse(&input.content_type);
        if let Err(error) = &content_type {
            record(&mut violations, FieldPath::ContentType, error);
        }

        let headers = validate_headers(&input.headers, &mut violations);

        if input.body.len() > MAX_BODY_BYTES {
            violations.push(Violation {
                field: FieldPath::Body,
                code: ViolationCode::TooLong {
                    max: MAX_BODY_BYTES,
                },
            });
        }

        let delay = u64::try_from(input.delay_ms)
            .ok()
            .filter(|&millis| millis <= MAX_DELAY_MS.unsigned_abs())
            .map(Duration::from_millis);
        if delay.is_none() {
            violations.push(Violation {
                field: FieldPath::DelayMs,
                code: ViolationCode::OutOfRange {
                    min: 0,
                    max: MAX_DELAY_MS,
                },
            });
        }

        match (status, content_type, delay) {
            (Some(status), Ok(content_type), Some(delay)) if violations.is_empty() => Ok(Self {
                status,
                content_type,
                headers,
                body: input.body,
                delay,
            }),
            _ => Err(ValidationError::new(violations)),
        }
    }

    pub fn status(&self) -> HttpStatus {
        self.status
    }

    pub fn content_type(&self) -> &ContentType {
        &self.content_type
    }

    pub fn headers(&self) -> &[ResponseHeader] {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn delay(&self) -> Duration {
        self.delay
    }
}

pub fn compose_response(
    config: &ResponseConfig,
    status_override: Option<HttpStatus>,
) -> ComposedResponse<'_> {
    ComposedResponse {
        status: status_override.unwrap_or(config.status),
        content_type: &config.content_type,
        headers: &config.headers,
    }
}

fn validate_headers(
    inputs: &[HeaderInput],
    violations: &mut Vec<Violation>,
) -> Vec<ResponseHeader> {
    let too_many = inputs.len() > MAX_HEADERS;
    if too_many {
        violations.push(Violation {
            field: FieldPath::Headers,
            code: ViolationCode::TooMany { max: MAX_HEADERS },
        });
    }

    let mut seen = HashSet::new();
    let mut accepted = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if violations.len() >= MAX_VIOLATIONS {
            break;
        }
        let name = HeaderName::parse(&input.name);
        if let Err(error) = &name {
            record(violations, FieldPath::HeaderName(index), error);
        }
        if !seen.insert(input.name.to_ascii_lowercase()) {
            violations.push(Violation {
                field: FieldPath::HeaderName(index),
                code: ViolationCode::Duplicate,
            });
        }
        let value = HeaderValue::parse(&input.value);
        if let Err(error) = &value {
            record(violations, FieldPath::HeaderValue(index), error);
        }
        if let (Ok(name), Ok(value)) = (name, value)
            && !too_many
        {
            accepted.push(ResponseHeader { name, value });
        }
    }
    accepted
}

fn record(violations: &mut Vec<Violation>, field: FieldPath, error: &InvalidValue) {
    violations.extend(error.codes().iter().map(|&code| Violation { field, code }));
}
