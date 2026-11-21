use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

pub const ENDPOINT_ID_LENGTH: usize = 24;
const CAPTURE_ID_LENGTH: usize = 36;
const MAX_TOKEN_BYTES: usize = 512;
const HTTP_KIND: &str = "http";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error(
        "an endpoint id has {ENDPOINT_ID_LENGTH} characters from 0-9 and a-z without i, l, o, u"
    )]
    EndpointId,
    #[error("a capture id must be a lowercase hyphenated UUID")]
    CaptureId,
    #[error(
        "a token must be non-empty visible ASCII without spaces, at most {MAX_TOKEN_BYTES} bytes"
    )]
    Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointId(String);

impl EndpointId {
    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl FromStr for EndpointId {
    type Err = ModelError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = text.len() == ENDPOINT_ID_LENGTH
            && text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'h' | b'j' | b'k' | b'm' | b'n' | b'p'..=b't' | b'v'..=b'z'));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ModelError::EndpointId)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureId(String);

impl CaptureId {
    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl FromStr for CaptureId {
    type Err = ModelError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = text.len() == CAPTURE_ID_LENGTH
            && text.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
                }
            });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ModelError::CaptureId)
        }
    }
}

impl<'de> Deserialize<'de> for CaptureId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl FromStr for Token {
    type Err = ModelError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = !text.is_empty()
            && text.len() <= MAX_TOKEN_BYTES
            && text.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ModelError::Token)
        }
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(<redacted>)")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ready {
    pub endpoint_id: String,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureSummary {
    pub id: CaptureId,
    pub seq: u64,
    pub method: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub body_size: u64,
}

impl CaptureSummary {
    pub fn is_http(&self) -> bool {
        self.kind.as_deref().is_none_or(|kind| kind == HTTP_KIND)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureDetail {
    pub method: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub query_string: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapturePage {
    pub items: Vec<CaptureSummary>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_well_formed_endpoint_id() {
        assert!("7k2m9q4xw8c1d5f3h6j0n2p4".parse::<EndpointId>().is_ok());
    }

    #[test]
    fn refuses_an_endpoint_id_with_a_forbidden_letter_or_length() {
        assert!("7k2m9q4xw8c1d5f3h6j0n2pl".parse::<EndpointId>().is_err());
        assert!("7k2m9q4xw8c1d5f3h6j0n2p".parse::<EndpointId>().is_err());
    }

    #[test]
    fn refuses_a_capture_id_that_is_not_a_lowercase_uuid() {
        assert!(
            "0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b"
                .parse::<CaptureId>()
                .is_ok()
        );
        assert!(
            "../../tokens/0193a1c2-9d10-7a3e-8b44"
                .parse::<CaptureId>()
                .is_err()
        );
        assert!(
            "0193A1C2-9D10-7A3E-8B44-1C2D3E4F5A6B"
                .parse::<CaptureId>()
                .is_err()
        );
    }

    #[test]
    fn keeps_a_token_out_of_debug_output() {
        let token: Token = "whl_secret".parse().expect("valid token");
        assert!(!format!("{token:?}").contains("secret"));
        assert_eq!(token.unpack(), "whl_secret");
    }

    #[test]
    fn refuses_a_token_with_whitespace() {
        assert!("whl_a b".parse::<Token>().is_err());
        assert!("".parse::<Token>().is_err());
    }

    #[test]
    fn treats_a_summary_without_a_kind_as_http() {
        let summary: CaptureSummary = serde_json::from_str(
            r#"{"id":"0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b","seq":3,"method":"GET"}"#,
        )
        .expect("valid summary");
        assert!(summary.is_http());
    }
}
