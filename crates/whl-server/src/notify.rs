use std::sync::Arc;

use url::Url;
use whl_api::NotifyApiSettings;
use whl_core::config::Config;
use whl_forward::GuardConfig;
use whl_notify::{NotifyLimits, NotifySettings, NotifyWorker, Transport, VendorHosts};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::error::ServerError;

pub struct NotifyRuntime {
    enabled: bool,
    keyring: Option<Arc<Keyring>>,
    settings: NotifySettings,
    guard: GuardConfig,
    proxy: Option<Url>,
}

impl NotifyRuntime {
    pub fn from_config(
        config: &Config,
        guard: GuardConfig,
        keyring: Option<Arc<Keyring>>,
    ) -> Result<Self, ServerError> {
        let notify = &config.notify;
        let limits = NotifyLimits {
            endpoint_per_hour: to_limit(notify.endpoint_per_hour),
            owner_per_hour: to_limit(notify.owner_per_hour),
            network_per_hour: to_limit(notify.network_per_hour),
            global_per_hour: to_limit(notify.global_per_hour),
        };
        let settings = NotifySettings::new(
            config.public_url.as_str().trim_end_matches('/').to_owned(),
            limits,
            usize::try_from(notify.concurrency).unwrap_or(usize::MAX),
            notify.max_attempts,
        );
        settings.validate()?;
        Ok(Self {
            enabled: notify.enabled,
            keyring,
            settings,
            guard,
            proxy: notify
                .vendor_proxy
                .as_ref()
                .map(|proxy| proxy.unpack().clone()),
        })
    }

    pub fn api_settings(&self) -> NotifyApiSettings {
        NotifyApiSettings {
            enabled: self.enabled && self.keyring.is_some(),
        }
    }

    pub fn worker(&self, store: &Store) -> Result<Option<NotifyWorker>, ServerError> {
        let Some(keyring) = self.keyring.clone().filter(|_| self.enabled) else {
            return Ok(None);
        };
        let transport = Transport::new(
            self.guard.clone(),
            self.proxy.as_ref(),
            VendorHosts::production(),
        )?;
        let worker = NotifyWorker::new(store.clone(), keyring, transport, self.settings.clone())?;
        Ok(Some(worker))
    }
}

fn to_limit(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
