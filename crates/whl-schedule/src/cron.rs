use std::time::Duration;

use croner::Cron;
use croner::errors::CronError as CronLibraryError;
use croner::parser::{CronParser, Seconds, Year};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use time::OffsetDateTime;

pub const MAX_CRON_BYTES: usize = 256;
pub const MAX_TIMEZONE_BYTES: usize = 64;
pub const FIELD_COUNT: usize = 5;
pub const OCCURRENCES_CHECKED: usize = 10;

const MAX_DETAIL_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CronError {
    #[error("the expression is longer than {MAX_CRON_BYTES} bytes")]
    TooLong,
    #[error("the expression must have exactly {FIELD_COUNT} fields")]
    FieldCount,
    #[error("the expression is not valid: {0}")]
    Invalid(String),
    #[error("the time zone is not an IANA name")]
    UnknownTimezone,
    #[error("the expression has no future occurrence")]
    NoOccurrence,
    #[error("occurrences are {found:?} apart, the minimum is {minimum:?}")]
    IntervalTooShort { found: Duration, minimum: Duration },
    #[error("the time is outside the supported range")]
    OutOfRange,
}

#[derive(Debug, Clone)]
pub struct CronSchedule {
    cron: Cron,
    zone: TimeZone,
}

impl CronSchedule {
    pub fn parse(expression: &str, timezone: &str) -> Result<Self, CronError> {
        if expression.len() > MAX_CRON_BYTES {
            return Err(CronError::TooLong);
        }
        if expression.split_whitespace().count() != FIELD_COUNT {
            return Err(CronError::FieldCount);
        }
        let zone = parse_zone(timezone)?;
        let cron = CronParser::builder()
            .seconds(Seconds::Disallowed)
            .year(Year::Disallowed)
            .build()
            .parse(expression)
            .map_err(|error| CronError::Invalid(detail(&error)))?;
        Ok(Self { cron, zone })
    }

    pub fn next_after(&self, after: OffsetDateTime) -> Result<OffsetDateTime, CronError> {
        let start = self.zoned(after)?;
        let next = self
            .cron
            .find_next_occurrence(&start, false)
            .map_err(search_error)?;
        OffsetDateTime::from_unix_timestamp(next.timestamp().as_second())
            .map_err(|_| CronError::OutOfRange)
    }

    pub fn check_interval(&self, from: OffsetDateTime, minimum: Duration) -> Result<(), CronError> {
        let mut current = self.zoned(from)?;
        let mut previous: Option<Zoned> = None;
        for index in 0..OCCURRENCES_CHECKED {
            let next = match self.cron.find_next_occurrence(&current, false) {
                Ok(next) => next,
                Err(error) if index == 0 => return Err(search_error(error)),
                Err(_) => return Ok(()),
            };
            if let Some(previous) = &previous {
                let gap = (next.timestamp().as_second() - previous.timestamp().as_second()).max(0);
                let found = Duration::from_secs(u64::try_from(gap).unwrap_or(0));
                if found < minimum {
                    return Err(CronError::IntervalTooShort { found, minimum });
                }
            }
            previous = Some(next.clone());
            current = next;
        }
        Ok(())
    }

    fn zoned(&self, moment: OffsetDateTime) -> Result<Zoned, CronError> {
        let timestamp =
            Timestamp::from_second(moment.unix_timestamp()).map_err(|_| CronError::OutOfRange)?;
        Ok(timestamp.to_zoned(self.zone.clone()))
    }
}

pub fn parse_zone(timezone: &str) -> Result<TimeZone, CronError> {
    if timezone.len() > MAX_TIMEZONE_BYTES || !timezone.is_ascii() {
        return Err(CronError::UnknownTimezone);
    }
    TimeZone::get(timezone).map_err(|_| CronError::UnknownTimezone)
}

fn search_error(error: CronLibraryError) -> CronError {
    match error {
        CronLibraryError::TimeSearchLimitExceeded
        | CronLibraryError::InvalidDate
        | CronLibraryError::InvalidTime => CronError::NoOccurrence,
        other => CronError::Invalid(detail(&other)),
    }
}

fn detail(error: &CronLibraryError) -> String {
    error.to_string().chars().take(MAX_DETAIL_CHARS).collect()
}
