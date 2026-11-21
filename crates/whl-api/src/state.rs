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
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        Self {
            store,
            settings,
            limits: None,
            forward: ForwardSettings::default(),
        }
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
