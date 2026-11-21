use whl_auth::AccessControl;
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
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        Self {
            store,
            settings,
            limits: None,
            forward: ForwardSettings::default(),
            access: AccessControl::open(),
        }
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
