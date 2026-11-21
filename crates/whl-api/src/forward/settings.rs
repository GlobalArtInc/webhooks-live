use std::sync::Arc;

use whl_forward::{GuardConfig, LookupHost, SystemLookup};

#[derive(Clone)]
pub struct ForwardSettings {
    pub(crate) guard: GuardConfig,
    pub(crate) lookup: Arc<dyn LookupHost>,
}

impl ForwardSettings {
    pub fn new(guard: GuardConfig, lookup: Arc<dyn LookupHost>) -> Self {
        Self { guard, lookup }
    }

    pub fn with_system_lookup(guard: GuardConfig) -> Self {
        Self::new(guard, Arc::new(SystemLookup))
    }
}

impl Default for ForwardSettings {
    fn default() -> Self {
        Self::with_system_lookup(GuardConfig::default())
    }
}
