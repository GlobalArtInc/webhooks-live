use time::OffsetDateTime;
use time::macros::format_description;
use whl_core::filename::safe_filename;

const STAMP: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year][month][day][hour][minute][second]");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Ndjson,
    Csv,
    Zip,
}

impl ExportFormat {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "json" => Some(Self::Json),
            "ndjson" => Some(Self::Ndjson),
            "csv" => Some(Self::Csv),
            "zip" => Some(Self::Zip),
            _ => None,
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Ndjson => "application/x-ndjson",
            Self::Csv => "text/csv; charset=utf-8",
            Self::Zip => "application/zip",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
            Self::Zip => "zip",
        }
    }
}

pub fn file_name(endpoint_id: &str, exported_at: OffsetDateTime, format: ExportFormat) -> String {
    let stamp = exported_at.format(STAMP).unwrap_or_default();
    safe_filename(
        &format!("webhooks-live-{endpoint_id}-{stamp}"),
        format.extension(),
    )
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn file_name_carries_the_endpoint_and_the_utc_stamp() {
        let at = datetime!(2026-11-21 9:05:07 UTC);
        assert_eq!(
            file_name("abcdefghjkmnpqrstvwxyz23", at, ExportFormat::Zip),
            "webhooks-live-abcdefghjkmnpqrstvwxyz23-20261121090507.zip"
        );
    }
}
