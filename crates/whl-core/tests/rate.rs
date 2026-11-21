use whl_core::rate::{RatePeriod, RateSpec, RateSpecError};

#[test]
fn parses_per_second_rate_with_burst() {
    let spec: RateSpec = "30/s:60".parse().expect("valid spec");

    assert_eq!(
        spec,
        RateSpec {
            count: 30,
            period: RatePeriod::Second,
            burst: 60
        }
    );
}

#[test]
fn parses_per_minute_rate_with_burst() {
    let spec: RateSpec = "10/m:10".parse().expect("valid spec");

    assert_eq!(
        spec,
        RateSpec {
            count: 10,
            period: RatePeriod::Minute,
            burst: 10
        }
    );
}

#[test]
fn rejects_unknown_unit() {
    let error = "30/h:60"
        .parse::<RateSpec>()
        .expect_err("hour is not a unit");

    assert_eq!(error, RateSpecError::Unit("30/h:60".to_owned()));
}

#[test]
fn rejects_spec_without_burst() {
    let error = "30/s".parse::<RateSpec>().expect_err("burst is required");

    assert_eq!(error, RateSpecError::Shape("30/s".to_owned()));
}

#[test]
fn rejects_zero_count() {
    let error = "0/s:5"
        .parse::<RateSpec>()
        .expect_err("count must be positive");

    assert_eq!(error, RateSpecError::Count("0/s:5".to_owned()));
}

#[test]
fn rejects_zero_burst() {
    let error = "5/s:0"
        .parse::<RateSpec>()
        .expect_err("burst must be positive");

    assert_eq!(error, RateSpecError::Burst("5/s:0".to_owned()));
}
