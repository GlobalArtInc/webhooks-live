use whl_store::Store;

use crate::limit::ApiLimits;
use crate::settings::ApiSettings;

#[derive(Clone)]
pub struct ApiState {
    pub(crate) store: Store,
    pub(crate) settings: ApiSettings,
    pub(crate) limits: Option<ApiLimits>,
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        Self {
            store,
            settings,
            limits: None,
        }
    }

    pub fn with_limits(mut self, limits: ApiLimits) -> Self {
        self.limits = Some(limits);
        self
    }
}
