use std::collections::BTreeMap;
use std::time::Duration;

use bytesize::ByteSize;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::de;
use crate::tier::{ANONYMOUS_TIER, MEMBER_TIER, TierLimits, Tiers};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TierPatch {
    endpoints: Option<u32>,
    max_captures: Option<u32>,
    #[serde(deserialize_with = "de::optional_duration")]
    max_retention: Option<Duration>,
    never_expire_endpoints: Option<u32>,
    #[serde(deserialize_with = "de::optional_byte_size")]
    storage_bytes: Option<ByteSize>,
    rules_per_endpoint: Option<u32>,
    steps_per_rule: Option<u32>,
    sync_http_steps: Option<u32>,
    #[serde(deserialize_with = "de::optional_duration")]
    script_before: Option<Duration>,
    #[serde(deserialize_with = "de::optional_duration")]
    script_after: Option<Duration>,
    outbound_per_endpoint_hour: Option<u32>,
    outbound_per_network_day: Option<u32>,
    outbound_per_user_hour: Option<u32>,
    notify_per_endpoint_hour: Option<u32>,
    notify_per_user_hour: Option<u32>,
    email_notifications_per_day: Option<u32>,
    notification_kinds: Option<Vec<String>>,
    email_in_per_hour: Option<u32>,
    dns_per_minute: Option<u32>,
    schedules: Option<u32>,
    #[serde(deserialize_with = "de::optional_duration")]
    schedule_min_interval: Option<Duration>,
    aliases: Option<u32>,
    tokens: Option<u32>,
    folders: Option<u32>,
    exports_per_hour: Option<u32>,
    #[serde(deserialize_with = "de::optional_byte_size")]
    export_max_bytes: Option<ByteSize>,
    imports_per_day: Option<u32>,
    #[serde(deserialize_with = "de::optional_byte_size")]
    import_max_bytes: Option<ByteSize>,
}

impl TierPatch {
    fn apply(self, limits: &mut TierLimits) {
        replace(&mut limits.endpoints, self.endpoints);
        replace(&mut limits.max_captures, self.max_captures);
        replace(&mut limits.max_retention, self.max_retention);
        replace(
            &mut limits.never_expire_endpoints,
            self.never_expire_endpoints,
        );
        if let Some(bytes) = self.storage_bytes {
            limits.storage_bytes = Some(bytes.as_u64());
        }
        replace(&mut limits.rules_per_endpoint, self.rules_per_endpoint);
        replace(&mut limits.steps_per_rule, self.steps_per_rule);
        replace(&mut limits.sync_http_steps, self.sync_http_steps);
        replace(&mut limits.script_before, self.script_before);
        replace(&mut limits.script_after, self.script_after);
        set(
            &mut limits.outbound_per_endpoint_hour,
            self.outbound_per_endpoint_hour,
        );
        set(
            &mut limits.outbound_per_network_day,
            self.outbound_per_network_day,
        );
        set(
            &mut limits.outbound_per_user_hour,
            self.outbound_per_user_hour,
        );
        set(
            &mut limits.notify_per_endpoint_hour,
            self.notify_per_endpoint_hour,
        );
        set(&mut limits.notify_per_user_hour, self.notify_per_user_hour);
        replace(
            &mut limits.email_notifications_per_day,
            self.email_notifications_per_day,
        );
        replace(&mut limits.notification_kinds, self.notification_kinds);
        replace(&mut limits.email_in_per_hour, self.email_in_per_hour);
        replace(&mut limits.dns_per_minute, self.dns_per_minute);
        replace(&mut limits.schedules, self.schedules);
        replace(
            &mut limits.schedule_min_interval,
            self.schedule_min_interval,
        );
        replace(&mut limits.aliases, self.aliases);
        replace(&mut limits.tokens, self.tokens);
        replace(&mut limits.folders, self.folders);
        replace(&mut limits.exports_per_hour, self.exports_per_hour);
        if let Some(bytes) = self.export_max_bytes {
            limits.export_max_bytes = bytes.as_u64();
        }
        replace(&mut limits.imports_per_day, self.imports_per_day);
        if let Some(bytes) = self.import_max_bytes {
            limits.import_max_bytes = bytes.as_u64();
        }
    }
}

fn replace<T>(slot: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn set<T>(slot: &mut Option<T>, value: Option<T>) {
    if value.is_some() {
        *slot = value;
    }
}

pub fn table<'de, D>(deserializer: D) -> Result<Tiers, D::Error>
where
    D: Deserializer<'de>,
{
    let mut patches = BTreeMap::<String, TierPatch>::deserialize(deserializer)?;
    let mut tiers = Tiers::default();
    for (name, base) in [
        (ANONYMOUS_TIER, TierLimits::anonymous()),
        (MEMBER_TIER, TierLimits::member()),
    ] {
        if let Some(patch) = patches.remove(name) {
            let mut limits = base;
            patch.apply(&mut limits);
            tiers = tiers.with_limits(name, limits).map_err(D::Error::custom)?;
        }
    }
    let member = tiers.member().limits().clone();
    for (name, patch) in patches {
        let mut limits = member.clone();
        patch.apply(&mut limits);
        tiers = tiers.with_limits(&name, limits).map_err(D::Error::custom)?;
    }
    Ok(tiers)
}
