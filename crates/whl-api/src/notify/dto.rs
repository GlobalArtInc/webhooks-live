use serde::Serialize;
use utoipa::ToSchema;
use whl_store::notify::{Channel, Delivery};

#[derive(Serialize, ToSchema)]
#[schema(
    as = Channel,
    description = "A notification channel of an endpoint. `config` per kind: `webhook`, `slack`, `discord`: `{url}`, write-only in full, responses carry only the masked form (`scheme://host/...`, `https://hooks.slack.com/services/...`, `https://discord.com/api/webhooks/...`); `telegram`: `{token_secret, chat_id}`, where `token_secret` is the name of an endpoint secret (its value is never returned) and `chat_id` is a numeric id, optionally negative, or `@username`; `email`: `{address_id}`, a verified address of the creator; `push`: `{}`, the creator's push subscriptions. `condition` is always null. `template` null means the default message; at most 4096 characters. `per_hour` is 1 to 3600."
)]
pub struct ChannelDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(value_type = ChannelKindDoc)]
    kind: &'static str,
    #[schema(min_length = 1, max_length = 64, example = "ops alerts")]
    name: String,
    enabled: bool,
    #[schema(value_type = TriggerDoc)]
    trigger: &'static str,
    #[schema(value_type = Option<Object>)]
    condition: Option<serde_json::Value>,
    #[schema(value_type = Object)]
    config: serde_json::Value,
    template: Option<String>,
    #[schema(minimum = 1, maximum = 3600, example = 30)]
    per_hour: i32,
    #[schema(format = DateTime)]
    created_at: String,
}

impl From<&Channel> for ChannelDto {
    fn from(channel: &Channel) -> Self {
        Self {
            id: channel.id.hyphenated().to_string(),
            kind: channel.kind.as_str(),
            name: channel.name.clone(),
            enabled: channel.enabled,
            trigger: channel.trigger.as_str(),
            condition: channel.condition.clone(),
            config: channel.config.clone(),
            template: channel.template.clone(),
            per_hour: channel.per_hour,
            created_at: channel.created_at.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ChannelList)]
pub struct ChannelListDto {
    pub items: Vec<ChannelDto>,
}

#[derive(Serialize, ToSchema)]
#[schema(
    as = Delivery,
    description = "One send through a channel: a single notification, a summary, or a test."
)]
pub struct DeliveryDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d71"
    )]
    id: String,
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    channel_id: String,
    #[schema(value_type = DeliveryKindDoc)]
    kind: &'static str,
    #[schema(value_type = DeliveryStateDoc)]
    state: &'static str,
    attempts: i16,
    status: Option<i16>,
    error: Option<String>,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
    finished_at: Option<String>,
}

impl From<&Delivery> for DeliveryDto {
    fn from(delivery: &Delivery) -> Self {
        Self {
            id: delivery.id.hyphenated().to_string(),
            channel_id: delivery.channel_id.hyphenated().to_string(),
            kind: delivery.kind.as_str(),
            state: delivery.state.as_str(),
            attempts: delivery.attempts,
            status: delivery.status,
            error: delivery.error.clone(),
            created_at: delivery.created_at.clone(),
            finished_at: delivery.finished_at.clone(),
        }
    }
}

#[derive(ToSchema)]
#[schema(
    as = ChannelInput,
    description = "Body of channel create and of channel PATCH. Create requires `kind`, `name` and `config`. PATCH takes any subset; absent fields are unchanged, `null` resets `template` or `condition`, and `kind` is refused with 422 `not_allowed`. `config` per kind: `webhook`, `slack`, `discord`: `{url}`; `telegram`: `{token_secret, chat_id}`; `email`: `{address_id}`; `push`: `{}`. Other keys answer 422 `not_allowed`. `condition` must be null or absent. `template` is at most 4096 characters. `per_hour` is 1 to 3600, default 30 on create."
)]
#[allow(dead_code)]
pub struct ChannelInputDoc {
    kind: Option<ChannelKindDoc>,
    #[schema(min_length = 1, max_length = 64, example = "ops alerts")]
    name: Option<String>,
    enabled: Option<bool>,
    trigger: Option<TriggerDoc>,
    #[schema(value_type = Option<Object>)]
    condition: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    config: Option<serde_json::Value>,
    template: Option<String>,
    #[schema(minimum = 1, maximum = 3600, example = 30)]
    per_hour: Option<i32>,
}

#[derive(ToSchema)]
#[schema(as = ChannelKind)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum ChannelKindDoc {
    Webhook,
    Slack,
    Discord,
    Telegram,
    Email,
    Push,
}

#[derive(ToSchema)]
#[schema(as = Trigger)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum TriggerDoc {
    Capture,
    Rule,
}

#[derive(ToSchema)]
#[schema(as = DeliveryKind)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum DeliveryKindDoc {
    Single,
    Summary,
    Test,
}

#[derive(ToSchema)]
#[schema(as = DeliveryState)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum DeliveryStateDoc {
    Pending,
    Running,
    Succeeded,
    Failed,
    Suppressed,
}
