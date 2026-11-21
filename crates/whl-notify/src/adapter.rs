use std::fmt;

use serde_json::{Value, json};
use whl_store::Uuid;
use whl_store::notify::ChannelKind;

use crate::mailtext::compose_mail;
use crate::notice::{Notice, truncate};
use crate::push::{PushHeaders, PushTarget};
use crate::target::{
    TELEGRAM_HOST, TargetRefusal, validate_bot_token, validate_chat_id, validate_discord_url,
    validate_slack_url,
};

pub const SLACK_TEXT_CHARS: usize = 8000;
pub const DISCORD_CONTENT_CHARS: usize = 2000;
pub const TELEGRAM_TEXT_UNITS: usize = 4096;

const ZERO_WIDTH_SPACE: char = '\u{200b}';

#[derive(Clone, PartialEq, Eq)]
pub struct MailPlan {
    pub to: String,
    pub subject: String,
    pub unsubscribe: String,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Destination {
    Guarded(String),
    Vendor(String),
    Mail(MailPlan),
    Push(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct Prepared {
    pub destination: Destination,
    pub body: Vec<u8>,
    pub delivery_header: Option<String>,
    pub push: Option<PushHeaders>,
}

impl fmt::Debug for Prepared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prepared")
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PrepareError {
    #[error("the channel has no stored secret")]
    MissingSecret,
    #[error("the channel configuration is incomplete")]
    Config,
    #[error("the stored target is refused: {0}")]
    Target(TargetRefusal),
    #[error("the message cannot be encoded")]
    Encode,
}

pub struct MailContext<'a> {
    pub recipient: &'a str,
    pub disable_token: &'a str,
    pub public_url: &'a str,
}

pub struct AdapterInput<'a> {
    pub config: &'a Value,
    pub url: Option<&'a str>,
    pub token: Option<&'a str>,
    pub mail: Option<MailContext<'a>>,
    pub notice: &'a Notice,
    pub delivery: Uuid,
    pub push: Option<&'a PushTarget<'a>>,
}

pub trait Adapter: Send + Sync {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError>;
}

pub fn adapter_for(kind: ChannelKind) -> Option<&'static dyn Adapter> {
    match kind {
        ChannelKind::Webhook => Some(&WebhookAdapter),
        ChannelKind::Slack => Some(&SlackAdapter),
        ChannelKind::Discord => Some(&DiscordAdapter),
        ChannelKind::Telegram => Some(&TelegramAdapter),
        ChannelKind::Email => Some(&EmailAdapter),
        ChannelKind::Push => Some(&PushAdapter),
    }
}

pub fn escape_slack(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub fn defuse_mentions(text: &str) -> String {
    let mut defused = String::with_capacity(text.len());
    for character in text.chars() {
        defused.push(character);
        if character == '@' {
            defused.push(ZERO_WIDTH_SPACE);
        }
    }
    defused
}

pub fn escape_discord(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '[' | ']' | '(' | ')' | '*' | '_' | '~' | '`') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

pub fn truncate_utf16(text: &str, max_units: usize) -> String {
    let mut units = 0;
    let mut kept = String::with_capacity(text.len().min(max_units));
    for character in text.chars() {
        units += character.len_utf16();
        if units > max_units {
            return kept;
        }
        kept.push(character);
    }
    kept
}

fn encode(value: &Value) -> Result<Vec<u8>, PrepareError> {
    serde_json::to_vec(value).map_err(|_| PrepareError::Encode)
}

struct WebhookAdapter;
struct SlackAdapter;
struct DiscordAdapter;
struct TelegramAdapter;
struct EmailAdapter;
struct PushAdapter;

impl Adapter for WebhookAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        let url = input.url.ok_or(PrepareError::MissingSecret)?;
        let body = encode(&json!({
            "endpoint_id": input.notice.endpoint_id,
            "capture": input.notice.capture,
            "message": input.notice.text,
        }))?;
        Ok(Prepared {
            destination: Destination::Guarded(url.to_owned()),
            body,
            delivery_header: Some(input.delivery.hyphenated().to_string()),
            push: None,
        })
    }
}

impl Adapter for SlackAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        let url = input.url.ok_or(PrepareError::MissingSecret)?;
        validate_slack_url(url).map_err(PrepareError::Target)?;
        let text = escape_slack(&truncate(&input.notice.text, SLACK_TEXT_CHARS));
        Ok(Prepared {
            destination: Destination::Vendor(url.to_owned()),
            body: encode(&json!({ "text": text }))?,
            delivery_header: None,
            push: None,
        })
    }
}

impl Adapter for DiscordAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        let url = input.url.ok_or(PrepareError::MissingSecret)?;
        validate_discord_url(url).map_err(PrepareError::Target)?;
        let content = truncate(
            &escape_discord(&defuse_mentions(&input.notice.text)),
            DISCORD_CONTENT_CHARS,
        );
        Ok(Prepared {
            destination: Destination::Vendor(url.to_owned()),
            body: encode(&json!({
                "content": content,
                "allowed_mentions": { "parse": [] },
            }))?,
            delivery_header: None,
            push: None,
        })
    }
}

impl Adapter for TelegramAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        let token = input.token.ok_or(PrepareError::MissingSecret)?;
        validate_bot_token(token).map_err(PrepareError::Target)?;
        let chat_id = input
            .config
            .get("chat_id")
            .and_then(Value::as_str)
            .ok_or(PrepareError::Config)?;
        validate_chat_id(chat_id).map_err(PrepareError::Target)?;
        let text = truncate_utf16(&defuse_mentions(&input.notice.text), TELEGRAM_TEXT_UNITS);
        Ok(Prepared {
            destination: Destination::Vendor(format!(
                "https://{TELEGRAM_HOST}/bot{token}/sendMessage"
            )),
            body: encode(&json!({
                "chat_id": chat_id,
                "text": text,
                "disable_web_page_preview": true,
            }))?,
            delivery_header: None,
            push: None,
        })
    }
}

impl Adapter for EmailAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        let mail = input.mail.as_ref().ok_or(PrepareError::Config)?;
        let message = compose_mail(input.notice, mail.public_url, mail.disable_token);
        Ok(Prepared {
            destination: Destination::Mail(MailPlan {
                to: mail.recipient.to_owned(),
                subject: message.subject,
                unsubscribe: message.unsubscribe,
            }),
            body: message.body.into_bytes(),
            delivery_header: None,
            push: None,
        })
    }
}

impl Adapter for PushAdapter {
    fn prepare(&self, input: &AdapterInput<'_>) -> Result<Prepared, PrepareError> {
        crate::push::prepare(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_markup_is_escaped_so_no_mention_or_link_is_interpreted() {
        assert_eq!(
            escape_slack("<!channel> <@U1> a&b <https://x|y>"),
            "&lt;!channel&gt; &lt;@U1&gt; a&amp;b &lt;https://x|y&gt;"
        );
    }

    #[test]
    fn discord_markdown_links_are_escaped() {
        assert_eq!(
            escape_discord("[x](https://evil) `a` *b*"),
            "\\[x\\]\\(https://evil\\) \\`a\\` \\*b\\*"
        );
    }

    #[test]
    fn at_signs_are_broken_so_no_mention_survives() {
        assert_eq!(defuse_mentions("@everyone"), "@\u{200b}everyone");
    }

    #[test]
    fn telegram_text_is_cut_by_utf16_units_without_splitting_a_character() {
        let text = "a\u{1f600}".repeat(3000);
        let cut = truncate_utf16(&text, TELEGRAM_TEXT_UNITS);
        assert!(cut.encode_utf16().count() <= TELEGRAM_TEXT_UNITS);
        assert!(cut.encode_utf16().count() >= TELEGRAM_TEXT_UNITS - 2);
    }

    #[test]
    fn email_and_push_both_have_an_adapter() {
        assert!(adapter_for(ChannelKind::Email).is_some());
        assert!(adapter_for(ChannelKind::Push).is_some());
    }
}
