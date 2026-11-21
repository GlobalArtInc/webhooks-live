use std::fmt;

use crate::error::NameError;

pub const MAX_SECRETS_PER_ENDPOINT: usize = 20;
const MAX_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretName(String);

impl SecretName {
    pub fn parse(text: &str) -> Result<Self, NameError> {
        let bytes = text.as_bytes();
        let valid = bytes.len() <= MAX_NAME_BYTES
            && bytes.split_first().is_some_and(|(first, rest)| {
                first.is_ascii_uppercase()
                    && rest.iter().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_'
                    })
            });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(NameError::Invalid)
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
