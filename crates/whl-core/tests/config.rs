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
    assert_eq!(config.auth.oidc.scopes, ["openid", "email", "profile"]);
    assert_eq!(config.capture.retention, Duration::from_secs(7 * 86_400));
    assert_eq!(config.capture.max_captures, 500);
    assert_eq!(config.capture.max_body_bytes, ByteSize::mib(10));
    assert_eq!(config.capture.body_memory_budget, ByteSize::mib(256));
    assert_eq!(config.capture.max_delayed, 2000);
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
fn duration_values_parse_from_humantime_strings() {
    let config = load(&with_required(&[("WEBHOOKS_LIVE_SESSION_IDLE", "90m")]))
        .expect("humantime duration is valid");

    assert_eq!(config.auth.session_idle, Duration::from_secs(90 * 60));
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
    assert_eq!(config.shutdown.drain, Duration::from_millis(300));
    assert_eq!(config.log, "debug");
}
