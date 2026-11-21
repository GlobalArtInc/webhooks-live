use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use http::{HeaderMap, HeaderName};
use time::OffsetDateTime;
use whl_secrets::{Secret, constant_time_eq};
use zeroize::Zeroizing;

use crate::config::{Scheme, Verifier};
use crate::verdict::{Reason, Verdict};

pub const TOLERANCE_SECONDS: i64 = 300;
pub const MAX_SIGNATURES: usize = 8;
pub const MAX_HEADER_ITEMS: usize = 16;
pub const MAX_SIGNATURE_TEXT_BYTES: usize = 512;
pub const MAX_TIMESTAMP_DIGITS: usize = 12;
pub const MAX_MESSAGE_ID_BYTES: usize = 256;

const SECRET_PREFIX: &str = "whsec_";
const MESSAGE_ID_HEADER: HeaderName = HeaderName::from_static("webhook-id");

#[derive(Debug, Clone, Copy)]
pub struct SignedRequest<'a> {
    pub headers: &'a HeaderMap,
    pub body: &'a [u8],
    pub received_at: OffsetDateTime,
}

#[derive(Clone, Copy)]
enum Layout {
    Plain,
    Dotted,
    Stripe,
    Slack,
    StandardWebhooks,
}

struct Extracted<'a> {
    tokens: Vec<&'a str>,
    timestamp: Option<&'a str>,
    message_id: Option<&'a str>,
}

enum HeaderText<'a> {
    Absent,
    Malformed,
    Present(&'a str),
}

impl Verifier {
    pub fn signature_present(&self, headers: &HeaderMap) -> bool {
        headers.contains_key(&self.header)
    }

    pub fn verify(&self, secret: &Secret, request: &SignedRequest<'_>) -> Verdict {
        if !self.signature_present(request.headers) {
            return Verdict::missing();
        }
        let extracted = match self.extract(request.headers) {
            Ok(extracted) => extracted,
            Err(reason) => return Verdict::invalid(reason),
        };
        let signatures = self.decode_signatures(&extracted.tokens);
        if signatures.is_empty() {
            return Verdict::invalid(Reason::SignatureMalformed);
        }
        if let Some(timestamp) = extracted.timestamp
            && let Err(reason) = within_tolerance(timestamp, request.received_at)
        {
            return Verdict::invalid(reason);
        }
        let Some(key) = self.key_bytes(secret) else {
            return Verdict::unusable_secret();
        };
        let parts = self.signed_parts(&extracted, request.body);
        let Some(expected) = self.algorithm.mac(&key, &parts) else {
            return Verdict::unusable_secret();
        };
        let matched = signatures.iter().fold(false, |found, candidate| {
            found | constant_time_eq(candidate, &expected)
        });
        if matched {
            Verdict::valid()
        } else {
            Verdict::invalid(Reason::Mismatch)
        }
    }

    fn layout(&self) -> Layout {
        match self.scheme {
            Scheme::Stripe => Layout::Stripe,
            Scheme::Slack => Layout::Slack,
            Scheme::StandardWebhooks => Layout::StandardWebhooks,
            Scheme::Hmac | Scheme::Github | Scheme::Shopify => {
                if self.timestamp_header.is_some() {
                    Layout::Dotted
                } else {
                    Layout::Plain
                }
            }
        }
    }

    fn extract<'a>(&self, headers: &'a HeaderMap) -> Result<Extracted<'a>, Reason> {
        let layout = self.layout();
        let mut timestamp = None;
        let mut message_id = None;
        let tokens = match layout {
            Layout::Stripe => {
                let (tokens, stamp) = stripe_tokens(headers, &self.header)?;
                timestamp = Some(stamp);
                tokens
            }
            Layout::Plain | Layout::Dotted | Layout::Slack | Layout::StandardWebhooks => {
                let split = matches!(layout, Layout::StandardWebhooks);
                header_tokens(headers, &self.header, split)?
            }
        };
        if let Some(name) = &self.timestamp_header {
            timestamp = Some(match single_header(headers, name) {
                HeaderText::Present(text) => text,
                HeaderText::Absent => return Err(Reason::TimestampAbsent),
                HeaderText::Malformed => return Err(Reason::TimestampMalformed),
            });
        }
        if matches!(layout, Layout::StandardWebhooks) {
            message_id = Some(match single_header(headers, &MESSAGE_ID_HEADER) {
                HeaderText::Present(text) if text.len() <= MAX_MESSAGE_ID_BYTES => text,
                HeaderText::Present(_) | HeaderText::Malformed => {
                    return Err(Reason::MessageIdMalformed);
                }
                HeaderText::Absent => return Err(Reason::MessageIdAbsent),
            });
        }
        Ok(Extracted {
            tokens,
            timestamp,
            message_id,
        })
    }

    fn decode_signatures(&self, tokens: &[&str]) -> Vec<Vec<u8>> {
        let expected_len = self.algorithm.output_bytes();
        tokens
            .iter()
            .filter_map(|token| {
                let encoded = match &self.prefix {
                    Some(prefix) => token.strip_prefix(prefix.as_str())?,
                    None => *token,
                };
                if encoded.len() > MAX_SIGNATURE_TEXT_BYTES {
                    return None;
                }
                self.encoding
                    .decode(encoded)
                    .filter(|bytes| bytes.len() == expected_len)
            })
            .take(MAX_SIGNATURES)
            .collect()
    }

    fn key_bytes(&self, secret: &Secret) -> Option<Zeroizing<Vec<u8>>> {
        let text = secret.unpack();
        if matches!(self.scheme, Scheme::StandardWebhooks) {
            let encoded = text.strip_prefix(SECRET_PREFIX).unwrap_or(text);
            let decoded = STANDARD.decode(encoded).ok()?;
            return (!decoded.is_empty()).then(|| Zeroizing::new(decoded));
        }
        Some(Zeroizing::new(text.as_bytes().to_vec()))
    }

    fn signed_parts<'a>(&self, extracted: &Extracted<'a>, body: &'a [u8]) -> Vec<&'a [u8]> {
        let timestamp = extracted.timestamp.unwrap_or_default().as_bytes();
        match self.layout() {
            Layout::Plain => vec![body],
            Layout::Dotted | Layout::Stripe => vec![timestamp, b".", body],
            Layout::Slack => vec![b"v0:", timestamp, b":", body],
            Layout::StandardWebhooks => vec![
                extracted.message_id.unwrap_or_default().as_bytes(),
                b".",
                timestamp,
                b".",
                body,
            ],
        }
    }
}

fn single_header<'a>(headers: &'a HeaderMap, name: &HeaderName) -> HeaderText<'a> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return HeaderText::Absent;
    };
    if values.next().is_some() {
        return HeaderText::Malformed;
    }
    match first.to_str() {
        Ok(text) => HeaderText::Present(text.trim()),
        Err(_) => HeaderText::Malformed,
    }
}

fn header_tokens<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
    split_on_spaces: bool,
) -> Result<Vec<&'a str>, Reason> {
    let mut tokens = Vec::new();
    for value in headers.get_all(name).iter().take(MAX_HEADER_ITEMS + 1) {
        let text = value.to_str().map_err(|_| Reason::SignatureMalformed)?;
        if split_on_spaces {
            tokens.extend(text.split_ascii_whitespace().take(MAX_HEADER_ITEMS + 1));
        } else {
            tokens.push(text.trim());
        }
        if tokens.len() > MAX_HEADER_ITEMS {
            return Err(Reason::SignatureMalformed);
        }
    }
    Ok(tokens)
}

fn stripe_tokens<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<(Vec<&'a str>, &'a str), Reason> {
    let text = match single_header(headers, name) {
        HeaderText::Present(text) => text,
        HeaderText::Absent | HeaderText::Malformed => return Err(Reason::SignatureMalformed),
    };
    let mut timestamp = None;
    let mut tokens = Vec::new();
    for (index, item) in text.split(',').enumerate() {
        if index >= MAX_HEADER_ITEMS {
            return Err(Reason::SignatureMalformed);
        }
        let Some((key, value)) = item.split_once('=') else {
            return Err(Reason::SignatureMalformed);
        };
        match key {
            "t" => {
                if timestamp.replace(value).is_some() {
                    return Err(Reason::TimestampMalformed);
                }
            }
            "v1" => tokens.push(value),
            _ => {}
        }
    }
    match timestamp {
        Some(stamp) => Ok((tokens, stamp)),
        None => Err(Reason::TimestampAbsent),
    }
}

fn within_tolerance(timestamp: &str, received_at: OffsetDateTime) -> Result<(), Reason> {
    let digits = timestamp.as_bytes();
    if digits.is_empty()
        || digits.len() > MAX_TIMESTAMP_DIGITS
        || !digits.iter().all(u8::is_ascii_digit)
    {
        return Err(Reason::TimestampMalformed);
    }
    let sent: i64 = timestamp.parse().map_err(|_| Reason::TimestampMalformed)?;
    if (received_at.unix_timestamp() - sent).abs() > TOLERANCE_SECONDS {
        return Err(Reason::TimestampOutOfTolerance);
    }
    Ok(())
}
