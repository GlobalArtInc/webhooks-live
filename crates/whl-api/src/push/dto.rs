use serde::Serialize;
use whl_store::notify::{PushSubscriptionInfo, SavedPushSubscription};

#[derive(Serialize)]
pub struct PublicKeyDto {
    pub public_key: String,
}

#[derive(Serialize)]
pub struct CreatedDto {
    id: String,
    created_at: String,
}

impl From<&SavedPushSubscription> for CreatedDto {
    fn from(saved: &SavedPushSubscription) -> Self {
        Self {
            id: saved.id.hyphenated().to_string(),
            created_at: saved.created_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct SubscriptionDto {
    id: String,
    host: String,
    user_agent: Option<String>,
    created_at: String,
    last_success_at: Option<String>,
}

impl From<&PushSubscriptionInfo> for SubscriptionDto {
    fn from(info: &PushSubscriptionInfo) -> Self {
        Self {
            id: info.id.hyphenated().to_string(),
            host: info.host.clone(),
            user_agent: info.user_agent.clone(),
            created_at: info.created_at.clone(),
            last_success_at: info.last_success_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct SubscriptionListDto {
    pub items: Vec<SubscriptionDto>,
}
