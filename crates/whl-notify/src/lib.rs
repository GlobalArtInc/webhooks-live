mod adapter;
mod notice;
mod secret;
mod settings;
mod target;
mod transport;
mod vendor;
mod worker;

pub use adapter::{
    Adapter, AdapterInput, DISCORD_CONTENT_CHARS, Destination, PrepareError, Prepared,
    SLACK_TEXT_CHARS, TELEGRAM_TEXT_UNITS, adapter_for, defuse_mentions, escape_discord,
    escape_slack, truncate_utf16,
};
pub use notice::{ComposeInput, MAX_NOTICE_CHARS, Notice, compose};
pub use secret::channel_secret_name;
pub use settings::{
    DEFAULT_HOUSEKEEPING_INTERVAL, DEFAULT_LEASE, DEFAULT_POLL_INTERVAL, MAX_ATTEMPTS_LIMIT,
    MAX_CLAIM_BATCH, NotifyLimits, NotifySettings, SettingsError,
};
pub use target::{
    MASK_SUFFIX, MAX_CHANNEL_URL_BYTES, MAX_CHAT_ID_BYTES, TargetRefusal, is_masked, mask_url,
    targets_instance, validate_bot_token, validate_chat_id, validate_discord_url,
    validate_slack_url,
};
pub use transport::{DELIVERY_HEADER, HOP_HEADER, REPLY_BODY_BYTES, Reply, SendError, Transport};
pub use vendor::{PinError, VendorHosts};
pub use worker::{NotifyHandle, NotifyWorker};
