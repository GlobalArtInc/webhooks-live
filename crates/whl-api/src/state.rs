use std::sync::Arc;
use std::time::Duration;

use whl_auth::{AccessControl, Caller, Creation, ViewAccess};
use whl_core::endpoint_id::EndpointId;
use whl_core::tier::{Tier, Tiers};
use whl_notify::PushService;
use whl_secrets::Keyring;
use whl_store::Store;

use crate::composer::ComposerSettings;
use crate::error::ApiError;
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
    pub(crate) push: Option<PushService>,
    pub(crate) tiers: Tiers,
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
            push: None,
            tiers: Tiers::default(),
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

    pub fn with_push(mut self, push: PushService) -> Self {
        self.push = Some(push);
        self
    }

    pub fn with_tiers(mut self, tiers: Tiers) -> Self {
        self.tiers = tiers;
        self
    }

    pub(crate) fn tier_of(&self, caller: &Caller) -> Tier<'_> {
        self.tiers
            .for_caller(caller.user().map(|user| user.tier.as_str()))
    }

    pub(crate) fn creation_tier(&self, caller: &Caller, creation: &Creation) -> Tier<'_> {
        match creation {
            Creation::Owned(_) => self.tier_of(caller),
            Creation::Anonymous { .. } => self.tiers.anonymous(),
        }
    }

    pub(crate) async fn endpoint_tier(&self, endpoint: &EndpointId) -> Result<Tier<'_>, ApiError> {
        let owner = if self.access.has_users() {
            self.store.endpoint_owner(endpoint).await?
        } else {
            None
        };
        Ok(self
            .tiers
            .for_caller(owner.as_ref().map(|user| user.tier.as_str())))
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
