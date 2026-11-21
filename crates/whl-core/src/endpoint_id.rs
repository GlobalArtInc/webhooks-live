use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::entropy::{IdGenerationError, random_bytes};

pub const ENDPOINT_ID_LEN: usize = 24;
pub const ENDPOINT_ID_ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

const ENTROPY_BYTES: usize = 15;
const BITS_PER_CHAR: usize = 5;
const CHAR_MASK: u128 = 0x1f;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointIdError {
    #[error("endpoint id must be {ENDPOINT_ID_LEN} bytes long, got {actual}")]
    Length { actual: usize },
    #[error("endpoint id has a character outside the alphabet at byte {index}")]
    Character { index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EndpointId(String);

impl EndpointId {
    pub fn generate() -> Result<Self, IdGenerationError> {
        let bytes = random_bytes::<ENTROPY_BYTES>()?;
        let mut wide = [0_u8; 16];
        wide[1..].copy_from_slice(&bytes);
        let bits = u128::from_be_bytes(wide);

        let encoded = (0..ENDPOINT_ID_LEN)
            .map(|position| {
                let shift = BITS_PER_CHAR * (ENDPOINT_ID_LEN - 1 - position);
                let index = ((bits >> shift) & CHAR_MASK) as usize;
                char::from(ENDPOINT_ID_ALPHABET[index])
            })
            .collect();
        Ok(Self(encoded))
    }

    pub fn parse(text: &str) -> Result<Self, EndpointIdError> {
        if text.len() != ENDPOINT_ID_LEN {
            return Err(EndpointIdError::Length { actual: text.len() });
        }
        if let Some(index) = text
            .bytes()
            .position(|byte| !ENDPOINT_ID_ALPHABET.contains(&byte))
        {
            return Err(EndpointIdError::Character { index });
        }
        Ok(Self(text.to_owned()))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for EndpointId {
    type Err = EndpointIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl TryFrom<String> for EndpointId {
    type Error = EndpointIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<EndpointId> for String {
    fn from(id: EndpointId) -> Self {
        id.0
    }
}
