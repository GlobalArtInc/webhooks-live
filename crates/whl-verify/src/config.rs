use http::HeaderName;
use serde::{Deserialize, Serialize};
use whl_secrets::SecretName;

use crate::algorithm::{Algorithm, Encoding};

pub const MAX_HEADER_NAME_BYTES: usize = 64;
pub const MAX_PREFIX_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Hmac,
    Github,
    Stripe,
    Slack,
    Shopify,
    StandardWebhooks,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hmac => "hmac",
            Self::Github => "github",
            Self::Stripe => "stripe",
            Self::Slack => "slack",
            Self::Shopify => "shopify",
            Self::StandardWebhooks => "standard_webhooks",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "hmac" => Some(Self::Hmac),
            "github" => Some(Self::Github),
            "stripe" => Some(Self::Stripe),
            "slack" => Some(Self::Slack),
            "shopify" => Some(Self::Shopify),
            "standard_webhooks" => Some(Self::StandardWebhooks),
            _ => None,
        }
    }

    fn preset(self) -> Option<Preset> {
        match self {
            Self::Hmac => None,
            Self::Github => Some(Preset {
                header: "x-hub-signature-256",
                algorithm: Algorithm::Sha256,
                encoding: Encoding::Hex,
                prefix: Some("sha256="),
                timestamp_header: None,
            }),
            Self::Shopify => Some(Preset {
                header: "x-shopify-hmac-sha256",
                algorithm: Algorithm::Sha256,
                encoding: Encoding::Base64,
                prefix: None,
                timestamp_header: None,
            }),
            Self::Stripe => Some(Preset {
                header: "stripe-signature",
                algorithm: Algorithm::Sha256,
                encoding: Encoding::Hex,
                prefix: None,
                timestamp_header: None,
            }),
            Self::Slack => Some(Preset {
                header: "x-slack-signature",
                algorithm: Algorithm::Sha256,
                encoding: Encoding::Hex,
                prefix: Some("v0="),
                timestamp_header: Some("x-slack-request-timestamp"),
            }),
            Self::StandardWebhooks => Some(Preset {
                header: "webhook-signature",
                algorithm: Algorithm::Sha256,
                encoding: Encoding::Base64,
                prefix: Some("v1,"),
                timestamp_header: Some("webhook-timestamp"),
            }),
        }
    }
}

struct Preset {
    header: &'static str,
    algorithm: Algorithm,
    encoding: Encoding,
    prefix: Option<&'static str>,
    timestamp_header: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigProblem {
    #[error("is required")]
    Required,
    #[error("has an invalid format")]
    InvalidFormat,
    #[error("is too long")]
    TooLong,
    #[error("is not allowed for this scheme")]
    NotAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigIssue {
    pub field: &'static str,
    pub problem: ConfigProblem,
}

impl ConfigIssue {
    fn new(field: &'static str, problem: ConfigProblem) -> Self {
        Self { field, problem }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifierInput {
    pub scheme: String,
    pub secret_name: String,
    pub header: Option<String>,
    pub algorithm: Option<String>,
    pub encoding: Option<String>,
    pub prefix: Option<String>,
    pub timestamp_header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierDocument {
    pub scheme: String,
    pub secret_name: String,
    pub header: Option<String>,
    pub algorithm: Option<String>,
    pub encoding: Option<String>,
    pub prefix: Option<String>,
    pub timestamp_header: Option<String>,
}

impl From<VerifierDocument> for VerifierInput {
    fn from(document: VerifierDocument) -> Self {
        Self {
            scheme: document.scheme,
            secret_name: document.secret_name,
            header: document.header,
            algorithm: document.algorithm,
            encoding: document.encoding,
            prefix: document.prefix,
            timestamp_header: document.timestamp_header,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verifier {
    pub(crate) scheme: Scheme,
    pub(crate) secret_name: SecretName,
    pub(crate) header: HeaderName,
    pub(crate) algorithm: Algorithm,
    pub(crate) encoding: Encoding,
    pub(crate) prefix: Option<String>,
    pub(crate) timestamp_header: Option<HeaderName>,
}

impl Verifier {
    pub fn new(input: &VerifierInput) -> Result<Self, Vec<ConfigIssue>> {
        let mut issues = Vec::new();
        let scheme = Scheme::parse(&input.scheme);
        if scheme.is_none() {
            issues.push(ConfigIssue::new("scheme", ConfigProblem::InvalidFormat));
        }
        let secret_name = match SecretName::parse(&input.secret_name) {
            Ok(name) => Some(name),
            Err(_) => {
                issues.push(ConfigIssue::new(
                    "secret_name",
                    ConfigProblem::InvalidFormat,
                ));
                None
            }
        };
        let header = parse_header(input.header.as_deref(), "header", &mut issues);
        let timestamp_header = parse_header(
            input.timestamp_header.as_deref(),
            "timestamp_header",
            &mut issues,
        );
        let algorithm = parse_choice(
            input.algorithm.as_deref(),
            "algorithm",
            Algorithm::parse,
            &mut issues,
        );
        let encoding = parse_choice(
            input.encoding.as_deref(),
            "encoding",
            Encoding::parse,
            &mut issues,
        );
        let prefix = parse_prefix(input.prefix.as_deref(), &mut issues);
        let (Some(scheme), Some(secret_name)) = (scheme, secret_name) else {
            return Err(issues);
        };
        let resolved = resolve(
            scheme,
            Given {
                header,
                algorithm,
                encoding,
                prefix,
                timestamp_header,
            },
            &mut issues,
        );
        match resolved {
            Some(resolved) if issues.is_empty() => Ok(Self {
                scheme,
                secret_name,
                header: resolved.header,
                algorithm: resolved.algorithm,
                encoding: resolved.encoding,
                prefix: resolved.prefix,
                timestamp_header: resolved.timestamp_header,
            }),
            _ => Err(issues),
        }
    }

    pub fn from_document(document: &VerifierDocument) -> Result<Self, Vec<ConfigIssue>> {
        Self::new(&VerifierInput::from(document.clone()))
    }

    pub fn document(&self) -> VerifierDocument {
        VerifierDocument {
            scheme: self.scheme.as_str().to_owned(),
            secret_name: self.secret_name.unpack().to_owned(),
            header: Some(self.header.as_str().to_owned()),
            algorithm: Some(self.algorithm.as_str().to_owned()),
            encoding: Some(self.encoding.as_str().to_owned()),
            prefix: self.prefix.clone(),
            timestamp_header: self
                .timestamp_header
                .as_ref()
                .map(|name| name.as_str().to_owned()),
        }
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn secret_name(&self) -> &SecretName {
        &self.secret_name
    }
}

struct Given {
    header: Option<HeaderName>,
    algorithm: Option<Algorithm>,
    encoding: Option<Encoding>,
    prefix: Option<String>,
    timestamp_header: Option<HeaderName>,
}

struct Resolved {
    header: HeaderName,
    algorithm: Algorithm,
    encoding: Encoding,
    prefix: Option<String>,
    timestamp_header: Option<HeaderName>,
}

fn resolve(scheme: Scheme, given: Given, issues: &mut Vec<ConfigIssue>) -> Option<Resolved> {
    let Some(preset) = scheme.preset() else {
        return resolve_generic(given, issues);
    };
    let header = HeaderName::from_static(preset.header);
    let timestamp_header = preset.timestamp_header.map(HeaderName::from_static);
    if given.header.as_ref().is_some_and(|value| *value != header) {
        issues.push(ConfigIssue::new("header", ConfigProblem::NotAllowed));
    }
    if given
        .algorithm
        .is_some_and(|value| value != preset.algorithm)
    {
        issues.push(ConfigIssue::new("algorithm", ConfigProblem::NotAllowed));
    }
    if given.encoding.is_some_and(|value| value != preset.encoding) {
        issues.push(ConfigIssue::new("encoding", ConfigProblem::NotAllowed));
    }
    if given
        .prefix
        .as_deref()
        .is_some_and(|value| Some(value) != preset.prefix)
    {
        issues.push(ConfigIssue::new("prefix", ConfigProblem::NotAllowed));
    }
    if given
        .timestamp_header
        .as_ref()
        .is_some_and(|value| Some(value) != timestamp_header.as_ref())
    {
        issues.push(ConfigIssue::new(
            "timestamp_header",
            ConfigProblem::NotAllowed,
        ));
    }
    Some(Resolved {
        header,
        algorithm: preset.algorithm,
        encoding: preset.encoding,
        prefix: preset.prefix.map(str::to_owned),
        timestamp_header,
    })
}

fn resolve_generic(given: Given, issues: &mut Vec<ConfigIssue>) -> Option<Resolved> {
    let header = require(given.header, "header", issues);
    let algorithm = require(given.algorithm, "algorithm", issues);
    let encoding = require(given.encoding, "encoding", issues);
    let (header, algorithm, encoding) = (header?, algorithm?, encoding?);
    if given.timestamp_header.as_ref() == Some(&header) {
        issues.push(ConfigIssue::new(
            "timestamp_header",
            ConfigProblem::InvalidFormat,
        ));
    }
    Some(Resolved {
        header,
        algorithm,
        encoding,
        prefix: given.prefix,
        timestamp_header: given.timestamp_header,
    })
}

fn require<T>(value: Option<T>, field: &'static str, issues: &mut Vec<ConfigIssue>) -> Option<T> {
    if value.is_none() && !issues.iter().any(|issue| issue.field == field) {
        issues.push(ConfigIssue::new(field, ConfigProblem::Required));
    }
    value
}

fn parse_header(
    text: Option<&str>,
    field: &'static str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<HeaderName> {
    let text = text?;
    if text.len() > MAX_HEADER_NAME_BYTES {
        issues.push(ConfigIssue::new(field, ConfigProblem::TooLong));
        return None;
    }
    match HeaderName::from_bytes(text.as_bytes()) {
        Ok(name) => Some(name),
        Err(_) => {
            issues.push(ConfigIssue::new(field, ConfigProblem::InvalidFormat));
            None
        }
    }
}

fn parse_choice<T>(
    text: Option<&str>,
    field: &'static str,
    parse: fn(&str) -> Option<T>,
    issues: &mut Vec<ConfigIssue>,
) -> Option<T> {
    let text = text?;
    let parsed = parse(text);
    if parsed.is_none() {
        issues.push(ConfigIssue::new(field, ConfigProblem::InvalidFormat));
    }
    parsed
}

fn parse_prefix(text: Option<&str>, issues: &mut Vec<ConfigIssue>) -> Option<String> {
    let text = text?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_graphic()) {
        issues.push(ConfigIssue::new("prefix", ConfigProblem::InvalidFormat));
        return None;
    }
    if text.len() > MAX_PREFIX_BYTES {
        issues.push(ConfigIssue::new("prefix", ConfigProblem::TooLong));
        return None;
    }
    Some(text.to_owned())
}
