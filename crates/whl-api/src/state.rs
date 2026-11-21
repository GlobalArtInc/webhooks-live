use whl_store::Store;

use crate::settings::ApiSettings;

#[derive(Clone)]
pub struct ApiState {
    pub(crate) store: Store,
    pub(crate) settings: ApiSettings,
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        Self { store, settings }
    }
}
