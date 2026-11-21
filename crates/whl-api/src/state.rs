use std::sync::Arc;

use whl_auth::{AccessControl, ViewAccess};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::forward::ForwardSettings;
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
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        let access =
            AccessControl::open().with_views(ViewAccess::new(store.clone(), settings.retention));
        Self {
            store,
            settings,
            limits: None,
            forward: ForwardSettings::default(),
            access,
            secrets: None,
        }
    }

    pub fn with_secrets(mut self, keyring: Arc<Keyring>) -> Self {
        self.secrets = Some(keyring);
        self
    }

    pub fn secrets_enabled(&self) -> bool {
        self.secrets.is_some()
    }

    pub fn with_access(mut self, access: AccessControl) -> Self {
        self.access = if access.views().is_some() {
            access
        } else {
            access.with_views(ViewAccess::new(self.store.clone(), self.settings.retention))
        };
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
