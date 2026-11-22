use serde::Serialize;
use utoipa::ToSchema;
use whl_store::notify::{PushSubscriptionInfo, SavedPushSubscription};

#[derive(Serialize, ToSchema)]
#[schema(as = PushPublicKey)]
pub struct PublicKeyDto {
    #[schema(
        example = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
    )]
    pub public_key: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = CreatedPushSubscription)]
pub struct CreatedDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d73"
    )]
    id: String,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(
    as = PushSubscription,
    description = "A registered browser push subscription. The endpoint URL and keys are never returned. `user_agent` is the browser's user agent at registration, null when none was sent; `last_success_at` is null until a push succeeds."
)]
pub struct SubscriptionDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d73"
    )]
    id: String,
    #[schema(example = "fcm.googleapis.com")]
    host: String,
    user_agent: Option<String>,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = PushSubscriptionList)]
pub struct SubscriptionListDto {
    pub items: Vec<SubscriptionDto>,
}

#[derive(ToSchema)]
#[schema(
    as = PushSubscriptionInput,
    description = "The browser's `PushSubscription` JSON, as returned by `pushManager.subscribe()`. Other keys answer 422 `not_allowed`. `expirationTime` is the browser's expiry in milliseconds since the epoch, or null."
)]
#[allow(dead_code)]
pub struct PushSubscriptionInputDoc {
    #[schema(max_length = 2048, example = "https://fcm.googleapis.com/fcm/send/abc")]
    endpoint: String,
    #[serde(rename = "expirationTime")]
    #[schema(value_type = Option<i64>)]
    expiration_time: Option<i64>,
    keys: PushSubscriptionKeysDoc,
}

#[derive(ToSchema)]
#[schema(
    as = PushSubscriptionKeys,
    description = "The browser's keys, both base64url: `p256dh` is the uncompressed P-256 public point (65 bytes), `auth` is the 16-byte auth secret. Other keys answer 422 `not_allowed`."
)]
#[allow(dead_code)]
pub struct PushSubscriptionKeysDoc {
    #[schema(max_length = 128)]
    p256dh: String,
    #[schema(max_length = 128)]
    auth: String,
}
