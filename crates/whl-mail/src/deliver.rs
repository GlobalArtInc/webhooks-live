use std::net::IpAddr;

use bytes::Bytes;
use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::CaptureKind;
use whl_store::captures::{CaptureOutcome, CaptureShape, NewCapture};

use crate::message::{ParseFailure, ParsedMessage, parse_message_bounded};
use crate::recipient::Recipient;
use crate::shared::{Shared, message_result};
use crate::wire::Collected;

pub const SMTP_METHOD: &str = "SMTP";
const ACCEPTED_STATUS: u16 = 250;
const MAX_ENVELOPE_CHARS: usize = 320;
const MAX_HELO_CHARS: usize = 255;

pub struct Envelope {
    pub sender: String,
    pub recipients: Vec<Recipient>,
    pub helo: Option<String>,
    pub tls: bool,
    pub client: IpAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    Stored(CaptureId),
    Full,
    Gone,
    OverQuota,
    Unavailable,
}

pub fn clean_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

fn group_by_endpoint(recipients: &[Recipient]) -> Vec<(EndpointId, Vec<&Recipient>)> {
    let mut groups: Vec<(EndpointId, Vec<&Recipient>)> = Vec::new();
    for recipient in recipients {
        match groups
            .iter_mut()
            .find(|(endpoint, _)| *endpoint == recipient.endpoint)
        {
            Some((_, members)) => members.push(recipient),
            None => groups.push((recipient.endpoint.clone(), vec![recipient])),
        }
    }
    groups
}

fn email_meta(
    envelope: &Envelope,
    members: &[&Recipient],
    parsed: &Result<ParsedMessage, ParseFailure>,
) -> Value {
    let message = parsed.as_ref().ok();
    let from = message
        .filter(|message| !message.from.is_empty())
        .map(|message| message.from.join(", "));
    json!({
        "envelope_from": clean_text(&envelope.sender, MAX_ENVELOPE_CHARS),
        "envelope_to": members.iter().map(|member| member.address.as_str()).collect::<Vec<_>>(),
        "helo": clean_text(envelope.helo.as_deref().unwrap_or_default(), MAX_HELO_CHARS),
        "tls": envelope.tls,
        "tag": members.first().and_then(|member| member.tag.as_deref()),
        "subject": message.and_then(|message| message.subject.as_deref()),
        "from": from,
        "to": message.map(|message| message.to.as_slice()).unwrap_or_default(),
        "cc": message.map(|message| message.cc.as_slice()).unwrap_or_default(),
        "date": message.and_then(|message| message.date.as_deref()),
        "message_id": message.and_then(|message| message.message_id.as_deref()),
        "parts": message.map(|message| message.parts.as_slice()).unwrap_or_default(),
        "parts_truncated": message.is_some_and(|message| message.parts_truncated),
        "parse_error": parsed.as_ref().err().map(|failure| failure.label()),
    })
}

async fn store_one(
    shared: &Shared,
    envelope: &Envelope,
    endpoint: EndpointId,
    members: &[&Recipient],
    raw: &Bytes,
    parsed: &Result<ParsedMessage, ParseFailure>,
) -> Delivered {
    let Some(first) = members.first() else {
        return Delivered::Unavailable;
    };
    let Ok(id) = CaptureId::generate() else {
        tracing::error!("the capture id source failed for a mail message");
        return Delivered::Unavailable;
    };
    let (headers, content_type) = match parsed {
        Ok(message) => (message.headers.clone(), message.content_type.clone()),
        Err(_) => (Vec::new(), None),
    };
    let capture = NewCapture {
        id,
        endpoint_id: endpoint,
        received_at: OffsetDateTime::now_utc(),
        method: SMTP_METHOD.to_owned(),
        url: format!("mailto:{}", first.address),
        path: first
            .tag
            .as_ref()
            .map(|tag| format!("+{tag}"))
            .unwrap_or_default(),
        query_string: String::new(),
        host: shared.domain.unpack().to_owned(),
        client_ip: envelope.client.to_canonical(),
        headers,
        query: Vec::new(),
        form: None,
        content_type,
        body: raw.clone(),
        status_override: None,
    };
    let shape = CaptureShape {
        kind: CaptureKind::Email,
        meta: email_meta(envelope, members, parsed),
        response_status: Some(ACCEPTED_STATUS),
        verification: None,
    };
    match shared
        .store
        .store_capture_as(capture, shared.policy, shape)
        .await
    {
        Ok(CaptureOutcome::Stored { capture, .. }) => Delivered::Stored(capture.id),
        Ok(CaptureOutcome::Full | CaptureOutcome::OverQuota) => Delivered::Full,
        Ok(CaptureOutcome::Gone) => Delivered::Gone,
        Err(error) => {
            if error.is_unavailable() {
                tracing::warn!(%error, "the store is unavailable for a mail message");
            } else {
                tracing::error!(%error, "the store failed a mail message");
            }
            Delivered::Unavailable
        }
    }
}

pub async fn deliver(shared: &Shared, envelope: &Envelope, collected: Collected) -> Delivered {
    let Collected { data, reservation } = collected;
    let raw = Bytes::from(data);
    let parsed = parse_message_bounded(raw.clone()).await;
    if let Err(failure) = &parsed {
        tracing::warn!(
            reason = failure.label(),
            "a mail message was stored without its structure"
        );
    }

    let mut stored = None;
    let mut failure = None;
    for (endpoint, members) in group_by_endpoint(&envelope.recipients) {
        let outcome = if shared.quota.has_room(&endpoint) {
            let outcome =
                store_one(shared, envelope, endpoint.clone(), &members, &raw, &parsed).await;
            if matches!(outcome, Delivered::Stored(_)) {
                shared.quota.take(&endpoint);
            }
            outcome
        } else {
            Delivered::OverQuota
        };
        match outcome {
            Delivered::Stored(id) => {
                stored.get_or_insert(id);
            }
            other => {
                failure.get_or_insert(other);
            }
        }
    }
    drop(reservation);

    let outcome = match (stored, failure) {
        (Some(id), _) => Delivered::Stored(id),
        (None, Some(failure)) => failure,
        (None, None) => Delivered::Unavailable,
    };
    message_result(match outcome {
        Delivered::Stored(_) => "stored",
        Delivered::Full | Delivered::Gone => "refused_recipient",
        Delivered::OverQuota => "quota",
        Delivered::Unavailable => "store_unavailable",
    });
    outcome
}
