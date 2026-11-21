use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_core::endpoint_id::EndpointId;

use super::error::{ImportError, Place, Problem};
use crate::encode::{DOCUMENT_FORMAT, DOCUMENT_VERSION};

#[derive(Debug, Clone, PartialEq)]
pub struct ImportHeader {
    pub exported_at: OffsetDateTime,
    pub endpoint: Map<String, Value>,
}

#[derive(Default)]
pub struct HeaderBuilder {
    format: Option<Value>,
    version: Option<Value>,
    exported_at: Option<Value>,
    endpoint: Option<Value>,
}

impl HeaderBuilder {
    pub fn set(&mut self, place: Place, key: &str, value: Value) -> Result<(), ImportError> {
        let slot = match key {
            "format" => &mut self.format,
            "version" => &mut self.version,
            "exported_at" => &mut self.exported_at,
            "endpoint" => &mut self.endpoint,
            _ => {
                return Err(ImportError::invalid(
                    place,
                    &shown(key),
                    Problem::NotAllowed,
                    "is not a known field",
                ));
            }
        };
        if slot.is_some() {
            return Err(ImportError::invalid(
                place,
                key,
                Problem::NotAllowed,
                "is repeated",
            ));
        }
        *slot = Some(value);
        Ok(())
    }

    pub fn finish(self, place: Place) -> Result<ImportHeader, ImportError> {
        let missing =
            |path: &str| ImportError::invalid(place, path, Problem::Required, "is required");
        let format = self.format.ok_or_else(|| missing("format"))?;
        if format.as_str() != Some(DOCUMENT_FORMAT) {
            return Err(ImportError::invalid(
                place,
                "format",
                Problem::InvalidFormat,
                "must be webhooks-live-export",
            ));
        }
        let version = self.version.ok_or_else(|| missing("version"))?;
        if version.as_u64() != Some(u64::from(DOCUMENT_VERSION)) {
            return Err(ImportError::invalid(
                place,
                "version",
                Problem::OutOfRange,
                "must be 1",
            ));
        }
        let exported_at = self.exported_at.ok_or_else(|| missing("exported_at"))?;
        let exported_at = exported_at
            .as_str()
            .filter(|text| text.len() <= 64)
            .and_then(|text| OffsetDateTime::parse(text, &Rfc3339).ok())
            .ok_or_else(|| {
                ImportError::invalid(
                    place,
                    "exported_at",
                    Problem::InvalidFormat,
                    "must be an RFC 3339 timestamp",
                )
            })?;
        let endpoint = self.endpoint.ok_or_else(|| missing("endpoint"))?;
        let Value::Object(mut endpoint) = endpoint else {
            return Err(ImportError::invalid(
                place,
                "endpoint",
                Problem::InvalidFormat,
                "must be an object",
            ));
        };
        let id = endpoint
            .remove("id")
            .ok_or_else(|| missing("endpoint.id"))?;
        if id
            .as_str()
            .is_none_or(|text| EndpointId::parse(text).is_err())
        {
            return Err(ImportError::invalid(
                place,
                "endpoint.id",
                Problem::InvalidFormat,
                "must be an endpoint id",
            ));
        }
        Ok(ImportHeader {
            exported_at,
            endpoint,
        })
    }
}

fn shown(key: &str) -> String {
    key.chars().take(64).collect()
}
