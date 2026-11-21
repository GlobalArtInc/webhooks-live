use std::sync::Arc;

use whl_capture::RulesSettings;
use whl_core::config::Config;
use whl_script::{PoolConfig, ScriptPool, allocator_installed};

use crate::error::ServerError;

pub struct RulesRuntimeSetup {
    settings: RulesSettings,
    pool: Option<Arc<ScriptPool>>,
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
        Ok(Self { settings, pool })
    }

    pub fn settings(&self) -> RulesSettings {
        self.settings
    }

    pub fn pool(&self) -> Option<Arc<ScriptPool>> {
        self.pool.clone()
    }
}
