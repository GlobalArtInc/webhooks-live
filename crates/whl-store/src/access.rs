use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use whl_core::capture_auth::{AuthHeaderName, AuthUsername};
use whl_core::sender_allow::{SenderAllowList, SenderRange};
use whl_secrets::{KeyId, MAC_BYTES, constant_time_eq};

use crate::endpoints::{EndpointError, classify};
use crate::store::Store;

const BASIC_HEADER: &str = "authorization";

#[derive(Debug, thiserror::Error)]
pub enum AccessDataError {
    #[error("the capture auth document is malformed")]
    Document,
    #[error("the capture auth document names an invalid key id")]
    KeyId,
    #[error("the capture auth document holds a malformed authentication code")]
    Mac,
    #[error("the capture auth document holds an invalid user name")]
    Username,
    #[error("the capture auth document holds an invalid header name")]
    HeaderName,
    #[error("the stored sender range at position {0} is invalid")]
    SenderRange(usize),
    #[error("the stored sender allow list is longer than the limit")]
    SenderList,
}

#[derive(Clone)]
pub struct StoredMac([u8; MAC_BYTES]);

impl StoredMac {
    pub fn new(bytes: [u8; MAC_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn unpack(&self) -> &[u8; MAC_BYTES] {
        &self.0
    }
}

impl PartialEq for StoredMac {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for StoredMac {}

impl fmt::Debug for StoredMac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoredMac(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureAuthRecord {
    Basic {
        key_id: KeyId,
        username: AuthUsername,
        password_mac: StoredMac,
    },
    Header {
        key_id: KeyId,
        name: AuthHeaderName,
        value_mac: StoredMac,
    },
}

impl CaptureAuthRecord {
    pub fn key_id(&self) -> KeyId {
        match self {
            Self::Basic { key_id, .. } | Self::Header { key_id, .. } => *key_id,
        }
    }

    pub fn credential_header(&self) -> &str {
        match self {
            Self::Basic { .. } => BASIC_HEADER,
            Self::Header { name, .. } => name.unpack(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureAuthKeyUse {
    pub key_id: Option<KeyId>,
    pub endpoints: u64,
}

impl Store {
    pub async fn capture_auth_key_usage(&self) -> Result<Vec<CaptureAuthKeyUse>, EndpointError> {
        let rows = sqlx::query(
            "SELECT capture_auth ->> 'key_id' AS key_id, count(*) AS endpoints \
             FROM endpoints WHERE capture_auth IS NOT NULL AND deleted_at IS NULL \
             GROUP BY 1 ORDER BY 1",
        )
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;
        rows.iter()
            .map(|row| {
                let key_id: Option<String> = row.try_get("key_id").map_err(EndpointError::Query)?;
                let endpoints: i64 = row.try_get("endpoints").map_err(EndpointError::Query)?;
                Ok(CaptureAuthKeyUse {
                    key_id: key_id
                        .and_then(|text| text.parse::<i16>().ok())
                        .and_then(|column| KeyId::from_column(column).ok()),
                    endpoints: u64::try_from(endpoints).unwrap_or(0),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccessSettings {
    pub sender_allow: SenderAllowList,
    pub capture_auth: Option<CaptureAuthRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Document {
    Basic {
        key_id: i16,
        username: String,
        mac: String,
    },
    Header {
        key_id: i16,
        name: String,
        mac: String,
    },
}

impl CaptureAuthRecord {
    pub(crate) fn to_document(&self) -> Result<String, AccessDataError> {
        let document = match self {
            Self::Basic {
                key_id,
                username,
                password_mac,
            } => Document::Basic {
                key_id: key_id.to_column(),
                username: username.unpack().to_owned(),
                mac: STANDARD.encode(password_mac.unpack()),
            },
            Self::Header {
                key_id,
                name,
                value_mac,
            } => Document::Header {
                key_id: key_id.to_column(),
                name: name.unpack().to_owned(),
                mac: STANDARD.encode(value_mac.unpack()),
            },
        };
        serde_json::to_string(&document).map_err(|_| AccessDataError::Document)
    }

    pub(crate) fn from_document(text: &str) -> Result<Self, AccessDataError> {
        let document: Document =
            serde_json::from_str(text).map_err(|_| AccessDataError::Document)?;
        match document {
            Document::Basic {
                key_id,
                username,
                mac,
            } => Ok(Self::Basic {
                key_id: KeyId::from_column(key_id).map_err(|_| AccessDataError::KeyId)?,
                username: AuthUsername::parse(&username).map_err(|_| AccessDataError::Username)?,
                password_mac: decode_mac(&mac)?,
            }),
            Document::Header { key_id, name, mac } => Ok(Self::Header {
                key_id: KeyId::from_column(key_id).map_err(|_| AccessDataError::KeyId)?,
                name: AuthHeaderName::parse(&name).map_err(|_| AccessDataError::HeaderName)?,
                value_mac: decode_mac(&mac)?,
            }),
        }
    }
}

fn decode_mac(text: &str) -> Result<StoredMac, AccessDataError> {
    let bytes = STANDARD.decode(text).map_err(|_| AccessDataError::Mac)?;
    let fixed = <[u8; MAC_BYTES]>::try_from(bytes.as_slice()).map_err(|_| AccessDataError::Mac)?;
    Ok(StoredMac::new(fixed))
}

pub(crate) fn ranges_to_column(list: &SenderAllowList) -> Vec<String> {
    list.ranges().iter().map(ToString::to_string).collect()
}

pub(crate) fn access_from_columns(
    ranges: &[String],
    document: Option<&str>,
) -> Result<AccessSettings, AccessDataError> {
    let parsed = ranges
        .iter()
        .enumerate()
        .map(|(index, text)| {
            SenderRange::parse(text).map_err(|_| AccessDataError::SenderRange(index))
        })
        .collect::<Result<Vec<SenderRange>, _>>()?;
    let sender_allow = SenderAllowList::new(parsed).map_err(|_| AccessDataError::SenderList)?;
    let capture_auth = document.map(CaptureAuthRecord::from_document).transpose()?;
    Ok(AccessSettings {
        sender_allow,
        capture_auth,
    })
}
