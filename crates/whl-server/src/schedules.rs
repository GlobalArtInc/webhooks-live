use std::sync::Arc;

use whl_api::ScheduleSettings;
use whl_capture::StorageFull;
use whl_core::config::{AuthMode, Config};
use whl_forward::{GuardConfig, GuardedClient};
use whl_schedule::{MAX_CLAIM_BATCH, RunnerConfig, ScheduleRunner, SystemClock};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::error::ServerError;

pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_WORKERS: u32 = MAX_CLAIM_BATCH as u32;

#[derive(Debug)]
pub struct ScheduleRuntime {
    enabled: bool,
    settings: ScheduleSettings,
    config: RunnerConfig,
    batch: usize,
    client: Option<GuardedClient>,
}

impl ScheduleRuntime {
    pub fn from_config(config: &Config) -> Result<Self, ServerError> {
        let enabled = config.schedules.enabled && config.auth.mode != AuthMode::Open;
        let guard = GuardConfig {
            allow: config.forward.allow_cidrs.clone(),
            max_redirects: 0,
            max_response_bytes: MAX_RESPONSE_BYTES,
            ..GuardConfig::default()
        };
        let client = if enabled {
            Some(GuardedClient::new(guard)?)
        } else {
            None
        };
        let settings = if enabled {
            ScheduleSettings::new(
                usize::try_from(config.schedules.max_per_user).unwrap_or(usize::MAX),
                config.schedules.min_interval,
            )
        } else {
            ScheduleSettings::disabled()
        };
        let runner = RunnerConfig::new(
            config.capture.retention,
            config.capture.max_captures,
            config.public_url.as_str(),
            config.schedules.min_interval,
            config.schedules.runs_per_hour,
        );
        Ok(Self {
            enabled,
            settings,
            config: runner,
            batch: usize::try_from(config.schedules.workers.clamp(1, MAX_WORKERS))
                .unwrap_or(MAX_CLAIM_BATCH),
            client,
        })
    }

    pub fn settings(&self) -> ScheduleSettings {
        self.settings
    }

    pub fn runner(
        &self,
        store: &Store,
        keyring: Option<Arc<Keyring>>,
        storage_full: StorageFull,
    ) -> Result<Option<ScheduleRunner>, ServerError> {
        let Some(client) = self.client.clone().filter(|_| self.enabled) else {
            return Ok(None);
        };
        let mut config = self.config.clone();
        config.batch = self.batch;
        let runner = ScheduleRunner::new(
            store.clone(),
            client,
            keyring,
            Arc::new(SystemClock),
            storage_full,
            config,
        )?;
        Ok(Some(runner))
    }
}
