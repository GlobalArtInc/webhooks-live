use std::io;
use std::net::SocketAddr;

use tokio::task::JoinError;
use whl_auth::AuthError;
use whl_capture::StateError;
use whl_core::limiter::LimiterError;
use whl_forward::{ForwardError, WorkerError};
use whl_notify::{MailConfigError, SendError, SettingsError};
use whl_store::StoreError;

use crate::secrets::SecretsSetupError;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("cannot prepare the database: {0}")]
    Database(#[from] StoreError),
    #[error("cannot set up the capture path: {0}")]
    Capture(#[from] StateError),
    #[error("cannot start sso authentication: {0}")]
    Auth(#[from] AuthError),
    #[error("cannot set up the rate limiters: {0}")]
    Limiter(#[from] LimiterError),
    #[error("cannot set up forwarding: {0}")]
    Forward(#[from] ForwardError),
    #[error("the forward worker settings are invalid: {0}")]
    Worker(#[from] WorkerError),
    #[error("the notification settings are invalid: {0}")]
    Notify(#[from] SettingsError),
    #[error("cannot set up notification delivery: {0}")]
    NotifyTransport(#[from] SendError),
    #[error("the email settings are invalid: {0}")]
    Mail(#[from] MailConfigError),
    #[error("WEBHOOKS_LIVE_SMTP_URL and WEBHOOKS_LIVE_NOTIFY_FROM must be set together")]
    MailIncomplete,
    #[error("cannot set up secrets: {0}")]
    Secrets(#[from] SecretsSetupError),
    #[error("cannot bind the {listener} listener on {addr}: {source}")]
    Bind {
        listener: &'static str,
        addr: SocketAddr,
        source: io::Error,
    },
    #[error("the {listener} listener failed: {source}")]
    Serve {
        listener: &'static str,
        source: io::Error,
    },
    #[error("a server task failed to finish: {0}")]
    Join(#[from] JoinError),
}
