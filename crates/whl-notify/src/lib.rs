mod adapter;
mod address;
mod disable;
mod mail;
mod mailtext;
mod notice;
mod push;
mod push_delivery;
mod push_encrypt;
mod secret;
mod settings;
mod target;
mod transport;
mod vapid;
mod vendor;
mod worker;

#[cfg(feature = "test-support")]
mod sink;

pub use adapter::{
    Adapter, AdapterInput, DISCORD_CONTENT_CHARS, Destination, MailContext, MailPlan, PrepareError,
    Prepared, SLACK_TEXT_CHARS, TELEGRAM_TEXT_UNITS, adapter_for, defuse_mentions, escape_discord,
    escape_slack, truncate_utf16,
};
pub use address::{
    AddressError, AddressKey, MAX_ADDRESS_BYTES, TOKEN_CHARS, VerificationToken, address_digest,
    address_key, parse_address,
};
pub use disable::{
    DisableTokenError, MAX_DISABLE_TOKEN_BYTES, issue_disable_token, verify_disable_token,
};
pub use mail::{
    DEFAULT_COMMAND_TIMEOUT, DEFAULT_DEADLINE, MailConfigError, MailError, MailSettings, Mailer,
    OutgoingMail,
};
pub use mailtext::{
    ComposedMail, SUBJECT_PREFIX, VERIFICATION_HOURS, compose_mail, disable_endpoint_url,
    disable_page_url, verification_mail, verification_url,
};
pub use notice::{ComposeInput, MAX_NOTICE_CHARS, Notice, compose};
pub use push::{
    HostPatternError, MAX_EXTRA_PUSH_HOSTS, MAX_PAYLOAD_BYTES, PUSH_TTL_SECONDS, PushHeaders,
    PushHosts, PushService, PushTarget, PushUrlRefusal,
};
#[cfg(any(test, feature = "test-support"))]
pub use push_encrypt::decrypt;
pub use push_encrypt::{
    AUTH_SECRET_BYTES, EncryptError, KeyError, MAX_ENCODED_KEY_CHARS, MAX_PLAINTEXT_BYTES,
    P256DH_BYTES, RECORD_SIZE, SALT_BYTES, SubscriptionKeys, decode_auth_secret, decode_p256dh,
    encrypt,
};
pub use secret::channel_secret_name;
pub use settings::{
    DEFAULT_EMAIL_PER_ADDRESS_PER_DAY, DEFAULT_EMAIL_PER_USER_PER_DAY,
    DEFAULT_HOUSEKEEPING_INTERVAL, DEFAULT_LEASE, DEFAULT_POLL_INTERVAL, EmailLimits,
    MAX_ATTEMPTS_LIMIT, MAX_CLAIM_BATCH, NotifyLimits, NotifySettings, SettingsError,
};
pub use target::{
    MASK_SUFFIX, MAX_CHANNEL_URL_BYTES, MAX_CHAT_ID_BYTES, TargetRefusal, is_masked, mask_url,
    targets_instance, validate_bot_token, validate_chat_id, validate_discord_url,
    validate_slack_url,
};
pub use transport::{DELIVERY_HEADER, HOP_HEADER, REPLY_BODY_BYTES, Reply, SendError, Transport};
pub use vapid::{
    MAX_SUBJECT_BYTES, TOKEN_LIFETIME_SECONDS, Vapid, VapidError, VapidSubject,
    load_or_create_vapid,
};
pub use vendor::{PinError, VendorHosts};
pub use worker::{NotifyHandle, NotifyWorker};

#[cfg(feature = "test-support")]
pub use sink::{CapturedMail, MailHeader, SinkMode, SmtpSink};
