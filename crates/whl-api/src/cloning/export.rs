use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};
use whl_auth::EndpointRole;
use whl_notify::channel_secret_name;
use whl_secrets::Keyring;
use whl_store::EndpointRecord;
use whl_store::notify::{Channel, ChannelKind};

use super::document::ConfigDocument;
use super::scan::referenced_secrets;
use crate::dto::EndpointDto;
use crate::error::ApiError;
use crate::state::ApiState;

const ENDPOINT_KEYS: &[&str] = &[
    "title",
    "response",
    "cors",
    "overflow",
    "tags",
    "retention",
    "capture_ttl_seconds",
    "verifier",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub endpoint: bool,
    pub forward_rules: bool,
    pub rules: bool,
    pub channels: bool,
    pub schedules: bool,
}

impl Selection {
    pub const ALL: Self = Self {
        endpoint: true,
        forward_rules: true,
        rules: true,
        channels: true,
        schedules: true,
    };

    pub const TEMPLATE: Self = Self {
        channels: false,
        ..Self::ALL
    };
}

#[derive(Clone, Copy)]
pub enum ChannelUrls<'a> {
    Omit,
    Include(&'a Keyring),
}

pub struct Exported {
    pub document: ConfigDocument,
    pub withheld: Vec<String>,
}

pub async fn export(
    state: &ApiState,
    record: &EndpointRecord,
    selection: Selection,
    role: EndpointRole,
    urls: ChannelUrls<'_>,
) -> Result<Exported, ApiError> {
    let mut document = ConfigDocument::default();
    let mut withheld = Vec::new();
    if selection.endpoint {
        document.endpoint = endpoint_value(state, record)?;
    }
    if selection.forward_rules && state.settings.forwarding {
        let rules = state.store.list_forward_rules(&record.id).await?;
        document.forward_rules = rules
            .iter()
            .map(|rule| {
                json!({
                    "id": rule.id.hyphenated().to_string(),
                    "enabled": rule.enabled,
                    "target_url": rule.target_url,
                    "method": rule.method,
                    "content_type": rule.content_type,
                    "header_allowlist": rule.header_allowlist,
                    "append_path": rule.append_path,
                })
            })
            .collect();
    }
    if selection.rules {
        let set = state.store.list_rules(&record.id).await?;
        document.rules = set
            .rules
            .iter()
            .map(|rule| {
                json!({
                    "name": rule.name,
                    "enabled": rule.enabled,
                    "condition": rule.condition,
                    "steps": rule.steps,
                })
            })
            .collect();
    }
    if selection.channels && state.notifications.enabled {
        if role >= EndpointRole::Editor {
            let channels = state.store.list_channels(&record.id).await?;
            for channel in &channels {
                match channel_value(state, record, channel, urls).await? {
                    Some(value) => document.channels.push(value),
                    None => withheld.push(format!("channels.{}", channel.name)),
                }
            }
        } else {
            withheld.push("channels".to_owned());
        }
    }
    if selection.schedules && state.schedules.enabled && state.access.has_users() {
        let schedules = state.store.list_schedules(&record.id).await?;
        for schedule in &schedules {
            let request = serde_json::to_value(&schedule.request)
                .map_err(|_| ApiError::InternalFault("a schedule request could not be encoded"))?;
            document.schedules.push(json!({
                "name": schedule.name,
                "cron": schedule.cron,
                "timezone": schedule.timezone,
                "request": request,
                "enabled": false,
            }));
        }
    }
    let endpoint = Value::Object(document.endpoint.clone());
    let mut scanned: Vec<&Value> = vec![&endpoint];
    scanned.extend(document.rules.iter());
    scanned.extend(document.channels.iter());
    scanned.extend(document.schedules.iter());
    document.secret_names = referenced_secrets(&scanned);
    Ok(Exported { document, withheld })
}

fn endpoint_value(
    state: &ApiState,
    record: &EndpointRecord,
) -> Result<Map<String, Value>, ApiError> {
    let dto = EndpointDto::from_record(record, &state.settings, &[]);
    let Value::Object(full) = serde_json::to_value(&dto)
        .map_err(|_| ApiError::InternalFault("an endpoint could not be encoded"))?
    else {
        return Err(ApiError::InternalFault("an endpoint was not an object"));
    };
    let mut kept = Map::new();
    for key in ENDPOINT_KEYS {
        if let Some(value) = full.get(*key) {
            kept.insert((*key).to_owned(), value.clone());
        }
    }
    if let Some(body) = kept
        .get_mut("response")
        .and_then(|response| response.get_mut("body"))
    {
        binary_safe(body);
    }
    kept.insert(
        "max_captures".to_owned(),
        json!(record.lifecycle.max_captures),
    );
    Ok(kept)
}

async fn channel_value(
    state: &ApiState,
    record: &EndpointRecord,
    channel: &Channel,
    urls: ChannelUrls<'_>,
) -> Result<Option<Value>, ApiError> {
    let mut value = json!({
        "id": channel.id.hyphenated().to_string(),
        "kind": channel.kind.as_str(),
        "name": channel.name,
        "enabled": channel.enabled,
        "trigger": channel.trigger.as_str(),
        "template": channel.template,
        "per_hour": channel.per_hour,
    });
    let config = match channel.kind {
        ChannelKind::Telegram => Some(channel.config.clone()),
        _ => match urls {
            ChannelUrls::Omit => None,
            ChannelUrls::Include(keyring) => {
                let Some(sealed) = state
                    .store
                    .get_channel_sealed(&record.id, channel.id)
                    .await?
                else {
                    return Ok(None);
                };
                let Ok(name) = channel_secret_name(channel.id) else {
                    return Ok(None);
                };
                let Ok(secret) = keyring.open(&record.id, &name, &sealed) else {
                    tracing::warn!(
                        endpoint = %record.id,
                        channel = %channel.id,
                        "a notification channel secret could not be opened for a clone"
                    );
                    return Ok(None);
                };
                Some(json!({ "url": secret.unpack() }))
            }
        },
    };
    if let (Some(config), Some(object)) = (config, value.as_object_mut()) {
        object.insert("config".to_owned(), config);
    }
    Ok(Some(value))
}

fn binary_safe(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let Some(text) = object
        .get("data")
        .and_then(Value::as_str)
        .filter(|data| data.contains('\0'))
    else {
        return;
    };
    let encoded = STANDARD.encode(text.as_bytes());
    object.insert("encoding".to_owned(), json!("base64"));
    object.insert("data".to_owned(), json!(encoded));
}
