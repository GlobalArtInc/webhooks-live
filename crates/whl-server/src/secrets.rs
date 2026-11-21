use whl_core::config::Config;
use whl_secrets::{Keyring, KeyringError};

const KEYS_VARIABLE: &str = "WEBHOOKS_LIVE_SECRETS_KEYS";

#[derive(Debug, thiserror::Error)]
pub enum SecretsSetupError {
    #[error("{KEYS_VARIABLE} is required when WEBHOOKS_LIVE_AUTH_MODE is {mode}")]
    RequiredForMode { mode: &'static str },
    #[error("{KEYS_VARIABLE} is required for this command")]
    RequiredForCommand,
    #[error("{KEYS_VARIABLE} is invalid: {0}")]
    Invalid(#[from] KeyringError),
}

pub fn keyring_from_config(config: &Config) -> Result<Option<Keyring>, SecretsSetupError> {
    let mode = config.auth.mode;
    match config.secrets.keys() {
        Some(keys) => Ok(Some(Keyring::parse(keys.unpack())?)),
        None if mode.needs_secrets() => Err(SecretsSetupError::RequiredForMode {
            mode: mode.as_str(),
        }),
        None => Ok(None),
    }
}

pub fn rotation_keyring(config: &Config) -> Result<Keyring, SecretsSetupError> {
    let keys = config
        .secrets
        .keys()
        .ok_or(SecretsSetupError::RequiredForCommand)?;
    Ok(Keyring::parse(keys.unpack())?)
}
