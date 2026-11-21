use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use super::{NotifyStoreError, classify};
use crate::forward::TIMESTAMP_SQL;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what} {value:?} is not known")]
pub struct ParseError {
    what: &'static str,
    value: String,
}

macro_rules! text_enum {
    ($name:ident, $what:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(text: &str) -> Result<Self, ParseError> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    other => Err(ParseError { what: $what, value: other.to_owned() }),
                }
            }
        }
    };
}

text_enum!(ChannelKind, "channel kind", {
    Webhook => "webhook",
    Slack => "slack",
    Discord => "discord",
    Telegram => "telegram",
    Email => "email",
    Push => "push",
});

text_enum!(Trigger, "trigger", {
    Capture => "capture",
    Rule => "rule",
});

text_enum!(DeliveryKind, "delivery kind", {
    Single => "single",
    Summary => "summary",
    Test => "test",
});

text_enum!(DeliveryState, "delivery state", {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Suppressed => "suppressed",
});

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub id: Uuid,
    pub endpoint_id: String,
    pub kind: ChannelKind,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub condition: Option<serde_json::Value>,
    pub config: serde_json::Value,
    pub template: Option<String>,
    pub per_hour: i32,
    pub created_by: Option<Uuid>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub kind: DeliveryKind,
    pub state: DeliveryState,
    pub attempts: i16,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

pub(crate) fn delivery_columns() -> String {
    format!(
        "id, channel_id, kind, state, attempts, status, error, \
         to_char(created_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS created_at, \
         to_char(finished_at AT TIME ZONE 'UTC', {TIMESTAMP_SQL}) AS finished_at"
    )
}

pub(crate) fn delivery_from_row(row: &PgRow) -> Result<Delivery, NotifyStoreError> {
    let kind: String = row.try_get("kind").map_err(classify)?;
    let state: String = row.try_get("state").map_err(classify)?;
    Ok(Delivery {
        id: row.try_get("id").map_err(classify)?,
        channel_id: row.try_get("channel_id").map_err(classify)?,
        kind: DeliveryKind::parse(&kind)?,
        state: DeliveryState::parse(&state)?,
        attempts: row.try_get("attempts").map_err(classify)?,
        status: row.try_get("status").map_err(classify)?,
        error: row.try_get("error").map_err(classify)?,
        created_at: row.try_get("created_at").map_err(classify)?,
        finished_at: row.try_get("finished_at").map_err(classify)?,
    })
}

pub(crate) fn parse_json(text: &str, what: &str) -> Result<serde_json::Value, NotifyStoreError> {
    serde_json::from_str(text)
        .map_err(|error| NotifyStoreError::Invalid(format!("{what}: {error}")))
}
