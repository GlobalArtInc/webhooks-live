use std::sync::Arc;

use url::Url;
use whl_api::{EmailApi, EmailApiLimits, NotifyApiSettings};
use whl_core::config::Config;
use whl_forward::GuardConfig;
use whl_notify::{
    EmailLimits, MailSettings, Mailer, NotifyLimits, NotifySettings, NotifyWorker, Transport,
    VendorHosts,
};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::error::ServerError;

pub struct NotifyRuntime {
    enabled: bool,
    keyring: Option<Arc<Keyring>>,
    settings: NotifySettings,
    guard: GuardConfig,
    proxy: Option<Url>,
    mailer: Option<Mailer>,
    email_limits: EmailApiLimits,
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
        let mut settings = NotifySettings::new(
            config.public_url.as_str().trim_end_matches('/').to_owned(),
            limits,
            usize::try_from(notify.concurrency).unwrap_or(usize::MAX),
            notify.max_attempts,
        );
        settings.email = EmailLimits {
            per_user_per_day: to_limit(notify.email_per_day),
            per_address_per_day: to_limit(notify.email_per_address_per_day),
        };
        settings.validate()?;
        let mailer = match (&notify.smtp_url, &notify.from) {
            (Some(url), Some(from)) => Some(Mailer::new(&MailSettings::new(
                url.unpack().clone(),
                from.clone(),
            ))?),
            (None, None) => None,
            _ => return Err(ServerError::MailIncomplete),
        };
        let email_limits = EmailApiLimits {
            verifications_per_user_per_day: to_limit(notify.email_verifications_per_day),
            verifications_per_address_per_day: to_limit(
                notify.email_verifications_per_address_per_day,
            ),
            verifications_per_domain_per_day: to_limit(
                notify.email_verifications_per_domain_per_day,
            ),
            verifications_per_day_global: to_limit(notify.email_verifications_per_day_global),
            ..EmailApiLimits::default()
        };
        Ok(Self {
            enabled: notify.enabled,
            keyring,
            settings,
            guard,
            proxy: notify
                .vendor_proxy
                .as_ref()
                .map(|proxy| proxy.unpack().clone()),
            mailer,
            email_limits,
        })
    }

    pub fn api_settings(&self) -> NotifyApiSettings {
        NotifyApiSettings {
            enabled: self.enabled && self.keyring.is_some(),
        }
    }

    pub fn email_api(&self) -> Option<EmailApi> {
        let mailer = self.mailer.clone()?;
        (self.enabled && self.keyring.is_some()).then(|| EmailApi::new(mailer, self.email_limits))
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
        let transport = match &self.mailer {
            Some(mailer) => transport.with_mailer(mailer.clone()),
            None => transport,
        };
        let worker = NotifyWorker::new(store.clone(), keyring, transport, self.settings.clone())?;
        Ok(Some(worker))
    }
}

fn to_limit(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
