use std::sync::Arc;

use whl_capture::RulesSettings;
use whl_core::config::Config;
use whl_forward::{GuardConfig, GuardedClient, Hardening};
use whl_rules_worker::{DENIED_PORTS, MAX_ATTEMPTS_LIMIT, RulesWorker, WorkerSettings};
use whl_script::{PoolConfig, ScriptPool, allocator_installed};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::error::ServerError;

struct WorkerParts {
    settings: WorkerSettings,
    client: GuardedClient,
}

pub struct RulesRuntimeSetup {
    settings: RulesSettings,
    pool: Option<Arc<ScriptPool>>,
    worker: Option<WorkerParts>,
}

impl RulesRuntimeSetup {
    pub fn from_config(config: &Config) -> Result<Self, ServerError> {
        let rules = &config.rules;
        let memory = usize::try_from(config.script.memory.as_u64()).unwrap_or(usize::MAX);
        let settings = RulesSettings {
            enabled: rules.enabled,
            sync_budget: rules.sync_budget,
            sync_concurrency: usize::try_from(rules.sync_concurrency).unwrap_or(usize::MAX),
            script_memory_bytes: memory,
            cache_bytes: usize::try_from(rules.cache_bytes.as_u64()).unwrap_or(usize::MAX),
            ..RulesSettings::default()
        };
        let pool = if rules.enabled && allocator_installed() {
            let threads = usize::try_from(config.script.threads).unwrap_or(usize::MAX);
            Some(Arc::new(ScriptPool::new(PoolConfig {
                threads,
                ..PoolConfig::default()
            })?))
        } else {
            if rules.enabled {
                tracing::warn!(
                    "the counting allocator is not installed, so script steps are unavailable"
                );
            }
            None
        };
        let worker = if rules.enabled {
            let guard = GuardConfig {
                allow: config.forward.allow_cidrs.clone(),
                max_redirects: 0,
                hardening: Hardening {
                    refuse_dotless_hosts: true,
                    denied_ports: DENIED_PORTS.to_vec(),
                },
                ..GuardConfig::default()
            };
            let mut worker_settings = WorkerSettings::new(
                config.public_url.as_str().trim_end_matches('/'),
                config.capture.retention,
                usize::try_from(rules.workers).unwrap_or(usize::MAX),
                config.forward.max_attempts.clamp(1, MAX_ATTEMPTS_LIMIT),
                config.tiers.clone(),
            );
            worker_settings.script_memory_bytes = memory;
            Some(WorkerParts {
                settings: worker_settings,
                client: GuardedClient::new(guard)?,
            })
        } else {
            None
        };
        Ok(Self {
            settings,
            pool,
            worker,
        })
    }

    pub fn settings(&self) -> RulesSettings {
        self.settings
    }

    pub fn pool(&self) -> Option<Arc<ScriptPool>> {
        self.pool.clone()
    }

    pub fn worker(
        &self,
        store: &Store,
        keyring: Option<Arc<Keyring>>,
    ) -> Result<Option<RulesWorker>, ServerError> {
        let Some(parts) = &self.worker else {
            return Ok(None);
        };
        let worker = RulesWorker::new(
            store.clone(),
            parts.client.clone(),
            keyring,
            self.pool.clone(),
            parts.settings.clone(),
        )?;
        Ok(Some(worker))
    }
}
