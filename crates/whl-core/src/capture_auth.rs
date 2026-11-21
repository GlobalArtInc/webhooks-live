use crate::header::{MAX_HEADER_NAME_BYTES, is_token};

pub const MAX_CREDENTIAL_BYTES: usize = 256;
pub const MIN_SECRET_BYTES: usize = 8;
pub const MAX_AUTH_HEADER_NAME_BYTES: usize = MAX_HEADER_NAME_BYTES;

const DENIED_HEADER_NAMES: &[&str] = &[
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];
const DENIED_HEADER_PREFIXES: &[&str] = &["proxy-"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("must not be empty")]
    Empty,
    #[error("must be at least {MIN_SECRET_BYTES} bytes")]
    TooShort,
    #[error("must be at most {MAX_CREDENTIAL_BYTES} bytes")]
    TooLong,
    #[error("must contain only visible ASCII characters without spaces")]
    NotVisibleAscii,
    #[error("must contain only printable ASCII characters")]
    NotPrintableAscii,
    #[error("must not start or end with a space")]
    EdgeSpace,
    #[error("must not contain a colon")]
    Colon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthHeaderNameError {
    #[error("must not be empty")]
    Empty,
    #[error("must be at most {MAX_AUTH_HEADER_NAME_BYTES} bytes")]
    TooLong,
    #[error("must be a valid HTTP token")]
    InvalidToken,
    #[error("is a header that proxies rewrite or that frames the message")]
    Denied,
}

fn check_length(text: &str, min: usize) -> Result<(), CredentialError> {
    if text.is_empty() {
        return Err(CredentialError::Empty);
    }
    if text.len() < min {
        return Err(CredentialError::TooShort);
    }
    if text.len() > MAX_CREDENTIAL_BYTES {
        return Err(CredentialError::TooLong);
    }
    Ok(())
}

fn check_visible(text: &str) -> Result<(), CredentialError> {
    if !text.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(CredentialError::NotVisibleAscii);
    }
    Ok(())
}

pub fn check_basic_password(text: &str) -> Result<(), CredentialError> {
    check_length(text, MIN_SECRET_BYTES)?;
    check_visible(text)
}

pub fn check_header_value(text: &str) -> Result<(), CredentialError> {
    check_length(text, MIN_SECRET_BYTES)?;
    if !text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(CredentialError::NotPrintableAscii);
    }
    if text.starts_with(' ') || text.ends_with(' ') {
        return Err(CredentialError::EdgeSpace);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthUsername(String);

impl AuthUsername {
    pub fn parse(text: &str) -> Result<Self, CredentialError> {
        check_length(text, 1)?;
        check_visible(text)?;
        if text.contains(':') {
            return Err(CredentialError::Colon);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHeaderName(String);

impl AuthHeaderName {
    pub fn parse(text: &str) -> Result<Self, AuthHeaderNameError> {
        if text.is_empty() {
            return Err(AuthHeaderNameError::Empty);
        }
        if text.len() > MAX_AUTH_HEADER_NAME_BYTES {
            return Err(AuthHeaderNameError::TooLong);
        }
        if !is_token(text) {
            return Err(AuthHeaderNameError::InvalidToken);
        }
        let lower = text.to_ascii_lowercase();
        if DENIED_HEADER_NAMES.contains(&lower.as_str())
            || DENIED_HEADER_PREFIXES
                .iter()
                .any(|prefix| lower.starts_with(prefix))
        {
            return Err(AuthHeaderNameError::Denied);
        }
        Ok(Self(lower))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}
