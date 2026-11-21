use crate::validation::{InvalidValue, ViolationCode};

pub const MAX_HEADERS: usize = 20;
pub const MAX_HEADER_NAME_BYTES: usize = 64;
pub const MAX_HEADER_VALUE_BYTES: usize = 1024;

const DENIED_NAMES: &[&str] = &[
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "trailer",
    "te",
    "host",
    "set-cookie",
    "x-content-type-options",
    "x-request-id",
    "x-endpoint-id",
    "content-type",
    "set-cookie2",
    "clear-site-data",
    "strict-transport-security",
    "cache-control",
    "alt-svc",
    "refresh",
    "content-encoding",
    "x-frame-options",
    "referrer-policy",
    "permissions-policy",
    "service-worker-allowed",
    "public-key-pins",
    "public-key-pins-report-only",
    "report-to",
    "nel",
    "origin-trial",
    "document-policy",
];

const DENIED_PREFIXES: &[&str] = &[
    "proxy-",
    "content-security-policy",
    "access-control-",
    "cross-origin-",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderName(String);

impl HeaderName {
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let mut codes = Vec::new();
        if text.is_empty() {
            codes.push(ViolationCode::Empty);
        } else {
            if text.len() > MAX_HEADER_NAME_BYTES {
                codes.push(ViolationCode::TooLong {
                    max: MAX_HEADER_NAME_BYTES,
                });
            }
            if !text.bytes().all(is_token_byte) {
                codes.push(ViolationCode::InvalidToken);
            } else if is_denied(text) {
                codes.push(ViolationCode::Denied);
            }
        }
        match InvalidValue::from_codes(codes) {
            Some(error) => Err(error),
            None => Ok(Self(text.to_owned())),
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderValue(String);

impl HeaderValue {
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let mut codes = Vec::new();
        if text.len() > MAX_HEADER_VALUE_BYTES {
            codes.push(ViolationCode::TooLong {
                max: MAX_HEADER_VALUE_BYTES,
            });
        }
        if text.bytes().any(is_control_byte) {
            codes.push(ViolationCode::ControlCharacter);
        }
        if !text.is_ascii() {
            codes.push(ViolationCode::NonAscii);
        }
        match InvalidValue::from_codes(codes) {
            Some(error) => Err(error),
            None => Ok(Self(text.to_owned())),
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHeader {
    pub name: HeaderName,
    pub value: HeaderValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderInput {
    pub name: String,
    pub value: String,
}

pub(crate) fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_control_byte(byte: u8) -> bool {
    (byte < 0x20 && byte != b'\t') || byte == 0x7f
}

fn is_denied(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    DENIED_NAMES.contains(&lower.as_str())
        || DENIED_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}
