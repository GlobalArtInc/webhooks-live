use serde_json::{Map, Value};
use whl_notify::{
    MAX_CHANNEL_URL_BYTES, TargetRefusal, is_masked, validate_chat_id, validate_discord_url,
    validate_slack_url,
};
use whl_secrets::SecretName;
use whl_store::Uuid;
use whl_store::notify::{ChannelKind, Trigger};
use whl_template::{Policy, RenderTarget};

use crate::error::{ApiError, FieldCode, FieldViolation};

pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_TEMPLATE_CHARS: usize = 4096;
pub const MAX_PER_HOUR: i64 = 3600;
pub const DEFAULT_PER_HOUR: i32 = 30;
pub const MAX_WEBHOOK_URL_BYTES: usize = 2048;

const CREATE_KEYS: &[&str] = &[
    "kind",
    "name",
    "enabled",
    "trigger",
    "condition",
    "config",
    "template",
    "per_hour",
];
const AVAILABLE_KINDS: [ChannelKind; 6] = [
    ChannelKind::Webhook,
    ChannelKind::Slack,
    ChannelKind::Discord,
    ChannelKind::Telegram,
    ChannelKind::Email,
    ChannelKind::Push,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelConfig {
    Url(String),
    Telegram {
        token_secret: SecretName,
        chat_id: String,
    },
    Email {
        address_id: Uuid,
    },
    Push,
}

#[derive(Debug, Default)]
pub struct ParsedChannel {
    pub kind: Option<ChannelKind>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub trigger: Option<Trigger>,
    pub template: Option<Option<String>>,
    pub per_hour: Option<i32>,
    pub config: Option<ChannelConfig>,
    pub violations: Vec<FieldViolation>,
}

pub fn parse_channel(
    bytes: &[u8],
    creating: bool,
    existing: Option<ChannelKind>,
) -> Result<ParsedChannel, ApiError> {
    let object = object_of(bytes)?;
    let mut parsed = ParsedChannel::default();
    let mut walk = Walk::default();
    for key in object.keys() {
        let known = CREATE_KEYS.contains(&key.as_str()) && (creating || key != "kind");
        if !known {
            walk.push(key.as_str(), FieldCode::NotAllowed, "is not a known field");
        }
    }
    if creating {
        parsed.kind = walk.kind(object.get("kind"));
    } else {
        parsed.kind = existing;
    }
    if let Some(value) = object.get("name") {
        parsed.name = walk.name(value);
    } else if creating {
        walk.push("name", FieldCode::Required, "is required");
    }
    if let Some(value) = object.get("enabled") {
        parsed.enabled = walk.boolean(value, "enabled");
    }
    if let Some(value) = object.get("trigger") {
        parsed.trigger = walk.trigger(value);
    }
    if let Some(value) = object.get("condition")
        && !value.is_null()
    {
        walk.push(
            "condition",
            FieldCode::NotAllowed,
            "conditions are not available yet",
        );
    }
    if let Some(value) = object.get("template") {
        parsed.template = walk.template(value);
    }
    if let Some(value) = object.get("per_hour") {
        parsed.per_hour = walk.per_hour(value);
    }
    match (object.get("config"), parsed.kind) {
        (Some(value), Some(kind)) => parsed.config = walk.config(kind, value),
        (None, _) if creating => walk.push("config", FieldCode::Required, "is required"),
        _ => {}
    }
    parsed.violations = walk.violations;
    Ok(parsed)
}

pub fn url_violation(field: &str, refusal: TargetRefusal) -> FieldViolation {
    let code = match refusal {
        TargetRefusal::TooLong => FieldCode::TooLong,
        TargetRefusal::Pattern | TargetRefusal::Format => FieldCode::InvalidFormat,
    };
    FieldViolation::new(field, code, refusal.to_string())
}

pub(crate) fn object_of(bytes: &[u8]) -> Result<Map<String, Value>, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    match serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?
    {
        Value::Object(object) => Ok(object),
        _ => Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        )),
    }
}

#[derive(Default)]
struct Walk {
    violations: Vec<FieldViolation>,
}

impl Walk {
    fn push(&mut self, field: impl Into<String>, code: FieldCode, message: impl Into<String>) {
        self.violations
            .push(FieldViolation::new(field, code, message));
    }

    fn kind(&mut self, value: Option<&Value>) -> Option<ChannelKind> {
        let Some(value) = value else {
            self.push("kind", FieldCode::Required, "is required");
            return None;
        };
        let Some(text) = value.as_str() else {
            self.push("kind", FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        match ChannelKind::parse(text) {
            Ok(kind) if AVAILABLE_KINDS.contains(&kind) => Some(kind),
            Ok(_) => {
                self.push(
                    "kind",
                    FieldCode::NotAllowed,
                    "this kind is not available yet",
                );
                None
            }
            Err(_) => {
                self.push(
                    "kind",
                    FieldCode::InvalidFormat,
                    "is not a known channel kind",
                );
                None
            }
        }
    }

    fn boolean(&mut self, value: &Value, field: &str) -> Option<bool> {
        let flag = value.as_bool();
        if flag.is_none() {
            self.push(field, FieldCode::InvalidFormat, "must be a boolean");
        }
        flag
    }

    fn name(&mut self, value: &Value) -> Option<String> {
        let Some(text) = value.as_str() else {
            self.push("name", FieldCode::InvalidFormat, "must be a string");
            return None;
        };
        let chars = text.chars().count();
        if text.trim().is_empty() {
            self.push("name", FieldCode::Required, "must not be empty");
            None
        } else if chars > MAX_NAME_CHARS {
            self.push(
                "name",
                FieldCode::TooLong,
                format!("must be at most {MAX_NAME_CHARS} characters"),
            );
            None
        } else if text.chars().any(char::is_control) {
            self.push(
                "name",
                FieldCode::InvalidFormat,
                "must not contain control characters",
            );
            None
        } else {
            Some(text.to_owned())
        }
    }

    fn trigger(&mut self, value: &Value) -> Option<Trigger> {
        let parsed = value.as_str().and_then(|text| Trigger::parse(text).ok());
        if parsed.is_none() {
            self.push(
                "trigger",
                FieldCode::InvalidFormat,
                "must be \"capture\" or \"rule\"",
            );
        }
        parsed
    }

    fn template(&mut self, value: &Value) -> Option<Option<String>> {
        let text = match value {
            Value::Null => return Some(None),
            Value::String(text) => text,
            _ => {
                self.push(
                    "template",
                    FieldCode::InvalidFormat,
                    "must be a string or null",
                );
                return None;
            }
        };
        if text.chars().count() > MAX_TEMPLATE_CHARS {
            self.push(
                "template",
                FieldCode::TooLong,
                format!("must be at most {MAX_TEMPLATE_CHARS} characters"),
            );
            return None;
        }
        match whl_template::validate(text, &Policy::response(RenderTarget::Body)) {
            Ok(()) => Some(Some(text.clone())),
            Err(error) => {
                self.push("template", FieldCode::InvalidFormat, error.to_string());
                None
            }
        }
    }

    fn per_hour(&mut self, value: &Value) -> Option<i32> {
        let number = value
            .as_i64()
            .filter(|number| (1..=MAX_PER_HOUR).contains(number));
        match number.and_then(|number| i32::try_from(number).ok()) {
            Some(number) => Some(number),
            None => {
                self.push(
                    "per_hour",
                    FieldCode::OutOfRange,
                    format!("must be between 1 and {MAX_PER_HOUR}"),
                );
                None
            }
        }
    }

    fn config(&mut self, kind: ChannelKind, value: &Value) -> Option<ChannelConfig> {
        let Some(object) = value.as_object() else {
            self.push("config", FieldCode::InvalidFormat, "must be an object");
            return None;
        };
        let known: &[&str] = match kind {
            ChannelKind::Telegram => &["token_secret", "chat_id"],
            ChannelKind::Email => &["address_id"],
            ChannelKind::Push => &[],
            _ => &["url"],
        };
        for key in object.keys().filter(|key| !known.contains(&key.as_str())) {
            self.push(
                format!("config.{key}"),
                FieldCode::NotAllowed,
                "is not a known field",
            );
        }
        match kind {
            ChannelKind::Telegram => self.telegram(object),
            ChannelKind::Email => self.email(object),
            ChannelKind::Push => Some(ChannelConfig::Push),
            _ => self.url(kind, object),
        }
    }

    fn url(&mut self, kind: ChannelKind, object: &Map<String, Value>) -> Option<ChannelConfig> {
        let text = self.required_text(object, "url")?;
        if is_masked(&text) {
            self.push(
                "config.url",
                FieldCode::InvalidFormat,
                "must be the full URL, not the masked form",
            );
            return None;
        }
        let checked = match kind {
            ChannelKind::Slack => validate_slack_url(&text),
            ChannelKind::Discord => validate_discord_url(&text),
            _ if text.len() > MAX_WEBHOOK_URL_BYTES => Err(TargetRefusal::TooLong),
            _ => Ok(()),
        };
        match checked {
            Ok(()) => Some(ChannelConfig::Url(text)),
            Err(refusal) => {
                self.violations.push(url_violation("config.url", refusal));
                None
            }
        }
    }

    fn email(&mut self, object: &Map<String, Value>) -> Option<ChannelConfig> {
        let text = self.required_text(object, "address_id")?;
        match Uuid::try_parse(&text)
            .ok()
            .filter(|uuid| uuid.hyphenated().to_string() == text)
        {
            Some(address_id) => Some(ChannelConfig::Email { address_id }),
            None => {
                self.push(
                    "config.address_id",
                    FieldCode::InvalidFormat,
                    "must be the id of one of your email addresses",
                );
                None
            }
        }
    }

    fn telegram(&mut self, object: &Map<String, Value>) -> Option<ChannelConfig> {
        let token = self.required_text(object, "token_secret");
        let chat = self.required_text(object, "chat_id");
        let (token, chat_id) = (token?, chat?);
        let name = match SecretName::parse(&token) {
            Ok(name) => Some(name),
            Err(error) => {
                self.push(
                    "config.token_secret",
                    FieldCode::InvalidFormat,
                    error.to_string(),
                );
                None
            }
        };
        if let Err(refusal) = validate_chat_id(&chat_id) {
            self.violations
                .push(url_violation("config.chat_id", refusal));
            return None;
        }
        Some(ChannelConfig::Telegram {
            token_secret: name?,
            chat_id,
        })
    }

    fn required_text(&mut self, object: &Map<String, Value>, key: &str) -> Option<String> {
        let field = format!("config.{key}");
        match object.get(key) {
            None | Some(Value::Null) => {
                self.push(field, FieldCode::Required, "is required");
                None
            }
            Some(Value::String(text)) if text.is_empty() => {
                self.push(field, FieldCode::Required, "must not be empty");
                None
            }
            Some(Value::String(text))
                if text.len() > MAX_CHANNEL_URL_BYTES.max(MAX_WEBHOOK_URL_BYTES) =>
            {
                self.push(field, FieldCode::TooLong, "is too long");
                None
            }
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => {
                self.push(field, FieldCode::InvalidFormat, "must be a string");
                None
            }
        }
    }
}
