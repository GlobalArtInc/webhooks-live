use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::{Builder, Uuid, Variant};

use crate::entropy::{IdGenerationError, random_bytes};

const UUID_V7: usize = 7;
const RANDOM_BYTES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureIdError {
    #[error("capture id must be a lowercase hyphenated UUID")]
    Format,
    #[error("capture id must be a UUID version 7 with the RFC 4122 variant")]
    Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CaptureId(Uuid);

impl CaptureId {
    pub fn generate() -> Result<Self, IdGenerationError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| IdGenerationError::Clock)?
            .as_millis();
        let millis = u64::try_from(millis).map_err(|_| IdGenerationError::Clock)?;
        let random = random_bytes::<RANDOM_BYTES>()?;
        Ok(Self(
            Builder::from_unix_timestamp_millis(millis, &random).into_uuid(),
        ))
    }

    pub fn parse(text: &str) -> Result<Self, CaptureIdError> {
        let uuid = Uuid::try_parse(text).map_err(|_| CaptureIdError::Format)?;
        if uuid.hyphenated().to_string() != text {
            return Err(CaptureIdError::Format);
        }
        if uuid.get_version_num() != UUID_V7 || uuid.get_variant() != Variant::RFC4122 {
            return Err(CaptureIdError::Version);
        }
        Ok(Self(uuid))
    }

    pub fn unpack(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for CaptureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl FromStr for CaptureId {
    type Err = CaptureIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl TryFrom<String> for CaptureId {
    type Error = CaptureIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<CaptureId> for String {
    fn from(id: CaptureId) -> Self {
        id.to_string()
    }
}
