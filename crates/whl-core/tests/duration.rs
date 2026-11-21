use std::time::Duration;

use whl_core::duration::{DurationError, parse_duration};

#[test]
fn parses_each_unit() {
    let cases = [
        ("250ms", Duration::from_millis(250)),
        ("5s", Duration::from_secs(5)),
        ("3m", Duration::from_secs(180)),
        ("12h", Duration::from_secs(12 * 3600)),
        ("7d", Duration::from_secs(7 * 86_400)),
        ("2w", Duration::from_secs(14 * 86_400)),
        ("0s", Duration::ZERO),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_duration(text), Ok(expected), "{text}");
    }
}

#[test]
fn parses_compound_values() {
    assert_eq!(parse_duration("1h30m"), Ok(Duration::from_secs(5400)));
    assert_eq!(
        parse_duration("1w2d3h4m5s6ms"),
        Ok(Duration::from_millis(
            604_800_000 + 2 * 86_400_000 + 3 * 3_600_000 + 4 * 60_000 + 5_000 + 6
        ))
    );
    assert_eq!(parse_duration("1s1s"), Ok(Duration::from_secs(2)));
}

#[test]
fn rejects_empty_input() {
    assert_eq!(parse_duration(""), Err(DurationError::Empty));
}

#[test]
fn rejects_unitless_numbers() {
    assert_eq!(parse_duration("30"), Err(DurationError::MissingUnit));
    assert_eq!(parse_duration("1h30"), Err(DurationError::MissingUnit));
    for text in ["1.5h", "5 s", "5\u{e9}", "5-5s"] {
        assert_eq!(
            parse_duration(text),
            Err(DurationError::MissingUnit),
            "{text}"
        );
    }
}

#[test]
fn rejects_negative_signed_and_missing_numbers() {
    for text in ["-5s", "+5s", "s", "ms", "1h-30m", ".5s", "5s "] {
        assert_eq!(
            parse_duration(text),
            Err(DurationError::ExpectedNumber),
            "{text}"
        );
    }
}

#[test]
fn rejects_unknown_units_and_wrong_case() {
    for text in ["5x", "5sec", "5S", "5Ms", "5mo", "5y"] {
        assert_eq!(
            parse_duration(text),
            Err(DurationError::UnknownUnit),
            "{text}"
        );
    }
}

#[test]
fn rejects_whitespace_between_parts() {
    assert_eq!(parse_duration("1h 30m"), Err(DurationError::ExpectedNumber));
}

#[test]
fn rejects_overflow_of_the_number_the_unit_and_the_sum() {
    assert_eq!(
        parse_duration("99999999999999999999999s"),
        Err(DurationError::Overflow)
    );
    assert_eq!(
        parse_duration("18446744073709551615w"),
        Err(DurationError::Overflow)
    );
    assert_eq!(
        parse_duration("18446744073709551615ms1ms"),
        Err(DurationError::Overflow)
    );
    assert_eq!(
        parse_duration("18446744073709551615ms"),
        Ok(Duration::from_millis(u64::MAX))
    );
}
