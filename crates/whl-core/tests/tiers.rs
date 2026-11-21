use std::time::Duration;

use figment::Jail;
use whl_core::config::{Config, ConfigError};
use whl_core::tier::{ANONYMOUS_TIER, MEMBER_TIER, TierLimits, Tiers};

#[allow(clippy::result_large_err)]
fn load(file: &str) -> Result<Config, ConfigError> {
    load_with(file, &[])
}

#[allow(clippy::result_large_err)]
fn load_with(file: &str, env: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let mut result = None;
    Jail::expect_with(|jail| {
        jail.create_file("webhooks.toml", file)?;
        jail.set_env("WEBHOOKS_LIVE_CONFIG", "webhooks.toml");
        jail.set_env("WEBHOOKS_LIVE_PUBLIC_URL", "https://hooks.example.test");
        jail.set_env("WEBHOOKS_LIVE_DATABASE_URL", "postgres://u:p@localhost/db");
        for (name, value) in env {
            jail.set_env(name, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result.expect("the jail closure always runs")
}

fn refusal(error: ConfigError) -> String {
    match error {
        ConfigError::Invalid { .. } => error.to_string(),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn the_built_in_tiers_carry_the_documented_numbers() {
    let tiers = Tiers::default();
    let anonymous = tiers.anonymous();
    let member = tiers.member();

    assert_eq!(anonymous.name(), ANONYMOUS_TIER);
    assert_eq!(member.name(), MEMBER_TIER);
    assert_eq!(anonymous.limits().endpoints, 20);
    assert_eq!(member.limits().endpoints, 200);
    assert_eq!(anonymous.limits().max_captures, 500);
    assert_eq!(member.limits().max_captures, 10_000);
    assert_eq!(
        member.limits().max_retention,
        Duration::from_secs(365 * 86_400)
    );
    assert_eq!(member.limits().never_expire_endpoints, 10);
    assert_eq!(member.limits().storage_bytes, Some(2 * 1024 * 1024 * 1024));
    assert_eq!(anonymous.limits().storage_bytes, None);
    assert_eq!(member.limits().rules_per_endpoint, 25);
    assert_eq!(anonymous.limits().steps_per_rule, 10);
    assert_eq!(member.limits().sync_http_steps, 2);
    assert_eq!(anonymous.limits().outbound_per_endpoint_hour, Some(200));
    assert_eq!(anonymous.limits().outbound_per_network_day, Some(1000));
    assert_eq!(member.limits().outbound_per_user_hour, Some(2000));
    assert_eq!(member.limits().notify_per_user_hour, Some(300));
    assert_eq!(member.limits().schedules, 5);
    assert_eq!(member.limits().tokens, 20);
    assert_eq!(member.limits().aliases, 5);
    assert_eq!(anonymous.limits().export_max_bytes, 256 * 1024 * 1024);
    assert_eq!(member.limits().exports_per_hour, 30);
    assert!(!anonymous.limits().allows_notification_kind("email"));
    assert!(member.limits().allows_notification_kind("push"));
}

#[test]
fn a_config_file_overrides_one_field_of_a_built_in_tier_and_keeps_the_rest() {
    let config = load("[tiers.member]\nmax_captures = 777\nstorage_bytes = \"1GiB\"\n")
        .expect("the file loads");
    let member = config.tiers.member();

    assert_eq!(member.limits().max_captures, 777);
    assert_eq!(member.limits().storage_bytes, Some(1024 * 1024 * 1024));
    assert_eq!(member.limits().tokens, 20);
    assert_eq!(config.tiers.anonymous().limits().max_captures, 500);
}

#[test]
fn a_new_tier_inherits_the_member_limits_and_applies_its_overrides() {
    let config =
        load("[tiers.pro]\ntokens = 99\nmax_retention = \"30d\"\n").expect("the file loads");
    let pro = config.tiers.for_user("pro");

    assert_eq!(pro.name(), "pro");
    assert_eq!(pro.limits().tokens, 99);
    assert_eq!(pro.limits().max_retention, Duration::from_secs(30 * 86_400));
    assert_eq!(pro.limits().schedules, TierLimits::member().schedules);
    assert!(config.tiers.is_assignable("pro"));
}

#[test]
fn an_unknown_tier_field_fails_the_load() {
    let error = load("[tiers.pro]\ntokenz = 1\n").expect_err("the field is unknown");

    assert!(matches!(error, ConfigError::Load(_)), "{error:?}");
}

#[test]
fn an_invalid_tier_name_or_value_fails_the_load() {
    let name = load("[tiers.\"Bad Name\"]\ntokens = 1\n").expect_err("the name is invalid");
    let value = load("[tiers.pro]\nmax_captures = 0\n").expect_err("the value is invalid");
    let kind = load("[tiers.pro]\nnotification_kinds = [\"carrier-pigeon\"]\n")
        .expect_err("the kind is unknown");

    assert!(matches!(name, ConfigError::Load(_)), "{name:?}");
    assert!(matches!(value, ConfigError::Load(_)), "{value:?}");
    assert!(matches!(kind, ConfigError::Load(_)), "{kind:?}");
}

#[test]
fn a_user_with_an_unknown_tier_fails_closed_to_the_anonymous_limits() {
    let tiers = Tiers::default();

    assert_eq!(tiers.for_user("vanished").name(), ANONYMOUS_TIER);
    assert_eq!(
        tiers.for_user("vanished").limits(),
        tiers.anonymous().limits()
    );
    assert_eq!(tiers.for_user(ANONYMOUS_TIER).name(), ANONYMOUS_TIER);
    assert_eq!(tiers.for_user(MEMBER_TIER).name(), MEMBER_TIER);
    assert_eq!(tiers.for_caller(None).name(), ANONYMOUS_TIER);
    assert!(tiers.is_assignable(MEMBER_TIER));
    assert!(!tiers.is_assignable(ANONYMOUS_TIER));
    assert!(!tiers.is_assignable("vanished"));
}

#[test]
fn a_new_tier_inherits_from_the_configured_member_not_the_built_in_one() {
    let config = load(
        "[tiers.member]\nendpoints = 50\n\n[tiers.trial]\ntokens = 1\n\n[tiers.aaa]\nschedules = 1\n",
    )
    .expect("the file loads");

    for name in ["trial", "aaa"] {
        let tier = config.tiers.for_user(name);
        assert_eq!(tier.name(), name);
        assert_eq!(tier.limits().endpoints, 50);
    }
    assert_eq!(config.tiers.for_user("trial").limits().tokens, 1);
    assert_eq!(config.tiers.for_user("aaa").limits().schedules, 1);
    assert_eq!(config.tiers.anonymous().limits().endpoints, 20);
}

#[test]
fn a_zero_storage_quota_fails_the_load() {
    let error = load("[tiers.member]\nstorage_bytes = 0\n").expect_err("zero is refused");

    assert!(matches!(error, ConfigError::Load(_)), "{error:?}");
}

#[test]
fn a_removed_key_in_the_environment_or_the_file_is_refused_naming_its_replacement() {
    let environment = refusal(
        load_with("", &[("WEBHOOKS_LIVE_COMPOSER_USER_PER_HOUR", "11")])
            .expect_err("the key is removed"),
    );
    let file = refusal(
        load("[schedules]\nmax_per_user = 7\n\n[notify]\nowner_per_hour = 9\n")
            .expect_err("the keys are removed"),
    );

    assert!(
        environment.contains("COMPOSER_USER_PER_HOUR"),
        "{environment}"
    );
    assert!(
        environment.contains("[tiers.member] outbound_per_user_hour"),
        "{environment}"
    );
    assert!(file.contains("schedules.max_per_user"), "{file}");
    assert!(file.contains("[tiers.member] schedules"), "{file}");
    assert!(
        file.contains("[tiers.member] notify_per_user_hour"),
        "{file}"
    );
}

#[test]
fn tiers_cannot_be_set_through_the_environment() {
    let error = refusal(
        load_with("", &[("WEBHOOKS_LIVE_TIERS_MEMBER_TOKENS", "3")])
            .expect_err("the variable is refused"),
    );

    assert!(error.contains("config file only"), "{error}");
}
