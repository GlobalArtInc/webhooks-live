use whl_core::config::Config;
use whl_forward::{ForwardWorker, GuardConfig, GuardedClient, WorkerConfig};
use whl_store::Store;

use crate::error::ServerError;

#[derive(Debug)]
pub struct ForwardRuntime {
    enabled: bool,
    concurrency: usize,
    max_attempts: u32,
    guard: GuardConfig,
    client: Option<GuardedClient>,
}

impl ForwardRuntime {
    pub fn from_config(config: &Config) -> Result<Self, ServerError> {
        let guard = GuardConfig {
            allow: config.forward.allow_cidrs.clone(),
            ..GuardConfig::default()
        };
        let client = if config.forward.enabled {
            Some(GuardedClient::new(guard.clone())?)
        } else {
            None
        };
        Ok(Self {
            enabled: config.forward.enabled,
            concurrency: usize::try_from(config.forward.concurrency).unwrap_or(usize::MAX),
            max_attempts: config.forward.max_attempts,
            guard,
            client,
        })
    }

    pub fn guard(&self) -> GuardConfig {
        self.guard.clone()
    }

    pub fn worker(&self, store: &Store) -> Result<Option<ForwardWorker>, ServerError> {
        let Some(client) = self.client.clone().filter(|_| self.enabled) else {
            return Ok(None);
        };
        let worker = ForwardWorker::new(
            store.clone(),
            client,
            WorkerConfig::new(self.concurrency, self.max_attempts),
        )?;
        Ok(Some(worker))
    }
}
