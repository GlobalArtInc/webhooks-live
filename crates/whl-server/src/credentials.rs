use std::fmt;

use whl_secrets::{KeyId, Keyring};
use whl_store::{EndpointError, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleCredentials {
    pub key_id: Option<KeyId>,
    pub endpoints: u64,
    pub key_available: bool,
}

impl fmt::Display for StaleCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.key_id, self.key_available) {
            (Some(key), true) => write!(
                f,
                "{} endpoints hold capture credentials under key {key}: they cannot be re-keyed and must be set again before that key leaves the key set",
                self.endpoints
            ),
            (Some(key), false) => write!(
                f,
                "{} endpoints hold capture credentials under key {key}, which is not in the key set: their senders are refused until the credentials are set again",
                self.endpoints
            ),
            (None, _) => write!(
                f,
                "{} endpoints hold malformed capture credentials: their senders are refused until the credentials are set again",
                self.endpoints
            ),
        }
    }
}

pub async fn stale_capture_credentials(
    store: &Store,
    keyring: Option<&Keyring>,
) -> Result<Vec<StaleCredentials>, EndpointError> {
    let usage = store.capture_auth_key_usage().await?;
    Ok(usage
        .into_iter()
        .filter_map(|used| {
            let current = keyring.is_some_and(|ring| used.key_id == Some(ring.active_id()));
            let available =
                keyring.is_some_and(|ring| used.key_id.is_some_and(|key| ring.contains(key)));
            (!current).then_some(StaleCredentials {
                key_id: used.key_id,
                endpoints: used.endpoints,
                key_available: available,
            })
        })
        .collect())
}

pub(crate) async fn warn_unusable_capture_credentials(store: &Store, keyring: Option<&Keyring>) {
    match stale_capture_credentials(store, keyring).await {
        Ok(stale) => {
            for entry in stale.iter().filter(|entry| !entry.key_available) {
                tracing::warn!("{entry}");
            }
        }
        Err(error) => {
            tracing::warn!(%error, "capture credentials could not be checked against the key set");
        }
    }
}
