mod channels;
mod deliveries;
mod model;
mod quota;
mod wake;

use sqlx::{Postgres, Transaction};
use whl_core::endpoint_id::EndpointId;

pub use channels::{
    ChannelPatch, ConfigChange, CreateChannelOutcome, DeleteChannelOutcome,
    MAX_CHANNELS_PER_ENDPOINT, NewChannel, UpdateChannelOutcome,
};
pub use deliveries::{
    ClaimPolicy, ClaimedDelivery, DeliveryChannel, DeliveryResult, NotifyCapture, TestOutcome,
};
pub use model::{Channel, ChannelKind, Delivery, DeliveryKind, DeliveryState, ParseError, Trigger};
pub use quota::{QuotaCap, QuotaVerdict};
pub use wake::NotifyWakeListener;

pub const WAKE_CHANNEL: &str = "whl_notify_wake";
pub const MAX_ERROR_CHARS: usize = 512;
pub const MAX_MESSAGE_CHARS: usize = 4096;
pub const TEST_BURST: i64 = 5;

#[derive(Debug, thiserror::Error)]
pub enum NotifyStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the notification query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored notification data is invalid: {0}")]
    Invalid(String),
}

impl NotifyStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl From<crate::forward::ForwardStoreError> for NotifyStoreError {
    fn from(error: crate::forward::ForwardStoreError) -> Self {
        match error {
            crate::forward::ForwardStoreError::Unavailable(source) => Self::Unavailable(source),
            crate::forward::ForwardStoreError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

impl From<ParseError> for NotifyStoreError {
    fn from(error: ParseError) -> Self {
        Self::Invalid(error.to_string())
    }
}

pub(crate) fn classify(error: sqlx::Error) -> NotifyStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        NotifyStoreError::Unavailable(error)
    } else {
        NotifyStoreError::Query(error)
    }
}

const INSERT_CAPTURE_DELIVERIES: &str = "WITH inserted AS ( \
       INSERT INTO notification_deliveries (id, channel_id, capture_id, kind, state) \
       SELECT gen_random_uuid(), c.id, $2, 'single', 'pending' \
       FROM notification_channels c \
       WHERE c.endpoint_id = $1 AND c.enabled AND c.trigger = 'capture' \
       RETURNING 1) \
     SELECT pg_notify($3, '') FROM (SELECT 1 FROM inserted LIMIT 1) AS first_delivery";

pub const HOP_HEADER: &str = "x-webhooks-live-hop";

fn is_relayed(headers: &[crate::captures::NameValue]) -> bool {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(HOP_HEADER))
        .any(|header| {
            header
                .value
                .trim()
                .parse::<u32>()
                .map_or(true, |hop| hop >= 1)
        })
}

pub(crate) async fn insert_capture_deliveries(
    tx: &mut Transaction<'_, Postgres>,
    endpoint: &EndpointId,
    capture: sqlx::types::Uuid,
    headers: &[crate::captures::NameValue],
) -> Result<(), sqlx::Error> {
    if is_relayed(headers) {
        return Ok(());
    }
    sqlx::query(INSERT_CAPTURE_DELIVERIES)
        .bind(endpoint.unpack())
        .bind(capture)
        .bind(WAKE_CHANNEL)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}
