use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::{
    ClientKey, KeyedLimiter, LimiterError, LimiterSettings, Limiters, Verdict, whole_seconds_up,
};
use whl_core::rate::RateSpec;

fn spec(text: &str) -> RateSpec {
    text.parse().expect("valid rate spec")
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("valid address")
}

fn settings(capture_ip: &str, max_keys: usize) -> LimiterSettings {
    LimiterSettings {
        capture_ip: spec(capture_ip),
        capture_endpoint: spec("1000/s:1000"),
        api_ip: spec("1000/s:1000"),
        create_ip: spec("1000/s:1000"),
        max_keys,
    }
}

#[test]
fn burst_is_admitted_then_the_key_is_limited_with_a_wait() {
    let limiter = KeyedLimiter::new(spec("1/s:3"), 100).expect("limiter builds");
    let key = ClientKey::from_ip(ip("198.51.100.1"));
    let remaining: Vec<u32> = (0..3)
        .map(|_| match limiter.check(&key) {
            Verdict::Allowed { limit, remaining } => {
                assert_eq!(limit, 3);
                remaining
            }
            other => panic!("expected the burst to pass, got {other:?}"),
        })
        .collect();
    assert_eq!(remaining, [2, 1, 0]);
    match limiter.check(&key) {
        Verdict::Limited { limit, retry_after } => {
            assert_eq!(limit, 3);
            assert!(retry_after > Duration::ZERO && retry_after <= Duration::from_secs(1));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn keys_are_limited_independently_and_recover() {
    let limiter = KeyedLimiter::new(spec("50/s:1"), 100).expect("limiter builds");
    let first = ClientKey::from_ip(ip("198.51.100.1"));
    let second = ClientKey::from_ip(ip("198.51.100.2"));
    assert!(limiter.check(&first).is_allowed());
    assert!(!limiter.check(&first).is_allowed());
    assert!(limiter.check(&second).is_allowed());
    std::thread::sleep(Duration::from_millis(60));
    assert!(limiter.check(&first).is_allowed());
}

#[test]
fn ipv6_addresses_share_a_bucket_per_64_prefix() {
    let limiter = KeyedLimiter::new(spec("1/m:1"), 100).expect("limiter builds");
    let first = ClientKey::from_ip(ip("2001:db8:1:2::1"));
    let same_prefix = ClientKey::from_ip(ip("2001:db8:1:2:ffff::9"));
    let other_prefix = ClientKey::from_ip(ip("2001:db8:1:3::1"));
    assert_eq!(first, same_prefix);
    assert_ne!(first, other_prefix);
    assert!(limiter.check(&first).is_allowed());
    assert!(!limiter.check(&same_prefix).is_allowed());
    assert!(limiter.check(&other_prefix).is_allowed());
}

#[test]
fn ipv4_mapped_addresses_share_the_bucket_of_the_ipv4_address() {
    assert_eq!(
        ClientKey::from_ip(ip("::ffff:198.51.100.7")),
        ClientKey::from_ip(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)))
    );
}

#[test]
fn tracked_keys_stay_within_the_cap_under_a_flood_of_distinct_addresses() {
    let cap = 5_000;
    let limiters = Limiters::new(&settings("1000/s:1", cap)).expect("limiters build");
    for index in 0_u32..100_000 {
        let address = IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + index));
        assert!(limiters.check_capture_ip(address).is_allowed());
        assert!(limiters.tracked_keys().capture_ip <= cap);
    }
    assert_eq!(limiters.tracked_keys().capture_ip, cap);
    std::thread::sleep(Duration::from_millis(20));
    limiters.cleanup();
    assert_eq!(limiters.tracked_keys().capture_ip, 0);
    assert!(limiters.check_capture_ip(ip("203.0.113.9")).is_allowed());
    assert_eq!(limiters.tracked_keys().capture_ip, 1);
}

#[test]
fn an_endpoint_key_is_limited_without_touching_other_endpoints() {
    let limiters = Limiters::new(&LimiterSettings {
        capture_endpoint: spec("1/m:1"),
        ..settings("1000/s:1000", 100)
    })
    .expect("limiters build");
    let first = EndpointId::generate().expect("entropy is available");
    let second = EndpointId::generate().expect("entropy is available");
    assert!(limiters.check_capture_endpoint(&first).is_allowed());
    assert!(!limiters.check_capture_endpoint(&first).is_allowed());
    assert!(limiters.check_capture_endpoint(&second).is_allowed());
}

#[test]
fn a_zero_rate_or_capacity_is_refused() {
    let zero = RateSpec {
        count: 0,
        ..spec("1/s:1")
    };
    assert_eq!(
        KeyedLimiter::<ClientKey>::new(zero, 10).err(),
        Some(LimiterError::ZeroRate(zero))
    );
    assert_eq!(
        KeyedLimiter::<ClientKey>::new(spec("1/s:1"), 0).err(),
        Some(LimiterError::ZeroCapacity)
    );
}

#[test]
fn retry_after_is_rounded_up_to_whole_seconds_with_a_floor_of_one() {
    assert_eq!(whole_seconds_up(Duration::ZERO), 1);
    assert_eq!(whole_seconds_up(Duration::from_millis(1)), 1);
    assert_eq!(whole_seconds_up(Duration::from_millis(1001)), 2);
    assert_eq!(whole_seconds_up(Duration::from_secs(3)), 3);
}
