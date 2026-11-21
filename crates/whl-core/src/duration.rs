use std::time::Duration;

const MILLIS_PER_SECOND: u64 = 1000;
const MILLIS_PER_MINUTE: u64 = 60 * MILLIS_PER_SECOND;
const MILLIS_PER_HOUR: u64 = 60 * MILLIS_PER_MINUTE;
const MILLIS_PER_DAY: u64 = 24 * MILLIS_PER_HOUR;
const MILLIS_PER_WEEK: u64 = 7 * MILLIS_PER_DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DurationError {
    #[error("duration must not be empty")]
    Empty,
    #[error("duration must start each part with a whole number")]
    ExpectedNumber,
    #[error("duration number must be followed by a unit")]
    MissingUnit,
    #[error("duration unit must be one of ms, s, m, h, d, w")]
    UnknownUnit,
    #[error("duration is too large")]
    Overflow,
}

pub fn parse_duration(text: &str) -> Result<Duration, DurationError> {
    if text.is_empty() {
        return Err(DurationError::Empty);
    }
    let mut rest = text.as_bytes();
    let mut total_millis: u64 = 0;
    while !rest.is_empty() {
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 {
            return Err(DurationError::ExpectedNumber);
        }
        let (number_bytes, after_number) = rest.split_at(digits);
        let units = after_number
            .iter()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        if units == 0 {
            return Err(DurationError::MissingUnit);
        }
        let (unit_bytes, after_unit) = after_number.split_at(units);
        let millis_per_unit = millis_per_unit(unit_bytes).ok_or(DurationError::UnknownUnit)?;
        let part = number(number_bytes)?
            .checked_mul(millis_per_unit)
            .ok_or(DurationError::Overflow)?;
        total_millis = total_millis
            .checked_add(part)
            .ok_or(DurationError::Overflow)?;
        rest = after_unit;
    }
    Ok(Duration::from_millis(total_millis))
}

fn number(digits: &[u8]) -> Result<u64, DurationError> {
    digits.iter().try_fold(0_u64, |value, &digit| {
        value
            .checked_mul(10)
            .and_then(|shifted| shifted.checked_add(u64::from(digit - b'0')))
            .ok_or(DurationError::Overflow)
    })
}

fn millis_per_unit(unit: &[u8]) -> Option<u64> {
    match unit {
        b"ms" => Some(1),
        b"s" => Some(MILLIS_PER_SECOND),
        b"m" => Some(MILLIS_PER_MINUTE),
        b"h" => Some(MILLIS_PER_HOUR),
        b"d" => Some(MILLIS_PER_DAY),
        b"w" => Some(MILLIS_PER_WEEK),
        _ => None,
    }
}
