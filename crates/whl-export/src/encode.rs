use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::macros::format_description;
use whl_store::{CaptureDetail, CaptureForm, EndpointRecord, NameValue};

use crate::csv;
use crate::error::ExportError;
use crate::format::ExportFormat;

pub const DOCUMENT_FORMAT: &str = "webhooks-live-export";
pub const DOCUMENT_VERSION: u8 = 1;
pub const INDEX_ENTRY_NAME: &str = "captures.ndjson";

const TIMESTAMP: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

#[derive(Serialize)]
struct BodyOut {
    encoding: &'static str,
    data: String,
    truncated: bool,
}

impl BodyOut {
    fn of(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self {
                encoding: "utf8",
                data: text.to_owned(),
                truncated: false,
            },
            Err(_) => Self {
                encoding: "base64",
                data: STANDARD.encode(bytes),
                truncated: false,
            },
        }
    }
}

#[derive(Serialize)]
struct CaptureOut<'a> {
    id: &'a str,
    seq: i64,
    kind: &'static str,
    received_at: &'a str,
    method: &'a str,
    path: &'a str,
    query_string: &'a str,
    url: &'a str,
    host: &'a str,
    client_ip: &'a str,
    content_type: Option<&'a str>,
    body_size: i32,
    status_override: Option<i16>,
    response_status: i16,
    headers: &'a [NameValue],
    query: &'a [NameValue],
    form: Option<&'a CaptureForm>,
    meta: &'a Value,
    body: Option<BodyOut>,
}

fn capture_out<'a>(detail: &'a CaptureDetail, body: Option<&[u8]>) -> CaptureOut<'a> {
    let summary = &detail.summary;
    CaptureOut {
        id: &summary.id,
        seq: summary.seq,
        kind: summary.kind.as_str(),
        received_at: &summary.received_at,
        method: &summary.method,
        path: &summary.path,
        query_string: &summary.query_string,
        url: &detail.url,
        host: &detail.host,
        client_ip: &summary.client_ip,
        content_type: summary.content_type.as_deref(),
        body_size: summary.body_size,
        status_override: summary.status_override,
        response_status: summary.response_status,
        headers: &detail.headers,
        query: &detail.query,
        form: detail.form.as_ref(),
        meta: &detail.meta,
        body: body.map(BodyOut::of),
    }
}

#[derive(Serialize)]
struct HeaderOut {
    name: String,
    value: String,
}

#[derive(Serialize)]
struct ResponseOut {
    status: u16,
    content_type: String,
    headers: Vec<HeaderOut>,
    body: BodyOut,
    delay_ms: u64,
}

#[derive(Serialize)]
struct EndpointOut {
    id: String,
    title: Option<String>,
    response: ResponseOut,
    cors: bool,
    overflow: &'static str,
}

fn endpoint_out(record: &EndpointRecord) -> EndpointOut {
    let settings = &record.settings;
    EndpointOut {
        id: record.id.unpack().to_owned(),
        title: settings.title.clone(),
        response: ResponseOut {
            status: settings.response.status().unpack(),
            content_type: settings.response.content_type().unpack().to_owned(),
            headers: settings
                .response
                .headers()
                .iter()
                .map(|header| HeaderOut {
                    name: header.name.unpack().to_owned(),
                    value: header.value.unpack().to_owned(),
                })
                .collect(),
            body: BodyOut::of(settings.response.body()),
            delay_ms: u64::try_from(settings.response.delay().as_millis()).unwrap_or(u64::MAX),
        },
        cors: settings.cors,
        overflow: settings.overflow.as_str(),
    }
}

#[derive(Serialize)]
struct HeadOut {
    format: &'static str,
    version: u8,
    exported_at: String,
    endpoint: EndpointOut,
}

pub struct Encoder {
    format: ExportFormat,
    first: bool,
}

impl Encoder {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            first: true,
        }
    }

    pub fn header(
        &self,
        endpoint: &EndpointRecord,
        exported_at: OffsetDateTime,
    ) -> Result<Vec<u8>, ExportError> {
        match self.format {
            ExportFormat::Csv => Ok(format!("{}{}", csv::HEADER_ROW, csv::NEWLINE).into_bytes()),
            ExportFormat::Json => {
                let mut text = serde_json::to_string(&head(endpoint, exported_at))?;
                text.pop();
                text.push_str(",\"captures\":[");
                Ok(text.into_bytes())
            }
            ExportFormat::Ndjson => {
                let mut line = serde_json::to_vec(&head(endpoint, exported_at))?;
                line.push(b'\n');
                Ok(line)
            }
            ExportFormat::Zip => Ok(Vec::new()),
        }
    }

    pub fn capture(&mut self, detail: &CaptureDetail, body: &[u8]) -> Result<Vec<u8>, ExportError> {
        let mut bytes = Vec::new();
        match self.format {
            ExportFormat::Json => {
                if !self.first {
                    bytes.push(b',');
                }
                bytes.extend(serde_json::to_vec(&capture_out(detail, Some(body)))?);
            }
            ExportFormat::Ndjson => {
                bytes.extend(serde_json::to_vec(&capture_out(detail, Some(body)))?);
                bytes.push(b'\n');
            }
            ExportFormat::Csv => {
                bytes.extend(csv::row(detail, body)?.into_bytes());
                bytes.extend_from_slice(csv::NEWLINE.as_bytes());
            }
            ExportFormat::Zip => {}
        }
        self.first = false;
        Ok(bytes)
    }

    pub fn footer(&self) -> Vec<u8> {
        match self.format {
            ExportFormat::Json => b"]}".to_vec(),
            _ => Vec::new(),
        }
    }
}

pub fn index_line(detail: &CaptureDetail) -> Result<Vec<u8>, ExportError> {
    let mut line = serde_json::to_vec(&capture_out(detail, None))?;
    line.push(b'\n');
    Ok(line)
}

pub fn body_entry_name(seq: i64, capture_id: &str) -> String {
    format!("bodies/{seq}-{capture_id}.bin")
}

fn head(endpoint: &EndpointRecord, exported_at: OffsetDateTime) -> HeadOut {
    HeadOut {
        format: DOCUMENT_FORMAT,
        version: DOCUMENT_VERSION,
        exported_at: exported_at.format(TIMESTAMP).unwrap_or_default(),
        endpoint: endpoint_out(endpoint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_use_only_the_sequence_and_the_capture_id() {
        assert_eq!(
            body_entry_name(42, "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
            "bodies/42-0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b.bin"
        );
    }
}
