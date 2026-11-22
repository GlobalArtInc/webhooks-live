use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub const ANONYMOUS_TIER: &str = "anonymous";
pub const MEMBER_TIER: &str = "member";
pub const MAX_TIER_NAME_LEN: usize = 32;
pub const MIN_TIER_RETENTION: Duration = Duration::from_secs(3600);
pub const MIN_TIER_SCHEDULE_INTERVAL: Duration = Duration::from_secs(60);
pub const NOTIFICATION_KINDS: [&str; 6] =
    ["webhook", "slack", "discord", "telegram", "email", "push"];

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const DAY: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TierError {
    #[error(
        "tier name {0:?} must be 1 to 32 characters of a-z, 0-9, _ and -, starting with a letter"
    )]
    Name(String),
    #[error("tier {tier}: {field} {reason}")]
    Field {
        tier: String,
        field: &'static str,
        reason: &'static str,
    },
    #[error("tier {tier}: {kind:?} is not a notification channel kind")]
    NotificationKind { tier: String, kind: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierLimits {
    pub endpoints: u32,
    pub max_captures: u32,
    pub max_retention: Duration,
    pub never_expire_endpoints: u32,
    pub storage_bytes: Option<u64>,
    pub rules_per_endpoint: u32,
    pub steps_per_rule: u32,
    pub sync_http_steps: u32,
    pub script_before: Duration,
    pub script_after: Duration,
    pub outbound_per_endpoint_hour: Option<u32>,
    pub outbound_per_network_day: Option<u32>,
    pub outbound_per_user_hour: Option<u32>,
    pub notify_per_endpoint_hour: Option<u32>,
    pub notify_per_user_hour: Option<u32>,
    pub email_notifications_per_day: u32,
    pub notification_kinds: Vec<String>,
    pub email_in_per_hour: u32,
    pub dns_per_minute: u32,
    pub schedules: u32,
    pub schedule_min_interval: Duration,
    pub aliases: u32,
    pub tokens: u32,
    pub folders: u32,
    pub exports_per_hour: u32,
    pub export_max_bytes: u64,
    pub imports_per_day: u32,
    pub import_max_bytes: u64,
}

impl TierLimits {
    pub fn anonymous() -> Self {
        Self {
            endpoints: 20,
            max_captures: 500,
            max_retention: Duration::from_secs(7 * DAY),
            never_expire_endpoints: 0,
            storage_bytes: None,
            rules_per_endpoint: 5,
            steps_per_rule: 10,
            sync_http_steps: 0,
            script_before: Duration::from_millis(50),
            script_after: Duration::from_millis(200),
            outbound_per_endpoint_hour: Some(200),
            outbound_per_network_day: Some(1000),
            outbound_per_user_hour: None,
            notify_per_endpoint_hour: Some(30),
            notify_per_user_hour: None,
            email_notifications_per_day: 0,
            notification_kinds: ["webhook", "slack", "discord", "telegram"]
                .map(str::to_owned)
                .to_vec(),
            email_in_per_hour: 30,
            dns_per_minute: 60,
            schedules: 0,
            schedule_min_interval: Duration::from_secs(300),
            aliases: 0,
            tokens: 0,
            folders: 0,
            exports_per_hour: 10,
            export_max_bytes: 256 * MIB,
            imports_per_day: 0,
            import_max_bytes: 0,
        }
    }

    pub fn member() -> Self {
        Self {
            endpoints: 200,
            max_captures: 10_000,
            max_retention: Duration::from_secs(365 * DAY),
            never_expire_endpoints: 10,
            storage_bytes: Some(2 * GIB),
            rules_per_endpoint: 25,
            steps_per_rule: 20,
            sync_http_steps: 0,
            script_before: Duration::from_millis(100),
            script_after: Duration::from_secs(1),
            outbound_per_endpoint_hour: None,
            outbound_per_network_day: None,
            outbound_per_user_hour: Some(2000),
            notify_per_endpoint_hour: None,
            notify_per_user_hour: Some(300),
            email_notifications_per_day: 50,
            notification_kinds: NOTIFICATION_KINDS.map(str::to_owned).to_vec(),
            email_in_per_hour: 600,
            dns_per_minute: 600,
            schedules: 5,
            schedule_min_interval: Duration::from_secs(300),
            aliases: 5,
            tokens: 20,
            folders: 50,
            exports_per_hour: 30,
            export_max_bytes: 2 * GIB,
            imports_per_day: 10,
            import_max_bytes: 100 * MIB,
        }
    }

    pub fn allows_notification_kind(&self, kind: &str) -> bool {
        self.notification_kinds
            .iter()
            .any(|allowed| allowed == kind)
    }

    pub fn max_retention_seconds(&self) -> i64 {
        i64::try_from(self.max_retention.as_secs()).unwrap_or(i64::MAX)
    }

    fn validate(&self, tier: &str) -> Result<(), TierError> {
        let refuse = |field: &'static str, reason: &'static str| TierError::Field {
            tier: tier.to_owned(),
            field,
            reason,
        };
        if self.max_captures == 0 || i32::try_from(self.max_captures).is_err() {
            return Err(refuse("max_captures", "must be between 1 and 2147483647"));
        }
        if self.max_retention < MIN_TIER_RETENTION
            || i32::try_from(self.max_retention.as_secs()).is_err()
        {
            return Err(refuse(
                "max_retention",
                "must be between 1 hour and 2147483647 seconds",
            ));
        }
        if self.schedule_min_interval < MIN_TIER_SCHEDULE_INTERVAL {
            return Err(refuse(
                "schedule_min_interval",
                "must be at least 60 seconds",
            ));
        }
        if self.script_before.is_zero() {
            return Err(refuse("script_before", "must not be zero"));
        }
        if self.script_after.is_zero() {
            return Err(refuse("script_after", "must not be zero"));
        }
        let fits_i32 = |value: Option<u32>| value.is_none_or(|value| i32::try_from(value).is_ok());
        if !fits_i32(self.notify_per_endpoint_hour) {
            return Err(refuse(
                "notify_per_endpoint_hour",
                "must be at most 2147483647",
            ));
        }
        if !fits_i32(self.notify_per_user_hour) {
            return Err(refuse("notify_per_user_hour", "must be at most 2147483647"));
        }
        if self.storage_bytes == Some(0) {
            return Err(refuse("storage_bytes", "must be greater than zero"));
        }
        if self
            .storage_bytes
            .is_some_and(|bytes| i64::try_from(bytes).is_err())
        {
            return Err(refuse("storage_bytes", "must fit 63 bits"));
        }
        if i64::try_from(self.export_max_bytes).is_err() {
            return Err(refuse("export_max_bytes", "must fit 63 bits"));
        }
        if i64::try_from(self.import_max_bytes).is_err() {
            return Err(refuse("import_max_bytes", "must fit 63 bits"));
        }
        for kind in &self.notification_kinds {
            if !NOTIFICATION_KINDS.contains(&kind.as_str()) {
                return Err(TierError::NotificationKind {
                    tier: tier.to_owned(),
                    kind: kind.clone(),
                });
            }
        }
        Ok(())
    }
}

pub fn is_valid_tier_name(name: &str) -> bool {
    let mut characters = name.chars();
    name.len() <= MAX_TIER_NAME_LEN
        && characters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|next| {
            next.is_ascii_lowercase() || next.is_ascii_digit() || next == '_' || next == '-'
        })
}

#[derive(Debug, Clone, Copy)]
pub struct Tier<'a> {
    name: &'a str,
    limits: &'a TierLimits,
}

impl<'a> Tier<'a> {
    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn limits(&self) -> &'a TierLimits {
        self.limits
    }

    pub fn is_anonymous(&self) -> bool {
        self.name == ANONYMOUS_TIER
    }
}

#[derive(Debug, Clone, Default)]
struct Reported(Arc<Mutex<BTreeSet<String>>>);

impl Reported {
    fn first_time(&self, name: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned())
    }
}

impl PartialEq for Reported {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Reported {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tiers {
    anonymous: TierLimits,
    member: TierLimits,
    custom: BTreeMap<String, TierLimits>,
    unknown: Reported,
}

impl Default for Tiers {
    fn default() -> Self {
        Self {
            anonymous: TierLimits::anonymous(),
            member: TierLimits::member(),
            custom: BTreeMap::new(),
            unknown: Reported::default(),
        }
    }
}

impl Tiers {
    pub fn from_table(table: BTreeMap<String, TierLimits>) -> Result<Self, TierError> {
        let mut tiers = Self::default();
        for (name, limits) in table {
            tiers = tiers.with_limits(&name, limits)?;
        }
        Ok(tiers)
    }

    pub fn with_limits(mut self, name: &str, limits: TierLimits) -> Result<Self, TierError> {
        if !is_valid_tier_name(name) {
            return Err(TierError::Name(name.to_owned()));
        }
        limits.validate(name)?;
        match name {
            ANONYMOUS_TIER => self.anonymous = limits,
            MEMBER_TIER => self.member = limits,
            other => {
                self.custom.insert(other.to_owned(), limits);
            }
        }
        Ok(self)
    }

    pub fn anonymous(&self) -> Tier<'_> {
        Tier {
            name: ANONYMOUS_TIER,
            limits: &self.anonymous,
        }
    }

    pub fn member(&self) -> Tier<'_> {
        Tier {
            name: MEMBER_TIER,
            limits: &self.member,
        }
    }

    pub fn for_user(&self, name: &str) -> Tier<'_> {
        if name == MEMBER_TIER {
            return self.member();
        }
        match self.custom.get_key_value(name) {
            Some((stored, limits)) => Tier {
                name: stored,
                limits,
            },
            None => {
                if self.unknown.first_time(name) {
                    tracing::error!(
                        tier = name,
                        "a user is assigned a tier this instance does not define, applying the anonymous limits"
                    );
                }
                self.anonymous()
            }
        }
    }

    pub fn for_caller(&self, user_tier: Option<&str>) -> Tier<'_> {
        match user_tier {
            Some(name) => self.for_user(name),
            None => self.anonymous(),
        }
    }

    pub fn is_assignable(&self, name: &str) -> bool {
        name == MEMBER_TIER || self.custom.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = Tier<'_>> {
        [self.anonymous(), self.member()].into_iter().chain(
            self.custom
                .iter()
                .map(|(name, limits)| Tier { name, limits }),
        )
    }
}
