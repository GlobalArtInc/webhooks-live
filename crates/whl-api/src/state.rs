use std::sync::Arc;

use whl_auth::AccessControl;
use whl_secrets::Keyring;
use whl_store::Store;

use crate::export::ExportSlots;
use crate::forward::ForwardSettings;
use crate::import::{IMPORT_COOLDOWN, ImportCooldown, import_slots};
use crate::limit::ApiLimits;
use crate::settings::ApiSettings;

#[derive(Clone)]
pub struct ApiState {
    pub(crate) store: Store,
    pub(crate) settings: ApiSettings,
    pub(crate) limits: Option<ApiLimits>,
    pub(crate) forward: ForwardSettings,
    pub(crate) access: AccessControl,
    pub(crate) secrets: Option<Arc<Keyring>>,
    pub(crate) exports: ExportSlots,
    pub(crate) imports: ExportSlots,
    pub(crate) import_cooldown: ImportCooldown,
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        let exports = ExportSlots::new(settings.export_concurrency);
        Self {
            store,
            settings,
            limits: None,
            forward: ForwardSettings::default(),
            access: AccessControl::open(),
            secrets: None,
            exports,
            imports: import_slots(),
            import_cooldown: ImportCooldown::new(IMPORT_COOLDOWN),
        }
    }

    pub fn with_import_cooldown(mut self, window: std::time::Duration) -> Self {
        self.import_cooldown = ImportCooldown::new(window);
        self
    }

    pub fn with_secrets(mut self, keyring: Arc<Keyring>) -> Self {
        self.secrets = Some(keyring);
        self
    }

    pub fn secrets_enabled(&self) -> bool {
        self.secrets.is_some()
    }

    pub fn with_access(mut self, access: AccessControl) -> Self {
        self.access = access;
        self
    }

    pub fn access(&self) -> &AccessControl {
        &self.access
    }

    pub fn with_limits(mut self, limits: ApiLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn with_forward(mut self, forward: ForwardSettings) -> Self {
        self.forward = forward;
        self
    }
}
