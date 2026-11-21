use serde::Serialize;
use whl_store::notify::{Channel, Delivery};

#[derive(Serialize)]
pub struct ChannelDto {
    id: String,
    kind: &'static str,
    name: String,
    enabled: bool,
    trigger: &'static str,
    condition: Option<serde_json::Value>,
    config: serde_json::Value,
    template: Option<String>,
    per_hour: i32,
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

#[derive(Serialize)]
pub struct ChannelListDto {
    pub items: Vec<ChannelDto>,
}

#[derive(Serialize)]
pub struct DeliveryDto {
    id: String,
    channel_id: String,
    kind: &'static str,
    state: &'static str,
    attempts: i16,
    status: Option<i16>,
    error: Option<String>,
    created_at: String,
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
