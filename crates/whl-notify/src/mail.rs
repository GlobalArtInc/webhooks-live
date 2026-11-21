use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::{AsyncSmtpTransportBuilder, Error as SmtpError};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use percent_encoding::percent_decode_str;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use tokio::sync::Semaphore;
use url::{Host, Url};
use whl_store::Uuid;

use crate::address::parse_address;

pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(12);
pub const DEFAULT_CONCURRENCY: usize = 16;
pub const MAX_SUBJECT_CHARS: usize = 150;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_SENDER_NAME_CHARS: usize = 64;
const BASE64_LINE_CHARS: usize = 76;
const SENDER_NAME_SYMBOLS: &str = " .-_'";

const SUBMISSIONS_PORT: u16 = 465;
const SUBMISSION_PORT: u16 = 587;
const PLAIN_PORT: u16 = 25;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MailConfigError {
    #[error("the SMTP URL must start with smtps:// or smtp://")]
    Scheme,
    #[error("the SMTP URL has no host")]
    Host,
    #[error("the SMTP URL query accepts only tls=required or tls=none")]
    Query,
    #[error("the SMTP URL asks for no TLS but names a host that is not on this machine")]
    PlaintextRemote,
    #[error("the SMTP credentials are not valid")]
    Credentials,
    #[error("the sender address is not valid")]
    Sender,
    #[error("the sender name must not contain control characters")]
    SenderControl,
    #[error("the SMTP transport cannot be set up: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MailError {
    #[error("the recipient address is not valid")]
    Recipient,
    #[error("the message cannot be built")]
    Build,
    #[error("too many messages are being sent at once")]
    Busy,
    #[error("the mail server did not answer in time")]
    Timeout,
    #[error("smtp_{0}: the mail server refused the message")]
    Permanent(String),
    #[error("smtp_{0}: the mail server could not take the message now")]
    Transient(String),
    #[error("the mail server connection failed")]
    Connection,
}

impl MailError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Busy | Self::Timeout | Self::Transient(_) | Self::Connection
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TlsMode {
    Wrapper,
    Required,
    Plain,
}

#[derive(Clone)]
pub struct MailSettings {
    url: Url,
    from: String,
    pub command_timeout: Duration,
    pub deadline: Duration,
    pub concurrency: usize,
}

impl MailSettings {
    pub fn new(url: Url, from: String) -> Self {
        Self {
            url,
            from,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            deadline: DEFAULT_DEADLINE,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

impl fmt::Debug for MailSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailSettings").finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OutgoingMail {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub unsubscribe: Option<String>,
}

impl fmt::Debug for OutgoingMail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutgoingMail")
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    sender: Sender,
    slots: Arc<Semaphore>,
    deadline: Duration,
}

impl fmt::Debug for Mailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mailer").finish_non_exhaustive()
    }
}

impl Mailer {
    pub fn new(settings: &MailSettings) -> Result<Self, MailConfigError> {
        let sender = parse_sender(&settings.from)?;
        let builder = transport_builder(&settings.url)?;
        let transport = builder.timeout(Some(settings.command_timeout)).build();
        Ok(Self {
            transport,
            sender,
            slots: Arc::new(Semaphore::new(settings.concurrency.max(1))),
            deadline: settings.deadline,
        })
    }

    pub async fn send(&self, mail: &OutgoingMail) -> Result<(), MailError> {
        let (envelope, raw) = self.build(mail)?;
        let _slot = self.slots.try_acquire().map_err(|_| MailError::Busy)?;
        let sent =
            tokio::time::timeout(self.deadline, self.transport.send_raw(&envelope, &raw)).await;
        match sent {
            Err(_) => Err(MailError::Timeout),
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(classify(&error)),
        }
    }

    fn build(&self, mail: &OutgoingMail) -> Result<(Envelope, Vec<u8>), MailError> {
        let recipient = parse_address(&mail.to).map_err(|_| MailError::Recipient)?;
        let recipient_address: Address = recipient.parse().map_err(|_| MailError::Recipient)?;
        let envelope = Envelope::new(Some(self.sender.address.clone()), vec![recipient_address])
            .map_err(|_| MailError::Build)?;
        let subject = ascii_line(&mail.subject, MAX_SUBJECT_CHARS);
        let date = OffsetDateTime::now_utc()
            .format(&Rfc2822)
            .map_err(|_| MailError::Build)?;
        let mut headers = vec![
            ("From", self.sender.header.clone()),
            ("To", recipient),
            ("Subject", subject),
            ("Date", date),
            (
                "Message-ID",
                format!("<{}@{}>", Uuid::now_v7(), self.sender.domain),
            ),
            ("MIME-Version", "1.0".to_owned()),
            ("Content-Type", "text/plain; charset=utf-8".to_owned()),
            ("Content-Transfer-Encoding", "base64".to_owned()),
            ("Auto-Submitted", "auto-generated".to_owned()),
        ];
        if let Some(unsubscribe) = &mail.unsubscribe {
            if unsubscribe
                .chars()
                .any(|character| !character.is_ascii_graphic() || matches!(character, '<' | '>'))
            {
                return Err(MailError::Build);
            }
            headers.push(("List-Unsubscribe", format!("<{unsubscribe}>")));
            headers.push((
                "List-Unsubscribe-Post",
                "List-Unsubscribe=One-Click".to_owned(),
            ));
        }
        let mut raw = String::new();
        for (name, value) in &headers {
            if value
                .chars()
                .any(|character| !(character == ' ' || character.is_ascii_graphic()))
            {
                return Err(MailError::Build);
            }
            raw.push_str(name);
            raw.push_str(": ");
            raw.push_str(value);
            raw.push_str("\r\n");
        }
        raw.push_str("\r\n");
        raw.push_str(&encode_body(&cap_bytes(&mail.body, MAX_BODY_BYTES)));
        Ok((envelope, raw.into_bytes()))
    }
}

fn encode_body(text: &str) -> String {
    let encoded = STANDARD.encode(text.as_bytes());
    let mut lines =
        String::with_capacity(encoded.len() + encoded.len() / BASE64_LINE_CHARS * 2 + 2);
    for chunk in encoded.as_bytes().chunks(BASE64_LINE_CHARS) {
        lines.push_str(&String::from_utf8_lossy(chunk));
        lines.push_str("\r\n");
    }
    lines
}

pub fn ascii_line(text: &str, max_chars: usize) -> String {
    let printable: String = text
        .chars()
        .map(|character| {
            if character == ' ' || character.is_ascii_graphic() {
                character
            } else if character.is_whitespace() || character.is_control() {
                ' '
            } else {
                '?'
            }
        })
        .collect();
    printable
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn cap_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[derive(Clone)]
struct Sender {
    header: String,
    address: Address,
    domain: String,
}

fn parse_sender(text: &str) -> Result<Sender, MailConfigError> {
    if text.chars().any(char::is_control) {
        return Err(MailConfigError::SenderControl);
    }
    let text = text.trim();
    let (name, mailbox) = match text.split_once('<') {
        Some((name, rest)) => (
            name.trim(),
            rest.strip_suffix('>').ok_or(MailConfigError::Sender)?,
        ),
        None => ("", text),
    };
    let allowed = |character: char| {
        character.is_ascii_alphanumeric() || SENDER_NAME_SYMBOLS.contains(character)
    };
    if name.chars().count() > MAX_SENDER_NAME_CHARS || !name.chars().all(allowed) {
        return Err(MailConfigError::Sender);
    }
    let mailbox = parse_address(mailbox).map_err(|_| MailConfigError::Sender)?;
    let address: Address = mailbox.parse().map_err(|_| MailConfigError::Sender)?;
    let header = if name.is_empty() {
        mailbox.clone()
    } else {
        format!("\"{name}\" <{mailbox}>")
    };
    let domain = address.domain().to_owned();
    Ok(Sender {
        header,
        address,
        domain,
    })
}

fn is_local_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain(name)) => {
            let bare = name.trim_start_matches('[').trim_end_matches(']');
            name.eq_ignore_ascii_case("localhost")
                || bare
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

fn credentials(url: &Url) -> Result<Option<Credentials>, MailConfigError> {
    if url.username().is_empty() {
        return Ok(None);
    }
    let decode = |text: &str| {
        percent_decode_str(text)
            .decode_utf8()
            .map(|decoded| decoded.into_owned())
            .map_err(|_| MailConfigError::Credentials)
    };
    let user = decode(url.username())?;
    let password = decode(url.password().ok_or(MailConfigError::Credentials)?)?;
    Ok(Some(Credentials::new(user, password)))
}

fn transport_builder(url: &Url) -> Result<AsyncSmtpTransportBuilder, MailConfigError> {
    let host = url
        .host_str()
        .map(|text| text.trim_start_matches('[').trim_end_matches(']'))
        .filter(|text| !text.is_empty())
        .ok_or(MailConfigError::Host)?;
    let tls = match (url.scheme(), url.query_pairs().next()) {
        ("smtps", None) => TlsMode::Wrapper,
        ("smtp", None) => TlsMode::Required,
        ("smtp", Some((key, value))) if key == "tls" && url.query_pairs().count() == 1 => {
            match value.as_ref() {
                "required" => TlsMode::Required,
                "none" => TlsMode::Plain,
                _ => return Err(MailConfigError::Query),
            }
        }
        ("smtp" | "smtps", Some(_)) => return Err(MailConfigError::Query),
        _ => return Err(MailConfigError::Scheme),
    };
    let transport = |error: SmtpError| MailConfigError::Transport(error.to_string());
    let builder = match tls {
        TlsMode::Wrapper => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
            .map_err(transport)?
            .port(url.port().unwrap_or(SUBMISSIONS_PORT)),
        TlsMode::Required => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
            .map_err(transport)?
            .port(url.port().unwrap_or(SUBMISSION_PORT)),
        TlsMode::Plain => {
            if !is_local_host(url.host()) {
                return Err(MailConfigError::PlaintextRemote);
            }
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(url.port().unwrap_or(PLAIN_PORT))
        }
    };
    Ok(match credentials(url)? {
        Some(credentials) => builder.credentials(credentials),
        None => builder,
    })
}

fn classify(error: &SmtpError) -> MailError {
    if error.is_timeout() {
        return MailError::Timeout;
    }
    match error.status() {
        Some(code) if error.is_permanent() => MailError::Permanent(code.to_string()),
        Some(code) => MailError::Transient(code.to_string()),
        None => MailError::Connection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(url: &str) -> Result<Mailer, MailConfigError> {
        Mailer::new(&MailSettings::new(
            Url::parse(url).expect("a URL"),
            "Hooks <noreply@hooks.example.test>".to_owned(),
        ))
    }

    #[test]
    fn plaintext_smtp_is_accepted_only_for_this_machine() {
        assert!(settings("smtp://127.0.0.1:2525?tls=none").is_ok());
        assert!(settings("smtp://localhost?tls=none").is_ok());
        assert!(settings("smtp://[::1]:2525?tls=none").is_ok());
        assert_eq!(
            settings("smtp://192.0.2.1?tls=none").err(),
            Some(MailConfigError::PlaintextRemote)
        );
        assert_eq!(
            settings("smtp://mail.example.test?tls=none").err(),
            Some(MailConfigError::PlaintextRemote)
        );
    }

    #[test]
    fn other_schemes_and_queries_are_refused() {
        assert_eq!(
            settings("http://mail.example.test").err(),
            Some(MailConfigError::Scheme)
        );
        assert_eq!(
            settings("smtp://mail.example.test?tls=opportunistic").err(),
            Some(MailConfigError::Query)
        );
        assert_eq!(
            settings("smtps://mail.example.test?tls=none").err(),
            Some(MailConfigError::Query)
        );
    }

    #[test]
    fn a_sender_with_control_characters_is_refused() {
        let outcome = Mailer::new(&MailSettings::new(
            Url::parse("smtps://mail.example.test").expect("a URL"),
            "a@b.test\r\nBcc: c@d.test".to_owned(),
        ));
        assert_eq!(outcome.err(), Some(MailConfigError::SenderControl));
    }

    #[test]
    fn a_subject_is_flattened_to_one_bounded_line() {
        let flat = ascii_line("a\r\nBcc: x@y.test\tb", MAX_SUBJECT_CHARS);
        assert_eq!(flat, "a Bcc: x@y.test b");
        assert_eq!(
            ascii_line(&"z".repeat(MAX_SUBJECT_CHARS * 2), MAX_SUBJECT_CHARS).len(),
            MAX_SUBJECT_CHARS
        );
    }
}
