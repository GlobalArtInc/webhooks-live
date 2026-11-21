use std::sync::Arc;

use url::Url;
use whl_api::{EmailApi, EmailApiLimits, NotifyApiSettings};
use whl_core::config::Config;
use whl_forward::GuardConfig;
use whl_notify::{
    EmailLimits, MailSettings, Mailer, NotifyLimits, NotifySettings, NotifyWorker, PushHosts,
    PushService, Transport, VapidError, VapidSubject, VendorHosts, load_or_create_vapid,
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
    push_hosts: PushHosts,
    vapid_subject: VapidSubject,
    push_possible: bool,
    push: Option<PushService>,
}

impl NotifyRuntime {
    pub fn from_config(
        config: &Config,
        guard: GuardConfig,
        keyring: Option<Arc<Keyring>>,
    ) -> Result<Self, ServerError> {
        let notify = &config.notify;
        let limits = NotifyLimits {
            network_per_hour: to_limit(notify.network_per_hour),
            global_per_hour: to_limit(notify.global_per_hour),
        };
        let mut settings = NotifySettings::new(
            config.public_url.as_str().trim_end_matches('/').to_owned(),
            limits,
            usize::try_from(notify.concurrency).unwrap_or(usize::MAX),
            notify.max_attempts,
        )
        .with_tiers(config.tiers.clone());
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
        let push_hosts = PushHosts::vendors().with_extras(&notify.push_extra_hosts)?;
        let vapid_subject = match &notify.vapid_subject {
            Some(subject) => VapidSubject::parse(subject)?,
            None => VapidSubject::from_public_url(config.public_url.as_str())?,
        };
        Ok(Self {
            enabled: notify.enabled,
            push_possible: notify.enabled && keyring.is_some() && config.auth.mode.needs_oidc(),
            keyring,
            settings,
            guard,
            proxy: notify
                .vendor_proxy
                .as_ref()
                .map(|proxy| proxy.unpack().clone()),
            mailer,
            email_limits,
            push_hosts,
            vapid_subject,
            push: None,
        })
    }

    pub async fn prepare_push(&mut self, store: &Store) -> Result<(), ServerError> {
        let Some(keyring) = self.keyring.clone().filter(|_| self.push_possible) else {
            return Ok(());
        };
        if let Err(error) = store.check_schema_version().await {
            tracing::warn!(%error, "web push is not prepared because the database schema is not ready");
            return Ok(());
        }
        match load_or_create_vapid(store, &keyring, &self.vapid_subject).await {
            Ok(vapid) => {
                tracing::info!("web push is enabled");
                self.push = Some(PushService::new(vapid, self.push_hosts.clone()));
                Ok(())
            }
            Err(error @ (VapidError::Open(_) | VapidError::Corrupt | VapidError::Mismatch)) => {
                tracing::error!(
                    %error,
                    "web push is disabled because the stored VAPID key cannot be used and it is never regenerated automatically; \
                     to recover, run `DELETE FROM push_vapid_keys` and restart (every user must subscribe again, because \
                     existing subscriptions are bound to the old key); run `webhooks-live secrets rotate` before dropping an old secrets key"
                );
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn push(&self) -> Option<PushService> {
        self.push.clone()
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
        let hosts = VendorHosts::production().with_push_hosts(self.push_hosts.clone());
        let transport = Transport::new(self.guard.clone(), self.proxy.as_ref(), hosts)?;
        let transport = match &self.mailer {
            Some(mailer) => transport.with_mailer(mailer.clone()),
            None => transport,
        };
        let worker = NotifyWorker::new(store.clone(), keyring, transport, self.settings.clone())?;
        Ok(Some(match self.push.clone() {
            Some(push) => worker.with_push(push),
            None => worker,
        }))
    }
}

fn to_limit(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use figment::Jail;
    use whl_testkit::TestDb;

    use super::*;

    const KEY_ONE: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const KEY_TWO: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";

    fn runtime(keys: &str) -> NotifyRuntime {
        let mut config = None;
        #[allow(clippy::result_large_err)]
        Jail::expect_with(|jail| {
            for (key, value) in [
                ("WEBHOOKS_LIVE_PUBLIC_URL", "https://whl.test"),
                (
                    "WEBHOOKS_LIVE_DATABASE_URL",
                    "postgres://u:p@127.0.0.1:1/none",
                ),
                ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
                ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.test"),
                ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "client"),
                ("WEBHOOKS_LIVE_OIDC_CLIENT_SECRET", "client-credential"),
                ("WEBHOOKS_LIVE_SECRETS_KEYS", keys),
            ] {
                jail.set_env(key, value);
            }
            config = Some(Config::load().expect("the configuration loads"));
            Ok(())
        });
        let config = config.expect("the jail closure always runs");
        let keyring = Arc::new(Keyring::parse(keys).expect("a valid key set"));
        NotifyRuntime::from_config(&config, GuardConfig::default(), Some(keyring))
            .expect("the runtime is built")
    }

    #[tokio::test]
    async fn an_unopenable_vapid_key_disables_push_without_aborting_and_is_never_replaced() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let mut first = runtime(&format!("1:{KEY_ONE}"));
        first.prepare_push(&store).await.expect("push is prepared");
        let public = first.push().expect("push is on").public_key().to_owned();

        let mut wrong = runtime(&format!("1:{KEY_TWO}"));
        wrong
            .prepare_push(&store)
            .await
            .expect("an unusable key does not abort the start");
        assert!(wrong.push().is_none());
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM push_vapid_keys")
            .fetch_one(db.pool())
            .await
            .expect("the count is readable");
        assert_eq!(rows, 1);

        let mut again = runtime(&format!("1:{KEY_ONE}"));
        again.prepare_push(&store).await.expect("push is prepared");
        assert_eq!(
            again
                .push()
                .expect("the original key still opens")
                .public_key(),
            public
        );
    }
}
