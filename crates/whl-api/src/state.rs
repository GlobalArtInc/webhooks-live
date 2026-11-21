use std::sync::Arc;
use std::time::Duration;

use whl_auth::{AccessControl, ViewAccess};
use whl_secrets::Keyring;
use whl_store::Store;

use crate::composer::ComposerSettings;
use crate::export::ExportSlots;
use crate::forward::ForwardSettings;
use crate::import::{IMPORT_COOLDOWN, ImportCooldown, import_slots};
use crate::limit::ApiLimits;
use crate::notify::{EmailApi, NotifyApiSettings};
use crate::schedules::ScheduleSettings;
use crate::settings::{ApiSettings, DEFAULT_ALIAS_QUARANTINE};

#[derive(Clone)]
pub struct ApiState {
    pub(crate) store: Store,
    pub(crate) settings: ApiSettings,
    pub(crate) limits: Option<ApiLimits>,
    pub(crate) forward: ForwardSettings,
    pub(crate) access: AccessControl,
    pub(crate) secrets: Option<Arc<Keyring>>,
    pub(crate) exports: ExportSlots,
    pub(crate) composer: ComposerSettings,
    pub(crate) schedules: ScheduleSettings,
    pub(crate) notifications: NotifyApiSettings,
    pub(crate) imports: ExportSlots,
    pub(crate) import_cooldown: ImportCooldown,
    pub(crate) alias_quarantine: Duration,
    pub(crate) email: Option<EmailApi>,
}

impl ApiState {
    pub fn new(store: Store, settings: ApiSettings) -> Self {
        let exports = ExportSlots::new(settings.export_concurrency);
        let access =
            AccessControl::open().with_views(ViewAccess::new(store.clone(), settings.retention));
        Self {
            store,
            settings,
            limits: None,
            forward: ForwardSettings::default(),
            access,
            secrets: None,
            exports,
            composer: ComposerSettings::default(),
            schedules: ScheduleSettings::default(),
            notifications: NotifyApiSettings::default(),
            imports: import_slots(),
            import_cooldown: ImportCooldown::new(IMPORT_COOLDOWN),
            alias_quarantine: DEFAULT_ALIAS_QUARANTINE,
            email: None,
        }
    }

    pub fn with_import_cooldown(mut self, window: std::time::Duration) -> Self {
        self.import_cooldown = ImportCooldown::new(window);
        self
    }

    pub fn with_alias_quarantine(mut self, quarantine: Duration) -> Self {
        self.alias_quarantine = quarantine;
        self
    }

    pub fn with_secrets(mut self, keyring: Arc<Keyring>) -> Self {
        self.secrets = Some(keyring);
        self
    }

    pub fn with_schedules(mut self, schedules: ScheduleSettings) -> Self {
        self.schedules = schedules;
        self
    }

    pub fn with_notifications(mut self, notifications: NotifyApiSettings) -> Self {
        self.notifications = notifications;
        self
    }

    pub fn with_email(mut self, email: EmailApi) -> Self {
        self.email = Some(email);
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

    pub fn with_composer(mut self, composer: ComposerSettings) -> Self {
        self.composer = composer;
        self
    }

    pub fn with_forward(mut self, forward: ForwardSettings) -> Self {
        self.forward = forward;
        self
    }
}
