use std::net::SocketAddr;
use std::time::Duration;

use bytesize::ByteSize;
use figment::Jail;
use ipnet::IpNet;
use whl_core::config::{AuthMode, Config, ConfigError, Problem};
use whl_core::rate::{RatePeriod, RateSpec};

const PUBLIC_URL: (&str, &str) = ("WEBHOOKS_LIVE_PUBLIC_URL", "https://hooks.example.test");
const DATABASE_URL: (&str, &str) = ("WEBHOOKS_LIVE_DATABASE_URL", "postgres://u:p@localhost/db");

#[allow(clippy::result_large_err)]
fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let mut result = None;
    Jail::expect_with(|jail| {
        for (key, value) in vars {
            jail.set_env(key, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result.expect("the jail closure always runs")
}

fn with_required(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut vars = vec![PUBLIC_URL, DATABASE_URL];
    vars.extend_from_slice(extra);
    vars
}

fn socket(text: &str) -> SocketAddr {
    text.parse().expect("test address is valid")
}

#[test]
fn defaults_apply_when_only_required_keys_are_set() {
    let config = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");

    assert_eq!(config.listen_addr, socket("0.0.0.0:8080"));
    assert_eq!(config.admin_addr, socket("0.0.0.0:9090"));
    assert_eq!(config.log, "info");
    assert!(config.trusted_proxies.is_empty());
    assert_eq!(config.database.pool_size, 20);
    assert_eq!(config.auth.mode, AuthMode::Open);
    assert_eq!(config.auth.session_idle, Duration::from_secs(12 * 3600));
    assert_eq!(config.auth.session_max, Duration::from_secs(7 * 86_400));
    assert_eq!(config.auth.max_pending_logins, 50_000);
    assert_eq!(config.auth.oidc.scopes, ["openid", "email", "profile"]);
    assert_eq!(config.capture.retention, Duration::from_secs(7 * 86_400));
    assert_eq!(config.capture.max_captures, 500);
    assert_eq!(config.capture.max_body_bytes, ByteSize::mib(10));
    assert_eq!(config.capture.body_memory_budget, ByteSize::mib(256));
    assert_eq!(config.capture.max_delayed, 2000);
    assert!(config.capture.ring_mode_allowed);
    assert_eq!(config.auth.oidc.provider_name, None);
    assert_eq!(config.capture.storage_cap_bytes, ByteSize::gib(20));
    assert_eq!(
        config.rate.capture_ip,
        RateSpec {
            count: 30,
            period: RatePeriod::Second,
            burst: 60
        }
    );
    assert_eq!(
        config.rate.capture_endpoint,
        RateSpec {
            count: 20,
            period: RatePeriod::Second,
            burst: 100
        }
    );
    assert_eq!(
        config.rate.api_ip,
        RateSpec {
            count: 20,
            period: RatePeriod::Second,
            burst: 40
        }
    );
    assert_eq!(
        config.rate.create_ip,
        RateSpec {
            count: 10,
            period: RatePeriod::Minute,
            burst: 10
        }
    );
    assert!(config.forward.enabled);
    assert_eq!(config.forward.concurrency, 32);
    assert_eq!(config.forward.max_attempts, 3);
    assert!(config.forward.allow_cidrs.is_empty());
    assert!(config.composer.enabled);
    assert_eq!(config.composer.endpoint_per_hour, 200);
    assert_eq!(config.composer.network_per_day, 1000);
    assert_eq!(config.composer.user_per_hour, 2000);
    assert_eq!(config.composer.max_in_flight, 32);
    assert_eq!(config.shutdown.drain, Duration::from_secs(5));
    assert_eq!(config.shutdown.grace, Duration::from_secs(25));
}

#[test]
#[allow(clippy::result_large_err)]
fn env_overrides_file_which_overrides_default() {
    let mut result = None;
    Jail::expect_with(|jail| {
        jail.create_file(
            "webhooks.toml",
            "[shutdown]\ndrain = \"2s\"\ngrace = \"40s\"\n\n[capture]\nretention = \"1d\"\n",
        )?;
        jail.set_env("WEBHOOKS_LIVE_CONFIG", "webhooks.toml");
        jail.set_env(PUBLIC_URL.0, PUBLIC_URL.1);
        jail.set_env(DATABASE_URL.0, DATABASE_URL.1);
        jail.set_env("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "10s");
        result = Some(Config::load());
        Ok(())
    });
    let config = result
        .expect("the jail closure always runs")
        .expect("config loads from file and env");

    assert_eq!(config.shutdown.grace, Duration::from_secs(10));
    assert_eq!(config.shutdown.drain, Duration::from_secs(2));
    assert_eq!(config.capture.retention, Duration::from_secs(86_400));
    assert_eq!(config.listen_addr, socket("0.0.0.0:8080"));
}

#[test]
fn missing_required_key_is_reported() {
    let error = load(&[DATABASE_URL]).expect_err("PUBLIC_URL is required");

    match &error {
        ConfigError::Invalid { problems } => {
            assert_eq!(problems, &[Problem::MissingRequired("PUBLIC_URL")]);
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        "invalid configuration: WEBHOOKS_LIVE_PUBLIC_URL is required"
    );
}

#[test]
fn every_missing_or_invalid_required_key_is_reported_at_once() {
    let missing = load(&[]).expect_err("both required keys are missing");
    let invalid = load(&[
        ("WEBHOOKS_LIVE_PUBLIC_URL", "not a url"),
        ("WEBHOOKS_LIVE_DATABASE_URL", "also not a url"),
    ])
    .expect_err("both required keys are invalid");
    let mixed = load(&[("WEBHOOKS_LIVE_PUBLIC_URL", "not a url")])
        .expect_err("one invalid and one missing");

    let problems = |error: ConfigError| match error {
        ConfigError::Invalid { problems } => problems,
        other => panic!("expected Invalid, got {other:?}"),
    };
    assert_eq!(
        problems(missing),
        [
            Problem::MissingRequired("PUBLIC_URL"),
            Problem::MissingRequired("DATABASE_URL"),
        ]
    );
    assert_eq!(
        problems(invalid),
        [
            Problem::InvalidRequired("PUBLIC_URL"),
            Problem::InvalidRequired("DATABASE_URL"),
        ]
    );
    assert_eq!(
        problems(mixed),
        [
            Problem::InvalidRequired("PUBLIC_URL"),
            Problem::MissingRequired("DATABASE_URL"),
        ]
    );
}

#[test]
fn sso_without_oidc_keys_reports_all_missing_keys_at_once() {
    let error = load(&with_required(&[("WEBHOOKS_LIVE_AUTH_MODE", "sso")]))
        .expect_err("sso needs OIDC keys");

    match error {
        ConfigError::Invalid { problems } => assert_eq!(
            problems,
            [
                Problem::MissingForSso("OIDC_ISSUER"),
                Problem::MissingForSso("OIDC_CLIENT_ID"),
                Problem::MissingForSso("OIDC_CLIENT_SECRET"),
            ]
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn problems_of(error: ConfigError) -> Vec<Problem> {
    match error {
        ConfigError::Invalid { problems } => problems,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn sso_key_problems_are_reported_together_with_required_key_problems() {
    let missing = load(&[("WEBHOOKS_LIVE_AUTH_MODE", "sso")]).expect_err("everything is missing");
    let invalid = load(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
        ("WEBHOOKS_LIVE_PUBLIC_URL", "not a url"),
        DATABASE_URL,
        ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.example.test"),
    ])
    .expect_err("PUBLIC_URL is invalid and two OIDC keys are missing");

    assert_eq!(
        problems_of(missing),
        [
            Problem::MissingRequired("PUBLIC_URL"),
            Problem::MissingRequired("DATABASE_URL"),
            Problem::MissingForSso("OIDC_ISSUER"),
            Problem::MissingForSso("OIDC_CLIENT_ID"),
            Problem::MissingForSso("OIDC_CLIENT_SECRET"),
        ]
    );
    assert_eq!(
        problems_of(invalid),
        [
            Problem::InvalidRequired("PUBLIC_URL"),
            Problem::MissingForSso("OIDC_CLIENT_ID"),
            Problem::MissingForSso("OIDC_CLIENT_SECRET"),
        ]
    );
}

#[test]
fn open_mode_does_not_report_oidc_keys_next_to_required_key_problems() {
    let error = load(&[]).expect_err("required keys are missing");

    assert_eq!(
        problems_of(error),
        [
            Problem::MissingRequired("PUBLIC_URL"),
            Problem::MissingRequired("DATABASE_URL"),
        ]
    );
}

#[test]
fn ring_mode_is_allowed_by_default_and_can_be_switched_off() {
    let default = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");
    let disabled = load(&with_required(&[(
        "WEBHOOKS_LIVE_RING_MODE_ALLOWED",
        "false",
    )]))
    .expect("the flag is a valid boolean");

    assert!(default.capture.ring_mode_allowed);
    assert!(!disabled.capture.ring_mode_allowed);
}

#[test]
fn api_cors_is_on_by_default_and_can_be_switched_off() {
    let default = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");
    let disabled = load(&with_required(&[("WEBHOOKS_LIVE_API_CORS", "false")]))
        .expect("the flag is a valid boolean");

    assert!(default.api_cors);
    assert!(!disabled.api_cors);
}

#[test]
fn oidc_provider_name_is_optional_and_read_from_env() {
    let default = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");
    let named = load(&with_required(&[(
        "WEBHOOKS_LIVE_OIDC_PROVIDER_NAME",
        "GlobalArt SSO",
    )]))
    .expect("the provider name is plain text");

    assert_eq!(default.auth.oidc.provider_name, None);
    assert_eq!(
        named.auth.oidc.provider_name.as_deref(),
        Some("GlobalArt SSO")
    );
}

#[test]
fn pool_size_below_two_is_reported_and_two_is_accepted() {
    for size in ["0", "1"] {
        let error = load(&with_required(&[("WEBHOOKS_LIVE_DB_POOL_SIZE", size)]))
            .expect_err("the migrator needs two connections");

        assert_eq!(
            problems_of(error),
            [Problem::PoolSizeTooSmall],
            "size {size}"
        );
    }
    let config = load(&with_required(&[("WEBHOOKS_LIVE_DB_POOL_SIZE", "2")]))
        .expect("two connections are enough");

    assert_eq!(config.database.pool_size, 2);
}

#[test]
fn pool_size_problem_is_reported_in_the_same_error_as_the_others() {
    let error = load(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
        ("WEBHOOKS_LIVE_DB_POOL_SIZE", "1"),
    ])
    .expect_err("several keys are wrong");

    assert_eq!(
        problems_of(error),
        [
            Problem::MissingRequired("PUBLIC_URL"),
            Problem::MissingRequired("DATABASE_URL"),
            Problem::MissingForSso("OIDC_ISSUER"),
            Problem::MissingForSso("OIDC_CLIENT_ID"),
            Problem::MissingForSso("OIDC_CLIENT_SECRET"),
            Problem::PoolSizeTooSmall,
        ]
    );
}

#[test]
fn validate_reports_pool_size_and_sso_problems_of_a_built_config() {
    let mut config = load(&with_required(&[])).expect("defaults are valid");
    config.auth.mode = AuthMode::Sso;
    config.database.pool_size = 1;

    let error = config.validate().expect_err("sso keys and pool are wrong");

    assert_eq!(
        problems_of(error),
        [
            Problem::MissingForSso("OIDC_ISSUER"),
            Problem::MissingForSso("OIDC_CLIENT_ID"),
            Problem::MissingForSso("OIDC_CLIENT_SECRET"),
            Problem::PoolSizeTooSmall,
        ]
    );
}

#[test]
fn sso_with_oidc_keys_loads() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
        ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.example.test"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "webhooks"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_SECRET", "secret"),
    ]))
    .expect("all OIDC keys are present");

    assert_eq!(config.auth.mode, AuthMode::Sso);
    assert_eq!(config.auth.oidc.client_id.as_deref(), Some("webhooks"));
}

#[test]
fn duration_values_parse_from_unit_strings_including_compound_ones() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_SESSION_IDLE", "1h30m"),
        ("WEBHOOKS_LIVE_SESSION_MAX", "2w"),
    ]))
    .expect("duration strings are valid");

    assert_eq!(config.auth.session_idle, Duration::from_secs(90 * 60));
    assert_eq!(config.auth.session_max, Duration::from_secs(14 * 86_400));
}

#[test]
fn the_pending_login_cap_is_read_from_the_environment() {
    let config = load(&with_required(&[(
        "WEBHOOKS_LIVE_AUTH_MAX_PENDING_LOGINS",
        "1234",
    )]))
    .expect("the cap is a number");

    assert_eq!(config.auth.max_pending_logins, 1234);
}

#[test]
fn invalid_duration_strings_fail_the_load() {
    for value in ["90", "-5s", "5x", "", "1.5h"] {
        let error = load(&with_required(&[("WEBHOOKS_LIVE_SESSION_IDLE", value)]))
            .expect_err("the duration is invalid");

        assert!(matches!(error, ConfigError::Load(_)), "value {value:?}");
    }
}

#[test]
fn duration_errors_do_not_echo_the_rejected_text() {
    let error = load(&with_required(&[(
        "WEBHOOKS_LIVE_SESSION_IDLE",
        "5parsecs",
    )]))
    .expect_err("the unit is unknown");

    assert!(!error.to_string().contains("parsecs"));
}

#[test]
fn byte_size_values_parse_from_unit_strings() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_MAX_BODY_BYTES", "512KiB"),
        ("WEBHOOKS_LIVE_STORAGE_CAP_BYTES", "1GiB"),
    ]))
    .expect("byte size strings are valid");

    assert_eq!(config.capture.max_body_bytes, ByteSize::kib(512));
    assert_eq!(config.capture.storage_cap_bytes, ByteSize::gib(1));
}

#[test]
fn cidr_lists_accept_a_comma_separated_env_string() {
    let config = load(&with_required(&[
        (
            "WEBHOOKS_LIVE_TRUSTED_PROXIES",
            "10.0.0.0/8, 192.168.1.0/24",
        ),
        ("WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS", ""),
    ]))
    .expect("CIDR lists are valid");

    assert_eq!(
        config.trusted_proxies,
        [
            "10.0.0.0/8".parse::<IpNet>().expect("valid CIDR"),
            "192.168.1.0/24".parse::<IpNet>().expect("valid CIDR")
        ]
    );
    assert!(config.forward.allow_cidrs.is_empty());
}

#[test]
fn env_keys_reach_their_fields() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_DB_POOL_SIZE", "5"),
        ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:18080"),
        ("WEBHOOKS_LIVE_OIDC_SCOPES", "openid profile"),
        ("WEBHOOKS_LIVE_MAX_DELAYED", "7"),
        ("WEBHOOKS_LIVE_RATE_API_IP", "5/s:9"),
        ("WEBHOOKS_LIVE_FORWARD_ENABLED", "false"),
        ("WEBHOOKS_LIVE_FORWARD_MAX_ATTEMPTS", "4"),
        ("WEBHOOKS_LIVE_COMPOSER_ENABLED", "false"),
        ("WEBHOOKS_LIVE_COMPOSER_USER_PER_HOUR", "11"),
        ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "300ms"),
        ("WEBHOOKS_LIVE_LOG", "debug"),
    ]))
    .expect("every key is valid");

    assert_eq!(config.database.pool_size, 5);
    assert_eq!(config.listen_addr, socket("127.0.0.1:18080"));
    assert_eq!(config.auth.oidc.scopes, ["openid", "profile"]);
    assert_eq!(config.capture.max_delayed, 7);
    assert_eq!(
        config.rate.api_ip,
        RateSpec {
            count: 5,
            period: RatePeriod::Second,
            burst: 9
        }
    );
    assert!(!config.forward.enabled);
    assert_eq!(config.forward.max_attempts, 4);
    assert!(!config.composer.enabled);
    assert_eq!(config.composer.user_per_hour, 11);
    assert_eq!(config.shutdown.drain, Duration::from_millis(300));
    assert_eq!(config.log, "debug");
}

#[test]
fn the_alias_quarantine_defaults_to_thirty_days_and_reads_from_the_environment() {
    let config = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");
    assert_eq!(config.alias.quarantine, Duration::from_secs(30 * 86_400));

    let config = load(&with_required(&[("WEBHOOKS_LIVE_ALIAS_QUARANTINE", "12h")]))
        .expect("the quarantine is valid");
    assert_eq!(config.alias.quarantine, Duration::from_secs(12 * 3600));
}

#[test]
fn an_alias_quarantine_shorter_than_the_cache_lifetime_is_refused() {
    let error = load(&with_required(&[("WEBHOOKS_LIVE_ALIAS_QUARANTINE", "29s")]))
        .expect_err("a quarantine below the cache lifetime is refused");
    assert_eq!(problems_of(error), [Problem::AliasQuarantineTooShort]);
    let config = load(&with_required(&[("WEBHOOKS_LIVE_ALIAS_QUARANTINE", "30s")]))
        .expect("the cache lifetime itself is accepted");
    assert_eq!(config.alias.quarantine, Duration::from_secs(30));
}

#[test]
fn mixed_without_oidc_keys_reports_all_missing_keys_at_once() {
    let error = load(&with_required(&[("WEBHOOKS_LIVE_AUTH_MODE", "mixed")]))
        .expect_err("mixed needs OIDC keys");

    assert_eq!(
        problems_of(error),
        [
            Problem::MissingForMixed("OIDC_ISSUER"),
            Problem::MissingForMixed("OIDC_CLIENT_ID"),
            Problem::MissingForMixed("OIDC_CLIENT_SECRET"),
        ]
    );
}

#[test]
fn a_missing_mixed_key_names_the_mode_in_its_message() {
    let error = load(&with_required(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "mixed"),
        ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.example.test"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "webhooks"),
    ]))
    .expect_err("the client secret is missing");

    assert_eq!(
        error.to_string(),
        "invalid configuration: WEBHOOKS_LIVE_OIDC_CLIENT_SECRET is required when WEBHOOKS_LIVE_AUTH_MODE is mixed"
    );
}

#[test]
fn mixed_with_oidc_keys_loads() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "mixed"),
        ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.example.test"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "webhooks"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_SECRET", "secret"),
    ]))
    .expect("all OIDC keys are present");

    assert_eq!(config.auth.mode, AuthMode::Mixed);
    assert!(config.auth.mode.needs_oidc());
    assert!(!AuthMode::Open.needs_oidc());
}

#[test]
fn validate_reports_the_oidc_keys_of_a_built_mixed_config() {
    let mut config = load(&with_required(&[])).expect("defaults are valid");
    config.auth.mode = AuthMode::Mixed;

    let error = config.validate().expect_err("mixed keys are missing");

    assert_eq!(
        problems_of(error),
        [
            Problem::MissingForMixed("OIDC_ISSUER"),
            Problem::MissingForMixed("OIDC_CLIENT_ID"),
            Problem::MissingForMixed("OIDC_CLIENT_SECRET"),
        ]
    );
}

#[test]
fn an_unknown_auth_mode_is_refused() {
    let error = load(&with_required(&[("WEBHOOKS_LIVE_AUTH_MODE", "hybrid")]))
        .expect_err("the mode is unknown");

    assert!(matches!(error, ConfigError::Load(_)), "{error:?}");
}

#[test]
fn dns_capture_is_off_unless_configured() {
    let config = load(&[PUBLIC_URL, DATABASE_URL]).expect("required keys are enough");

    assert!(!config.dns.enabled());
    assert!(config.dns.listen_addr.is_none());
    assert!(config.dns.answer_a.is_none());
}

#[test]
fn dns_keys_reach_their_fields_and_blank_answers_mean_none() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_DNS_LISTEN_ADDR", "0.0.0.0:5353"),
        ("WEBHOOKS_LIVE_DNS_ZONE", "dns.example.test"),
        ("WEBHOOKS_LIVE_DNS_NS_NAME", "ns.example.test"),
        ("WEBHOOKS_LIVE_DNS_SOA_EMAIL", "hostmaster@example.test"),
        (
            "WEBHOOKS_LIVE_DNS_TRUSTED_PROXIES",
            "10.0.0.0/8, 192.168.0.0/16",
        ),
        ("WEBHOOKS_LIVE_DNS_ANSWER_A", "192.0.2.7"),
        ("WEBHOOKS_LIVE_DNS_ANSWER_AAAA", ""),
        ("WEBHOOKS_LIVE_DNS_RATE_SOURCE", "5/s:7"),
    ]))
    .expect("the DNS keys load");

    assert!(config.dns.enabled());
    assert_eq!(config.dns.listen_addr, Some(socket("0.0.0.0:5353")));
    assert_eq!(config.dns.zone.as_deref(), Some("dns.example.test"));
    assert_eq!(config.dns.trusted_proxies.len(), 2);
    assert_eq!(
        config.dns.answer_a,
        Some("192.0.2.7".parse().expect("literal"))
    );
    assert_eq!(config.dns.answer_aaaa, None);
    assert_eq!(
        config.dns.rate_source,
        RateSpec {
            count: 5,
            period: RatePeriod::Second,
            burst: 7
        }
    );
}

#[test]
fn a_half_configured_dns_responder_reports_every_missing_key() {
    let error = load(&with_required(&[(
        "WEBHOOKS_LIVE_DNS_LISTEN_ADDR",
        "0.0.0.0:5353",
    )]))
    .expect_err("the zone, name server and contact are missing");

    assert_eq!(
        problems_of(error),
        vec![
            Problem::MissingForDns("DNS_ZONE"),
            Problem::MissingForDns("DNS_NS_NAME"),
            Problem::MissingForDns("DNS_SOA_EMAIL"),
        ]
    );
}

#[test]
fn an_invalid_dns_answer_address_fails_the_load() {
    let error = load(&with_required(&[(
        "WEBHOOKS_LIVE_DNS_ANSWER_A",
        "not-an-address",
    )]))
    .expect_err("the address is invalid");

    assert!(matches!(error, ConfigError::Load(_)), "{error:?}");
}

#[test]
fn notification_limits_have_defaults_and_read_the_environment() {
    let defaults = load(&with_required(&[])).expect("the defaults load");
    assert!(defaults.notify.enabled);
    assert_eq!(defaults.notify.endpoint_per_hour, 30);
    assert_eq!(defaults.notify.owner_per_hour, 300);
    assert_eq!(defaults.notify.network_per_hour, 60);
    assert_eq!(defaults.notify.global_per_hour, 600);
    assert!(defaults.notify.vendor_proxy.is_none());

    let set = load(&with_required(&[
        ("WEBHOOKS_LIVE_NOTIFY_GLOBAL_PER_HOUR", "50"),
        ("WEBHOOKS_LIVE_VENDOR_PROXY", "http://proxy.test:3128"),
    ]))
    .expect("the keys load");
    assert_eq!(set.notify.global_per_hour, 50);
    let proxy = set.notify.vendor_proxy.expect("the proxy is set");
    assert_eq!(proxy.unpack().host_str(), Some("proxy.test"));
    assert_eq!(format!("{proxy:?}"), "ProxyUrl(<redacted>)");
}

#[test]
fn mail_is_off_by_default() {
    let config = load(&with_required(&[])).expect("defaults are valid");

    assert_eq!(config.mail.listen_addr, None);
    assert_eq!(config.mail.domain, None);
    assert!(config.mail.trusted_proxies.is_empty());
}

#[test]
fn the_mail_keys_are_read_from_the_environment() {
    let config = load(&with_required(&[
        ("WEBHOOKS_LIVE_MAIL_LISTEN_ADDR", "0.0.0.0:2525"),
        ("WEBHOOKS_LIVE_MAIL_DOMAIN", "email.example.test"),
        (
            "WEBHOOKS_LIVE_MAIL_TRUSTED_PROXIES",
            "10.0.0.0/8, 192.0.2.7/32",
        ),
    ]))
    .expect("the mail keys are valid");

    assert_eq!(config.mail.listen_addr, Some(socket("0.0.0.0:2525")));
    assert_eq!(config.mail.domain.as_deref(), Some("email.example.test"));
    assert_eq!(config.mail.trusted_proxies.len(), 2);
}

#[test]
fn a_mail_listener_without_a_domain_is_refused() {
    let error = load(&with_required(&[(
        "WEBHOOKS_LIVE_MAIL_LISTEN_ADDR",
        "0.0.0.0:2525",
    )]))
    .expect_err("the domain is missing");

    assert_eq!(problems_of(error), [Problem::MailNeedsDomain]);
}

#[test]
fn an_invalid_mail_domain_and_a_lone_tls_file_are_refused_together() {
    let error = load(&with_required(&[
        ("WEBHOOKS_LIVE_MAIL_DOMAIN", "bad..domain"),
        ("WEBHOOKS_LIVE_MAIL_TLS_CERT", "/tls/cert.pem"),
    ]))
    .expect_err("both are invalid");

    assert_eq!(
        problems_of(error),
        [Problem::MailInvalidDomain, Problem::MailTlsIncomplete]
    );
}

#[test]
fn email_settings_are_off_by_default_and_the_smtp_url_is_never_printed() {
    let defaults = load(&with_required(&[])).expect("the defaults load");
    assert!(defaults.notify.smtp_url.is_none());
    assert!(defaults.notify.from.is_none());
    assert_eq!(defaults.notify.email_per_day, 50);
    assert_eq!(defaults.notify.email_per_address_per_day, 100);
    assert_eq!(defaults.notify.email_verifications_per_day, 5);
    assert_eq!(defaults.notify.email_verifications_per_address_per_day, 3);
    assert_eq!(defaults.notify.email_verifications_per_domain_per_day, 200);
    assert_eq!(defaults.notify.email_verifications_per_day_global, 2000);

    let set = load(&with_required(&[
        (
            "WEBHOOKS_LIVE_SMTP_URL",
            "smtps://mailer:placeholder-value@smtp.example.test:465",
        ),
        ("WEBHOOKS_LIVE_NOTIFY_FROM", "noreply@hooks.example.test"),
        ("WEBHOOKS_LIVE_NOTIFY_EMAIL_PER_DAY", "7"),
        (
            "WEBHOOKS_LIVE_NOTIFY_EMAIL_VERIFICATIONS_PER_DAY_GLOBAL",
            "9",
        ),
    ]))
    .expect("the keys load");
    let url = set.notify.smtp_url.as_ref().expect("the url is set");
    assert_eq!(url.unpack().host_str(), Some("smtp.example.test"));
    assert_eq!(
        set.notify.from.as_deref(),
        Some("noreply@hooks.example.test")
    );
    assert_eq!(set.notify.email_per_day, 7);
    assert_eq!(set.notify.email_verifications_per_day_global, 9);
    assert!(!format!("{set:?}").contains("placeholder-value"));
}

#[test]
fn web_push_settings_default_to_nothing_and_read_the_environment() {
    let defaults = load(&with_required(&[])).expect("the defaults load");
    assert_eq!(defaults.notify.vapid_subject, None);
    assert!(defaults.notify.push_extra_hosts.is_empty());

    let set = load(&with_required(&[
        (
            "WEBHOOKS_LIVE_VAPID_SUBJECT",
            "mailto:ops@hooks.example.test",
        ),
        (
            "WEBHOOKS_LIVE_PUSH_EXTRA_HOSTS",
            "push.example.test, *.relay.example.test",
        ),
    ]))
    .expect("the keys load");
    assert_eq!(
        set.notify.vapid_subject.as_deref(),
        Some("mailto:ops@hooks.example.test")
    );
    assert_eq!(
        set.notify.push_extra_hosts,
        ["push.example.test", "*.relay.example.test"]
    );
}
