use std::sync::{Arc, LazyLock};
use std::time::Duration;

use bytes::Bytes;
use mail_parser::{AddressList, MessageParser, MessagePart, PartFlags};
use serde::Serialize;
use tokio::sync::Semaphore;
use whl_store::captures::NameValue;

pub const MAX_PARTS: usize = 50;
pub const MAX_DEPTH: usize = 8;
pub const MAX_LINES: usize = 500_000;
pub const MAX_HEADER_BLOCK_BYTES: usize = 1 << 20;
pub const MAX_HEADERS_STORED: usize = 200;
pub const MAX_HEADER_VALUE_BYTES: usize = 4096;
pub const MAX_ADDRESSES: usize = 50;
pub const MAX_ADDRESS_CHARS: usize = 320;
pub const MAX_SUBJECT_CHARS: usize = 1000;
pub const MAX_NAME_CHARS: usize = 255;
pub const INGEST_PARSE_SLOTS: usize = 4;
pub const API_PARSE_SLOTS: usize = 2;
pub const PARSE_TIMEOUT: Duration = Duration::from_secs(2);
pub const PARSE_SLOT_WAIT: Duration = Duration::from_secs(1);

static INGEST_PERMITS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(INGEST_PARSE_SLOTS)));

static API_PERMITS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(API_PARSE_SLOTS)));

static PARSER: LazyLock<MessageParser> = LazyLock::new(|| {
    MessageParser::new()
        .max_parts(MAX_PARTS)
        .max_depth(MAX_DEPTH)
        .max_encoded_nesting(1)
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseFailure {
    #[error("the message has more than {MAX_LINES} lines")]
    TooManyLines,
    #[error("the message header block is larger than {MAX_HEADER_BLOCK_BYTES} bytes")]
    HeaderBlockTooLarge,
    #[error("the message has no header block")]
    Unparseable,
    #[error("no parser slot became free in time")]
    Busy,
    #[error("the parser did not finish within {PARSE_TIMEOUT:?}")]
    Timeout,
    #[error("the parser task failed")]
    Aborted,
}

impl ParseFailure {
    pub fn label(self) -> &'static str {
        match self {
            Self::TooManyLines => "too_many_lines",
            Self::HeaderBlockTooLarge => "header_block_too_large",
            Self::Unparseable => "unparseable",
            Self::Busy => "busy",
            Self::Timeout => "timeout",
            Self::Aborted => "aborted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PartFailure {
    #[error("the part does not exist")]
    NotFound,
    #[error("the message could not be parsed: {0}")]
    Parse(#[from] ParseFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Inline,
    Attachment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartSummary {
    pub index: usize,
    pub content_type: String,
    pub filename: Option<String>,
    pub size: usize,
    pub disposition: Option<Disposition>,
    pub is_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    pub headers: Vec<NameValue>,
    pub content_type: Option<String>,
    pub subject: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub parts: Vec<PartSummary>,
    pub parts_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPart {
    pub content_type: String,
    pub filename: Option<String>,
    pub is_text: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartView {
    Decoded,
    Text,
}

fn check_shape(raw: &[u8]) -> Result<(), ParseFailure> {
    if raw.iter().filter(|byte| **byte == b'\n').count() > MAX_LINES {
        return Err(ParseFailure::TooManyLines);
    }
    let block_end = raw
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .into_iter()
        .chain(raw.windows(4).position(|quad| quad == b"\r\n\r\n"))
        .min()
        .unwrap_or(raw.len());
    if block_end > MAX_HEADER_BLOCK_BYTES {
        return Err(ParseFailure::HeaderBlockTooLarge);
    }
    Ok(())
}

fn listed<'m>(
    message: &'m mail_parser::Message<'_>,
) -> impl Iterator<Item = MessagePart<'m>> + use<'m> {
    let root = message.root().id();
    message
        .parts()
        .filter(move |part| !part.is_multipart() && part.message().id() == root)
}

fn clean(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(max_chars)
        .collect()
}

fn content_type_of(part: &MessagePart<'_>) -> String {
    match part.content_type() {
        Some(content_type) => {
            let main = clean(&content_type.ctype().to_ascii_lowercase(), 64);
            match content_type.subtype() {
                Some(sub) => format!("{main}/{}", clean(&sub.to_ascii_lowercase(), 64)),
                None => main,
            }
        }
        None if part.is_text() => "text/plain".to_owned(),
        None => "application/octet-stream".to_owned(),
    }
}

fn disposition_of(part: &MessagePart<'_>) -> Option<Disposition> {
    let disposition = part.content_disposition()?;
    if disposition.is_attachment() {
        Some(Disposition::Attachment)
    } else if disposition.is_inline() {
        Some(Disposition::Inline)
    } else {
        None
    }
}

fn summarize(index: usize, part: &MessagePart<'_>) -> PartSummary {
    PartSummary {
        index,
        content_type: content_type_of(part),
        filename: part
            .attachment_name()
            .map(|name| clean(name, MAX_NAME_CHARS))
            .filter(|name| !name.is_empty()),
        size: part.decoded_len(),
        disposition: disposition_of(part),
        is_text: part.is_text(),
    }
}

fn addresses(list: Option<AddressList<'_>>) -> Vec<String> {
    let Some(list) = list else {
        return Vec::new();
    };
    list.mailboxes()
        .take(MAX_ADDRESSES)
        .filter_map(|mailbox| {
            let formatted = match (mailbox.name(), mailbox.address()) {
                (Some(name), Some(address)) => format!("{name} <{address}>"),
                (None, Some(address)) => address.to_owned(),
                (Some(name), None) => name.to_owned(),
                (None, None) => return None,
            };
            Some(clean(&formatted, MAX_ADDRESS_CHARS))
        })
        .collect()
}

fn unfold(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len().min(MAX_HEADER_VALUE_BYTES + 4));
    for segment in text.split(['\r', '\n']) {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(trimmed);
        if out.len() > MAX_HEADER_VALUE_BYTES {
            break;
        }
    }
    let mut end = out.len().min(MAX_HEADER_VALUE_BYTES);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out.retain(|character| !character.is_control() || character == '\t');
    out
}

pub fn parse_message(raw: &[u8]) -> Result<ParsedMessage, ParseFailure> {
    check_shape(raw)?;
    let message = PARSER.parse(raw).ok_or(ParseFailure::Unparseable)?;

    let headers = message
        .headers()
        .iter()
        .take(MAX_HEADERS_STORED)
        .map(|header| NameValue {
            name: clean(header.raw_name(), MAX_NAME_CHARS),
            value: unfold(header.raw_value()),
        })
        .collect();
    let mut parts = Vec::new();
    for part in listed(&message).take(MAX_PARTS) {
        parts.push(summarize(parts.len(), &part));
    }
    let parts_truncated = message
        .parts()
        .any(|part| part.flags().contains(PartFlags::LIMIT_REACHED));
    let date = message
        .date()
        .filter(|date| date.is_valid())
        .map(|date| date.to_rfc3339());

    Ok(ParsedMessage {
        headers,
        content_type: message
            .content_type()
            .map(|_| content_type_of(&message.root_part())),
        subject: message
            .subject()
            .map(|subject| clean(subject, MAX_SUBJECT_CHARS)),
        from: addresses(message.from()),
        to: addresses(message.to()),
        cc: addresses(message.cc()),
        date,
        message_id: message.message_id().map(|id| clean(id, MAX_ADDRESS_CHARS)),
        parts,
        parts_truncated,
    })
}

pub fn decode_part(raw: &[u8], index: usize, view: PartView) -> Result<DecodedPart, PartFailure> {
    check_shape(raw)?;
    let message = PARSER.parse(raw).ok_or(ParseFailure::Unparseable)?;
    let part = listed(&message)
        .take(MAX_PARTS)
        .nth(index)
        .ok_or(PartFailure::NotFound)?;
    let summary = summarize(index, &part);
    let data = match (view, part.is_text()) {
        (PartView::Text, true) => part
            .text()
            .map(|text| text.into_owned().into_bytes())
            .unwrap_or_default(),
        _ => part.decoded().into_owned(),
    };
    Ok(DecodedPart {
        content_type: summary.content_type,
        filename: summary.filename,
        is_text: summary.is_text,
        data,
    })
}

async fn run_blocking<T, F>(slots: &Arc<Semaphore>, work: F) -> Result<T, ParseFailure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ParseFailure> + Send + 'static,
{
    let permit = tokio::time::timeout(PARSE_SLOT_WAIT, Arc::clone(slots).acquire_owned())
        .await
        .map_err(|_| ParseFailure::Busy)?
        .map_err(|_| ParseFailure::Aborted)?;
    let task = tokio::task::spawn_blocking(move || {
        let outcome = work();
        drop(permit);
        outcome
    });
    match tokio::time::timeout(PARSE_TIMEOUT, task).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => Err(ParseFailure::Aborted),
        Err(_) => Err(ParseFailure::Timeout),
    }
}

pub async fn parse_message_bounded(raw: Bytes) -> Result<ParsedMessage, ParseFailure> {
    run_blocking(&INGEST_PERMITS, move || parse_message(&raw)).await
}

pub async fn decode_part_bounded(
    raw: Vec<u8>,
    index: usize,
    view: PartView,
) -> Result<DecodedPart, PartFailure> {
    run_blocking(&API_PERMITS, move || Ok(decode_part(&raw, index, view))).await?
}
