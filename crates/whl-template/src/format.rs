use time::{OffsetDateTime, UtcOffset};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FormatProblem {
    LoneEscape,
    UnknownSpecifier(char),
    OutOfRange,
    TooLarge,
}

fn abbreviation(name: &str) -> &str {
    name.get(..3).unwrap_or(name)
}

fn push_specifier(
    out: &mut String,
    specifier: char,
    instant: OffsetDateTime,
) -> Result<(), FormatProblem> {
    let month = usize::from(u8::from(instant.month()));
    let month_name = MONTHS.get(month.saturating_sub(1)).copied().unwrap_or("");
    let weekday = usize::from(instant.weekday().number_days_from_monday());
    let weekday_name = WEEKDAYS.get(weekday).copied().unwrap_or("");
    let hour12 = match instant.hour() % 12 {
        0 => 12,
        other => other,
    };
    let text = match specifier {
        'Y' => format!("{:04}", instant.year()),
        'y' => format!("{:02}", instant.year().rem_euclid(100)),
        'm' => format!("{month:02}"),
        'd' => format!("{:02}", instant.day()),
        'e' => format!("{:2}", instant.day()),
        'H' => format!("{:02}", instant.hour()),
        'I' => format!("{hour12:02}"),
        'M' => format!("{:02}", instant.minute()),
        'S' => format!("{:02}", instant.second()),
        'L' => format!("{:03}", instant.millisecond()),
        'f' => format!("{:06}", instant.microsecond()),
        'p' => (if instant.hour() < 12 { "AM" } else { "PM" }).to_owned(),
        'a' => abbreviation(weekday_name).to_owned(),
        'A' => weekday_name.to_owned(),
        'b' => abbreviation(month_name).to_owned(),
        'B' => month_name.to_owned(),
        'j' => format!("{:03}", instant.ordinal()),
        'u' => instant.weekday().number_from_monday().to_string(),
        'z' => "+0000".to_owned(),
        's' => instant.unix_timestamp().to_string(),
        'F' => format!("{:04}-{month:02}-{:02}", instant.year(), instant.day()),
        'T' => format!(
            "{:02}:{:02}:{:02}",
            instant.hour(),
            instant.minute(),
            instant.second()
        ),
        '%' => "%".to_owned(),
        other => {
            return Err(FormatProblem::UnknownSpecifier(other));
        }
    };
    out.push_str(&text);
    Ok(())
}

pub(crate) fn render(
    format: &str,
    instant: OffsetDateTime,
    max_bytes: usize,
) -> Result<String, FormatProblem> {
    let utc = instant
        .checked_to_offset(UtcOffset::UTC)
        .ok_or(FormatProblem::OutOfRange)?;
    let mut out = String::new();
    let mut characters = format.chars();
    while let Some(character) = characters.next() {
        if character == '%' {
            let Some(specifier) = characters.next() else {
                return Err(FormatProblem::LoneEscape);
            };
            push_specifier(&mut out, specifier, utc)?;
        } else {
            out.push(character);
        }
        if out.len() > max_bytes {
            return Err(FormatProblem::TooLarge);
        }
    }
    Ok(out)
}

pub(crate) fn validate(format: &str) -> Result<(), String> {
    match render(format, OffsetDateTime::UNIX_EPOCH, usize::MAX) {
        Ok(_) => Ok(()),
        Err(FormatProblem::LoneEscape) => Err("date format ends with a lone %".to_owned()),
        Err(FormatProblem::UnknownSpecifier(specifier)) => {
            Err(format!("unknown date specifier %{specifier}"))
        }
        Err(FormatProblem::OutOfRange) => Err("date is out of range".to_owned()),
        Err(FormatProblem::TooLarge) => Err("date format output is too large".to_owned()),
    }
}
